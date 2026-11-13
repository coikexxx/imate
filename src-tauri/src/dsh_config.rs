use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::adapters::{CommandSpec, SupportFile, TurnRequest};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Authentication { #[default] Headless, Account }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Profile { #[default] Web, Headless }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DshSettings { pub authentication: Authentication, pub profile: Profile }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DshModelSelection {
    pub profile: Profile, pub provider: String, pub model: String,
    /// 推理强度（如 high、off）。为空时由 Dsh 配置里的供应商默认值决定。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

fn invalid(message: &str) -> AppError { AppError::Invalid(message.into()) }
fn valid_id(value: &str) -> bool { !value.trim().is_empty() && value.len() <= 512 && !value.chars().any(char::is_control) }
/// 强度标识只允许短的字母数字、连字符和下划线，避免把任意文本写进补丁。
fn valid_effort(value: &str) -> bool {
    !value.is_empty() && value.len() <= 32 && value.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

pub fn parse_selection(value: Option<&str>) -> AppResult<Option<DshModelSelection>> {
    let Some(value) = value.filter(|value| !value.trim().is_empty()) else { return Ok(None); };
    let selection: DshModelSelection = serde_json::from_str(value).map_err(|_| invalid("Dsh 模型选择无效，请从已配置模型列表重新选择"))?;
    if !valid_id(&selection.provider) || !valid_id(&selection.model) { return Err(invalid("Dsh 供应商或模型标识无效，请重新选择")); }
    if selection.reasoning_effort.as_deref().is_some_and(|effort| !valid_effort(effort)) { return Err(invalid("Dsh 推理强度无效，请重新选择")); }
    Ok(Some(selection))
}

pub fn load(conn: &Connection) -> AppResult<DshSettings> {
    let value: Option<String> = conn.query_row("SELECT value FROM settings WHERE key='dsh_settings'", [], |row| row.get(0)).optional()?;
    value.map(|value| serde_json::from_str(&value).map_err(|_| invalid("Dsh 设置无法读取，请重新保存"))).transpose().map(|value| value.unwrap_or_default())
}

pub fn save(conn: &Connection, settings: &DshSettings) -> AppResult<()> {
    let value = serde_json::to_string(settings).map_err(|_| invalid("无法保存 Dsh 设置"))?;
    conn.execute("INSERT INTO settings(key,value) VALUES('dsh_settings',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [value])?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshModel {
    pub id: String, pub name: String,
    /// 该模型声明的可选推理强度，从最弱到最强；没有声明时为空。
    #[serde(default)]
    pub efforts: Vec<DshEffort>,
    /// Dsh 配置里为该模型设置的默认强度。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_effort: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshEffort { pub id: String, pub name: String }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshModelGroup { pub id: String, pub name: String, pub models: Vec<DshModel> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshModelFailure { pub provider: String, pub message: String }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DshCatalog { pub profile: Profile, pub groups: Vec<DshModelGroup>, #[serde(default)] pub failures: Vec<DshModelFailure> }
pub type CatalogCapture = Arc<Mutex<Option<DshCatalog>>>;

impl DshCatalog {
    pub fn from_event(event: Value) -> AppResult<Self> {
        let catalog: Self = serde_json::from_value(event).map_err(|_| invalid("Dsh 模型目录格式无法识别，请核对 CLI 版本"))?;
        if catalog.groups.len() > 128 || catalog.groups.iter().any(|group| !valid_id(&group.id) || !valid_id(&group.name)
            || group.models.len() > 4096 || group.models.iter().any(|model| !valid_id(&model.id) || !valid_id(&model.name)
                || model.efforts.len() > 16 || model.efforts.iter().any(|effort| !valid_effort(&effort.id) || !valid_id(&effort.name))
                || model.default_effort.as_deref().is_some_and(|effort| !valid_effort(effort)))) {
            return Err(invalid("Dsh 模型目录包含无效字段"));
        }
        Ok(catalog)
    }
}

/// 只放置应用自带插件和选择标识；用户配置由 Dsh 在子进程里自行读取。
pub fn configure_command(spec: &mut CommandSpec, request: &TurnRequest, settings: &DshSettings, catalog: bool) {
    if !catalog && request.model.is_none() && settings.authentication == Authentication::Headless { return; }
    let selection = parse_selection(request.model.as_deref());
    let selection_error = selection.is_err();
    let selection = selection.ok().flatten();
    let profile = selection.as_ref().map(|selection| selection.profile).unwrap_or(settings.profile);
    let base = std::env::temp_dir().join(format!("imates-dsh-{}", uuid::Uuid::new_v4()));
    let script: PathBuf = base.with_extension("mjs");
    let patch_path = base.with_extension("json");
    let mut patch = vec![json!({"id":"credentials","disabled":true}), json!({"id":"authorization","disabled":true}),
        json!({"id":"deepseek-account","disabled":true}), json!({"id":"headless-runner","disabled":true})];
    if let Some(selection) = &selection {
        let mut model = json!({"provider":selection.provider,"model":selection.model});
        if let Some(effort) = &selection.reasoning_effort { model["reasoningEffort"] = json!(effort); }
        patch.push(json!({"id":"agent-default-model","config":model}));
    } else if settings.authentication == Authentication::Account && !catalog {
        patch.push(json!({"id":"agent-default-model","config":{"provider":"deepseek-account","model":"deepseek-flash"}}));
    }
    patch.push(json!({"insert":[{"id":"imates-dsh-bridge","name":script,"config":{
        "operation":if catalog {"catalog"} else {"run"},"profile":profile,"selection":selection,
        "selectionError":selection_error,"authentication":settings.authentication}}]}));
    // launcher 参数必须放在 headless 的 --json 等内部参数之前。
    spec.args.splice(1..1, ["--patch".into(), patch_path.to_string_lossy().into_owned()]);
    spec.support_files = vec![SupportFile { path: script, content: include_str!("dsh_bridge.mjs").into() },
        SupportFile { path: patch_path, content: serde_json::to_string(&patch).expect("应用构造的 JSON 可序列化") }];
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::{CliAdapter, RunMode, TurnRequest};

    #[test]
    fn dsh_settings_preserve_legacy_default_and_persist_options() {
        let conn = crate::store::open_in_memory().unwrap();
        assert_eq!(load(&conn).unwrap(), DshSettings::default());
        let settings = DshSettings { authentication: Authentication::Account, profile: Profile::Web };
        save(&conn, &settings).unwrap(); assert_eq!(load(&conn).unwrap(), settings);
        assert!(serde_json::from_str::<DshSettings>(r#"{"authentication":"unknown","profile":"web"}"#).is_err());
        conn.execute("UPDATE settings SET value='损坏的配置' WHERE key='dsh_settings'", []).unwrap();
        assert!(load(&conn).is_err());
        assert_eq!(conn.query_row("SELECT value FROM settings WHERE key='dsh_settings'", [], |row| row.get::<_, String>(0)).unwrap(), "损坏的配置");
    }

    #[test]
    fn dsh_model_selection_is_structured_and_validated() {
        let selection = DshModelSelection { profile: Profile::Web, provider: "gpt".into(), model: "gpt-6".into(), reasoning_effort: None };
        let encoded = serde_json::to_string(&selection).unwrap();
        assert!(!encoded.contains("reasoningEffort"), "未选强度时不写出该字段，旧数据与新数据保持一致");
        assert_eq!(parse_selection(Some(&encoded)).unwrap(), Some(selection));
        assert_eq!(parse_selection(None).unwrap(), None);
        for invalid in ["gpt-6", r#"{"profile":"../web","provider":"gpt","model":"m"}"#,
            r#"{"profile":"web","provider":"","model":"m"}"#, r#"{"profile":"web","provider":"gpt","model":"m\nsecret"}"#,
            r#"{"profile":"web","provider":"gpt","model":"m","reasoningEffort":""}"#,
            r#"{"profile":"web","provider":"gpt","model":"m","reasoningEffort":"high\nx"}"#,
            r#"{"profile":"web","provider":"gpt","model":"m","reasoningEffort":"a b"}"#] {
            assert!(parse_selection(Some(invalid)).is_err(), "{invalid}");
        }
    }

    #[test]
    fn dsh_reasoning_effort_is_kept_in_selection_and_written_to_the_model_patch() {
        let selection = DshModelSelection { profile: Profile::Web, provider: "claude".into(), model: "claude-opus-5-5".into(), reasoning_effort: Some("high".into()) };
        let encoded = serde_json::to_string(&selection).unwrap();
        assert_eq!(parse_selection(Some(&encoded)).unwrap(), Some(selection));
        let patch_of = |model: Option<String>| {
            let request = TurnRequest { prompt: "p".into(), session_id: None, cwd: std::env::temp_dir(), mode: RunMode::Execute, model };
            let adapter = crate::adapters::dsh::DshAdapter::new("dsh".into(), RunMode::Execute);
            let spec = adapter.build_command(&request);
            let patch: serde_json::Value = serde_json::from_str(&spec.support_files[1].content).unwrap();
            patch.as_array().unwrap().iter().find(|row| row["id"] == "agent-default-model").unwrap()["config"].clone()
        };
        let with_effort = patch_of(Some(encoded));
        assert_eq!(with_effort["reasoningEffort"], "high");
        assert_eq!(with_effort["model"], "claude-opus-5-5");
        let without = patch_of(Some(r#"{"profile":"web","provider":"claude","model":"claude-opus-5-5"}"#.into()));
        assert!(without.get("reasoningEffort").is_none(), "没选强度时不覆盖 Dsh 配置里的默认值");
    }

    #[test]
    fn dsh_catalog_carries_model_efforts_and_rejects_bad_ones() {
        let event = |efforts: serde_json::Value| json!({"type":"imates_dsh_catalog","profile":"web","failures":[],
            "groups":[{"id":"claude","name":"claude","models":[{"id":"m","name":"m","efforts":efforts,"defaultEffort":"high"}]}]});
        let catalog = DshCatalog::from_event(event(json!([{"id":"off","name":"off"},{"id":"high","name":"high"}]))).unwrap();
        assert_eq!(catalog.groups[0].models[0].efforts.len(), 2);
        assert_eq!(catalog.groups[0].models[0].default_effort.as_deref(), Some("high"));
        let legacy = DshCatalog::from_event(json!({"type":"imates_dsh_catalog","profile":"web","groups":[{"id":"g","name":"g","models":[{"id":"m","name":"m"}]}]})).unwrap();
        assert!(legacy.groups[0].models[0].efforts.is_empty(), "旧格式的目录没有强度字段也能读取");
        assert!(DshCatalog::from_event(event(json!([{"id":"bad id","name":"x"}]))).is_err());
    }

    #[test]
    fn dsh_account_and_custom_model_commands_use_private_native_overlays() {
        let mut request = TurnRequest { prompt: "私有提示词".into(), session_id: Some("session-safe".into()), cwd: std::env::temp_dir(), mode: RunMode::Execute, model: None };
        let account = crate::adapters::dsh::DshAdapter::new("dsh".into(), RunMode::Execute)
            .with_settings(DshSettings { authentication: Authentication::Account, profile: Profile::Web });
        let spec = account.build_command(&request);
        assert_eq!(spec.support_files.len(), 2);
        assert!(spec.args.iter().any(|arg| arg == "--patch"));
        let patch: serde_json::Value = serde_json::from_str(&spec.support_files[1].content).unwrap();
        let bridge = patch.as_array().unwrap().iter().find_map(|row| row["insert"].as_array()).unwrap();
        assert_eq!(bridge[0]["config"]["authentication"], "account");
        assert!(!spec.support_files.iter().any(|file| file.content.contains(&request.prompt)));
        assert!(spec.support_files.iter().all(|file| !file.path.exists()));
        request.model = Some(r#"{"profile":"web","provider":"gpt","model":"gpt-6"}"#.into());
        let selected = account.build_command(&request);
        let patch: serde_json::Value = serde_json::from_str(&selected.support_files[1].content).unwrap();
        let model = patch.as_array().unwrap().iter().find(|row| row["id"] == "agent-default-model").unwrap();
        assert_eq!(model["config"]["provider"], "gpt"); assert_eq!(model["config"]["model"], "gpt-6");
        assert!(!selected.args.iter().any(|arg| arg == "--model" || arg == "gpt-6"));
        assert!(selected.args.windows(2).any(|pair| pair == ["--session-id", "session-safe"]));
    }
}
