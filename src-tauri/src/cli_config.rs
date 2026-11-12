//! 应用管理：每个 CLI 的启用开关、手动路径、模型白名单与默认模型。
//! 配置存在 `settings` 表，键为 `cli_config:<id>`。没有记录时按默认值处理。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::cli_registry;
use crate::error::{AppError, AppResult};

const KEY_PREFIX: &str = "cli_config:";
const MAX_MODELS: usize = 200;
const MAX_ID_LEN: usize = 512;

/// 白名单里的一个模型。`id` 对后端是不透明字符串（Dsh 的 id 是选择 JSON）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliModel {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub custom: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliConfig {
    pub enabled: bool,
    #[serde(default)]
    pub custom_path: Option<String>,
    /// 为空表示不限制。
    #[serde(default)]
    pub models: Vec<CliModel>,
    #[serde(default)]
    pub default_model: Option<String>,
}

fn invalid(message: impl Into<String>) -> AppError { AppError::Invalid(message.into()) }

fn label_of(id: &str) -> &str {
    cli_registry::KNOWN_CLIS.iter().find(|spec| spec.id == id).map(|spec| spec.label).unwrap_or(id)
}

fn key(id: &str) -> AppResult<String> {
    if !cli_registry::is_known(id) { return Err(invalid(format!("未知 CLI：{id}"))); }
    Ok(format!("{KEY_PREFIX}{id}"))
}

/// 没有记录时的配置：受支持的 CLI 默认启用，白名单为空。
pub fn default_config(id: &str) -> CliConfig {
    CliConfig { enabled: cli_registry::is_supported(id), custom_path: None, models: vec![], default_model: None }
}

/// 校验并整理配置。失败时不产生任何副作用。
pub fn validate(id: &str, config: &CliConfig) -> AppResult<CliConfig> {
    key(id)?;
    if config.enabled && !cli_registry::is_supported(id) {
        return Err(invalid(format!("{} 尚未接入，不能启用", label_of(id))));
    }
    if config.models.len() > MAX_MODELS { return Err(invalid(format!("每个 CLI 最多 {MAX_MODELS} 个模型"))); }
    let mut seen = HashSet::new();
    let mut models = Vec::with_capacity(config.models.len());
    for model in &config.models {
        let model_id = model.id.trim();
        if model_id.is_empty() { return Err(invalid("模型 ID 不能为空")); }
        if model_id.len() > MAX_ID_LEN || model_id.chars().any(char::is_control) {
            return Err(invalid(format!("模型 ID 无效：{}", model_id.escape_debug())));
        }
        if id == "dsh" { crate::dsh_config::parse_selection(Some(model_id))?; }
        if !seen.insert(model_id.to_string()) { return Err(invalid(format!("模型重复：{model_id}"))); }
        let label = model.label.trim();
        models.push(CliModel {
            id: model_id.to_string(),
            label: if label.is_empty() { model_id.to_string() } else { label.to_string() },
            custom: model.custom,
        });
    }
    let default_model = config.default_model.as_deref().map(str::trim).filter(|value| !value.is_empty()).map(str::to_string);
    if let Some(default) = &default_model {
        if !seen.contains(default) { return Err(invalid("默认模型必须在已勾选的模型列表中")); }
    }
    let custom_path = match config.custom_path.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        None => None,
        Some(raw) => {
            let path = Path::new(raw);
            if !path.is_absolute() || !path.is_file() { return Err(invalid(format!("指定的路径不是存在的文件：{raw}"))); }
            Some(raw.to_string())
        }
    };
    Ok(CliConfig { enabled: config.enabled, custom_path, models, default_model })
}

pub fn load(conn: &Connection, id: &str) -> AppResult<CliConfig> {
    let value: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key=?1", [key(id)?], |row| row.get(0))
        .optional()?;
    match value {
        None => Ok(default_config(id)),
        Some(value) => serde_json::from_str(&value).map_err(|_| invalid("应用配置无法读取，请重新保存")),
    }
}

/// 校验后写入，返回规范化后的配置。
pub fn save(conn: &Connection, id: &str, config: &CliConfig) -> AppResult<CliConfig> {
    let normalized = validate(id, config)?;
    let value = serde_json::to_string(&normalized).map_err(|_| invalid("无法保存应用配置"))?;
    conn.execute(
        "INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        rusqlite::params![key(id)?, value],
    )?;
    Ok(normalized)
}

pub fn is_enabled(conn: &Connection, id: &str) -> AppResult<bool> { Ok(load(conn, id)?.enabled) }

/// 新建成员、新选 CLI 时调用：已关闭的 CLI 不能再被新选。
pub fn ensure_selectable(conn: &Connection, id: &str) -> AppResult<()> {
    if is_enabled(conn, id)? { return Ok(()); }
    Err(invalid(format!("{} 已在应用管理中关闭，请先在系统设置里启用", label_of(id))))
}

/// 所有已关闭的已知 CLI（含没有适配器、默认关闭的 Pi）。
pub fn disabled_ids(conn: &Connection) -> AppResult<HashSet<String>> {
    let mut disabled = HashSet::new();
    for spec in cli_registry::KNOWN_CLIS.iter() {
        if !is_enabled(conn, spec.id)? { disabled.insert(spec.id.to_string()); }
    }
    Ok(disabled)
}

/// 成员自己填了模型就用成员的，否则用该 CLI 的默认模型。
pub fn effective_model(conn: &Connection, cli_id: &str, member_model: Option<&str>) -> AppResult<Option<String>> {
    if let Some(model) = member_model.map(str::trim).filter(|model| !model.is_empty()) {
        return Ok(Some(model.to_string()));
    }
    Ok(load(conn, cli_id)?.default_model)
}

/// 启动时把已保存的手动路径写进 `cli_registry`。读不出来的跳过，不阻止应用启动。
pub fn load_path_overrides(conn: &Connection) {
    for spec in cli_registry::KNOWN_CLIS.iter() {
        if let Ok(config) = load(conn, spec.id) {
            cli_registry::set_path_override(spec.id, config.custom_path.map(PathBuf::from));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::open_in_memory;

    fn model(id: &str) -> CliModel { CliModel { id: id.into(), label: id.into(), custom: false } }
    fn config(models: &[&str], default: Option<&str>) -> CliConfig {
        CliConfig { enabled: true, custom_path: None, models: models.iter().map(|id| model(id)).collect(), default_model: default.map(str::to_string) }
    }

    #[test]
    fn defaults_follow_support_not_installation() {
        let conn = open_in_memory().unwrap();
        for id in ["claude", "codex", "grok", "dsh"] { assert!(load(&conn, id).unwrap().enabled, "{id}"); }
        assert!(!load(&conn, "pi").unwrap().enabled);
        assert!(load(&conn, "claude").unwrap().models.is_empty());
        assert!(load(&conn, "gemini").is_err(), "未知 CLI 不允许读取");
    }

    #[test]
    fn save_round_trips_and_normalizes() {
        let conn = open_in_memory().unwrap();
        let mut input = config(&["  opus ", "sonnet"], Some(" opus "));
        input.models[0].label = "  ".into();
        let saved = save(&conn, "claude", &input).unwrap();
        assert_eq!(saved.models[0], CliModel { id: "opus".into(), label: "opus".into(), custom: false });
        assert_eq!(saved.default_model.as_deref(), Some("opus"));
        assert_eq!(load(&conn, "claude").unwrap(), saved);
    }

    #[test]
    fn invalid_configs_are_rejected_and_write_nothing() {
        let conn = open_in_memory().unwrap();
        let mut relative = config(&[], None); relative.custom_path = Some("claude".into());
        let mut missing = config(&[], None);
        missing.custom_path = Some(std::env::temp_dir().join("imates-no-such-cli-binary").to_string_lossy().into_owned());
        let mut pi = config(&[], None); pi.enabled = true;
        let cases = [
            ("claude", config(&["opus"], Some("sonnet"))),
            ("claude", config(&["opus", " opus"], None)),
            ("claude", config(&["  "], None)),
            ("claude", config(&["a
b"], None)),
            ("claude", relative),
            ("claude", missing),
            ("pi", pi),
            ("dsh", config(&["不是 JSON"], None)),
            ("gemini", config(&[], None)),
        ];
        for (id, case) in cases {
            assert!(save(&conn, id, &case).is_err(), "{id} {case:?}");
        }
        let rows: i64 = conn.query_row("SELECT COUNT(*) FROM settings WHERE key LIKE 'cli_config:%'", [], |row| row.get(0)).unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn dsh_models_must_be_valid_selections_and_existing_files_are_accepted_as_paths() {
        let conn = open_in_memory().unwrap();
        let selection = r#"{"profile":"web","provider":"gpt","model":"gpt-6"}"#;
        save(&conn, "dsh", &config(&[selection], Some(selection))).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("claude.exe");
        std::fs::write(&program, b"").unwrap();
        let mut with_path = config(&[], None); with_path.custom_path = Some(program.to_string_lossy().into_owned());
        assert_eq!(save(&conn, "claude", &with_path).unwrap().custom_path, with_path.custom_path);
    }

    #[test]
    fn corrupt_record_is_reported_not_silently_reset() {
        let conn = open_in_memory().unwrap();
        conn.execute("INSERT INTO settings(key,value) VALUES('cli_config:claude','{坏')", []).unwrap();
        assert!(load(&conn, "claude").unwrap_err().to_string().contains("重新保存"));
    }

    #[test]
    fn closed_cli_cannot_be_newly_selected() {
        let conn = open_in_memory().unwrap();
        assert!(ensure_selectable(&conn, "codex").is_ok());
        let mut closed = config(&[], None); closed.enabled = false;
        save(&conn, "codex", &closed).unwrap();
        assert!(!is_enabled(&conn, "codex").unwrap());
        assert!(ensure_selectable(&conn, "codex").unwrap_err().to_string().contains("已在应用管理中关闭"));
        assert_eq!(disabled_ids(&conn).unwrap(), HashSet::from(["codex".to_string(), "pi".to_string()]));
    }

    #[test]
    fn effective_model_prefers_member_then_cli_default() {
        let conn = open_in_memory().unwrap();
        save(&conn, "claude", &config(&["opus", "sonnet"], Some("opus"))).unwrap();
        assert_eq!(effective_model(&conn, "claude", None).unwrap().as_deref(), Some("opus"));
        assert_eq!(effective_model(&conn, "claude", Some("  ")).unwrap().as_deref(), Some("opus"));
        assert_eq!(effective_model(&conn, "claude", Some("sonnet")).unwrap().as_deref(), Some("sonnet"));
        assert_eq!(effective_model(&conn, "codex", None).unwrap(), None);
    }
}
