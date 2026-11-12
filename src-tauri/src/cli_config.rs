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

// ---- 模型候选 ----

/// Claude Code 的 `--model` 接受的别名。Codex 没有列模型的命令，且本机常用自定义供应商，所以留空让用户手填。
pub fn builtin_models(id: &str) -> Vec<CliModel> {
    match id {
        "claude" => ["opus", "sonnet", "haiku"].iter()
            .map(|alias| CliModel { id: (*alias).into(), label: (*alias).into(), custom: false }).collect(),
        _ => vec![],
    }
}

/// 解析 `grok models` 的输出：取 `Available models:` 之后以 `*` 或 `-` 开头的行，第一个词是模型 ID。
pub fn parse_grok_models(output: &str) -> Vec<CliModel> {
    let mut in_list = false;
    let mut models: Vec<CliModel> = vec![];
    for line in output.lines().map(str::trim) {
        if line.eq_ignore_ascii_case("Available models:") { in_list = true; continue; }
        if !in_list { continue; }
        let Some(rest) = line.strip_prefix('*').or_else(|| line.strip_prefix('-')) else { continue; };
        let Some(id) = rest.split_whitespace().next() else { continue; };
        if models.iter().any(|model| model.id == id) { continue; }
        let label = if rest.contains("(default)") { format!("{id}（CLI 默认）") } else { id.to_string() };
        models.push(CliModel { id: id.to_string(), label, custom: false });
    }
    models
}

/// 在临时目录运行 `grok models`，带上应用的代理环境。
fn fetch_grok_models(environment: &[(String, Option<String>)]) -> Result<Vec<CliModel>, String> {
    let cli = cli_registry::resolve_cli("grok").map_err(|error| error.to_string())?;
    let (program, args) = crate::runner::platform::resolve_program(&cli.program, &["models".to_string()], Some(cli.path_env.as_os_str()))?;
    let mut command = std::process::Command::new(program);
    command.args(args).env("PATH", &cli.path_env).current_dir(std::env::temp_dir());
    for (name, value) in environment {
        match value { Some(value) => { command.env(name, value); } None => { command.env_remove(name); } }
    }
    let output = cli_registry::probe_output(command, std::time::Duration::from_secs(20))
        .ok_or_else(|| "读取 Grok 模型失败或超时，请检查登录状态与网络代理".to_string())?;
    let models = parse_grok_models(&output);
    if models.is_empty() { return Err("Grok 没有返回可用模型，请先登录后重试".into()); }
    Ok(models)
}

// ---- Tauri 命令 ----

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliConfigView {
    pub id: String,
    pub label: String,
    pub installed: bool,
    pub supported: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub enabled: bool,
    pub custom_path: Option<String>,
    pub models: Vec<CliModel>,
    pub default_model: Option<String>,
    /// `auto` 自动读取，`builtin` 内置候选加手填，`none` 暂不支持。
    pub models_source: String,
}

fn view(info: cli_registry::CliInfo, config: CliConfig) -> CliConfigView {
    let models_source = match info.id.as_str() {
        "grok" | "dsh" => "auto",
        "claude" | "codex" => "builtin",
        _ => "none",
    };
    CliConfigView {
        id: info.id, label: info.label, installed: info.installed, supported: info.supported, path: info.path, version: info.version,
        enabled: config.enabled, custom_path: config.custom_path, models: config.models, default_model: config.default_model,
        models_source: models_source.into(),
    }
}

fn detect_one(id: &str) -> cli_registry::CliInfo {
    cli_registry::resolve_cli(id).map(|cli| cli.info).unwrap_or_else(|_| {
        let spec = cli_registry::KNOWN_CLIS.iter().find(|spec| spec.id == id);
        cli_registry::CliInfo {
            id: id.to_string(), label: spec.map(|spec| spec.label).unwrap_or(id).to_string(),
            installed: false, supported: cli_registry::is_supported(id), path: None, version: None,
        }
    })
}

/// 探测要逐个启动 CLI 取版本号，放到后台线程。
#[tauri::command(async)]
pub fn list_cli_configs(state: tauri::State<'_, crate::commands::AppState>) -> AppResult<Vec<CliConfigView>> {
    let infos = cli_registry::detect_all();
    let conn = state.db_lock()?;
    infos.into_iter().map(|info| { let config = load(&conn, &info.id)?; Ok(view(info, config)) }).collect()
}

/// 保存配置。默认模型变了，成员「模型留空」时实际模型随之改变，其会话要清空（手册：改模型清会话）。
/// 配置与清会话在同一个事务里；已在进行的回合不受影响，下一回合生效。
pub fn save_and_reset_sessions(conn: &Connection, id: &str, config: &CliConfig) -> AppResult<CliConfig> {
    let previous = load(conn, id).ok().and_then(|old| old.default_model);
    let tx = conn.unchecked_transaction()?;
    let saved = save(&tx, id, config)?;
    if saved.default_model != previous {
        tx.execute("UPDATE agents SET session_id=NULL WHERE cli=?1 AND COALESCE(TRIM(model),'')=''", [id])?;
    }
    tx.commit()?;
    Ok(saved)
}

#[tauri::command(async)]
pub fn save_cli_config(state: tauri::State<'_, crate::commands::AppState>, id: String, config: CliConfig) -> AppResult<CliConfigView> {
    let saved = { let conn = state.db_lock()?; save_and_reset_sessions(&conn, &id, &config)? };
    cli_registry::set_path_override(&id, saved.custom_path.clone().map(PathBuf::from));
    Ok(view(detect_one(&id), saved))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliModelCatalog { pub models: Vec<CliModel>, pub error: Option<String> }

/// 只服务 claude、codex、grok；Dsh 的候选由前端读 Dsh 目录得到。
#[tauri::command]
pub async fn list_cli_models(state: tauri::State<'_, crate::commands::AppState>, id: String) -> AppResult<CliModelCatalog> {
    match id.as_str() {
        "claude" | "codex" => Ok(CliModelCatalog { models: builtin_models(&id), error: None }),
        "grok" => {
            let proxy = { let conn = state.db_lock()?; crate::proxy::load(&conn)? };
            let environment = crate::proxy::resolve(&proxy)?.map(|proxy| crate::proxy::overrides(&proxy)).unwrap_or_default();
            let result = tauri::async_runtime::spawn_blocking(move || fetch_grok_models(&environment)).await
                .map_err(|error| invalid(format!("读取模型失败：{error}")))?;
            Ok(match result {
                Ok(models) => CliModelCatalog { models, error: None },
                Err(error) => CliModelCatalog { models: vec![], error: Some(error) },
            })
        }
        _ => Err(invalid("该 CLI 的模型候选不由本命令提供")),
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

    #[test]
    fn parses_the_recorded_grok_models_sample() {
        let models = parse_grok_models(include_str!("../tests/fixtures/grok/models.txt"));
        let ids: Vec<&str> = models.iter().map(|model| model.id.as_str()).collect();
        assert_eq!(ids, ["grok-4.7", "grok-4.7-build-fast", "grok-4.6", "grok-4.5"]);
        assert!(models[0].label.contains("默认"));
        assert!(models.iter().all(|model| !model.custom));
    }

    #[test]
    fn grok_parser_ignores_noise_and_duplicates() {
        assert!(parse_grok_models("").is_empty());
        assert!(parse_grok_models("You are logged in.
  - not-a-model-yet
").is_empty(), "必须在 Available models 之后");
        let models = parse_grok_models("Available models:
  - a
  - a

  * b (default)
垃圾行
");
        assert_eq!(models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    }

    #[test]
    fn builtin_models_cover_claude_aliases_and_leave_codex_to_manual_entry() {
        let claude: Vec<String> = builtin_models("claude").into_iter().map(|m| m.id).collect();
        assert_eq!(claude, ["opus", "sonnet", "haiku"]);
        assert!(builtin_models("codex").is_empty());
        assert!(builtin_models("pi").is_empty());
    }

    #[test]
    fn view_marks_models_source_per_cli() {
        let info = |id: &str| cli_registry::CliInfo { id: id.into(), label: id.into(), installed: true, supported: id != "pi", path: None, version: None };
        assert_eq!(view(info("grok"), default_config("grok")).models_source, "auto");
        assert_eq!(view(info("dsh"), default_config("dsh")).models_source, "auto");
        assert_eq!(view(info("claude"), default_config("claude")).models_source, "builtin");
        assert_eq!(view(info("codex"), default_config("codex")).models_source, "builtin");
        assert_eq!(view(info("pi"), default_config("pi")).models_source, "none");
    }

    fn seed_agent(conn: &Connection, project_id: &str, name: &str, cli: &str, model: Option<&str>, leader: bool) -> String {
        let agent = crate::store::agents::create(conn, project_id, &crate::store::agents::NewAgent {
            name: name.into(), role: "开发".into(), persona: "## 定位
开发
## 职责
测试
## 边界
不越界
## 工作风格
务实".into(), cli: cli.into(),
            model: model.map(str::to_string), is_leader: leader,
        }).unwrap();
        crate::store::agents::set_session(conn, &agent.id, Some("会话")).unwrap();
        agent.id
    }

    fn session_of(conn: &Connection, agent_id: &str) -> Option<String> {
        conn.query_row("SELECT session_id FROM agents WHERE id=?1", [agent_id], |row| row.get(0)).unwrap()
    }

    #[test]
    fn changing_default_model_clears_only_sessions_that_follow_the_default() {
        let conn = open_in_memory().unwrap();
        let project = crate::store::projects::create(&conn, "项目", "描述", "/tmp/项目").unwrap();
        let follows = seed_agent(&conn, &project.id, "跟随", "claude", None, true);
        let blank = seed_agent(&conn, &project.id, "空白", "claude", Some("  "), false);
        let own = seed_agent(&conn, &project.id, "自填", "claude", Some("haiku"), false);
        let other_cli = seed_agent(&conn, &project.id, "别家", "codex", None, false);

        // 默认模型从无变为 opus：沿用默认的成员清会话，自填模型与别家 CLI 的成员不动
        save_and_reset_sessions(&conn, "claude", &config(&["opus", "sonnet"], Some("opus"))).unwrap();
        assert_eq!(session_of(&conn, &follows), None);
        assert_eq!(session_of(&conn, &blank), None);
        assert_eq!(session_of(&conn, &own).as_deref(), Some("会话"));
        assert_eq!(session_of(&conn, &other_cli).as_deref(), Some("会话"));
        assert_eq!(load(&conn, "claude").unwrap().default_model.as_deref(), Some("opus"));

        // 默认模型没变（只改 enabled）：不清会话
        crate::store::agents::set_session(&conn, &follows, Some("新会话")).unwrap();
        let mut closed = config(&["opus", "sonnet"], Some("opus")); closed.enabled = false;
        save_and_reset_sessions(&conn, "claude", &closed).unwrap();
        assert_eq!(session_of(&conn, &follows).as_deref(), Some("新会话"));

        // 默认模型改成 sonnet：再次清空
        save_and_reset_sessions(&conn, "claude", &config(&["opus", "sonnet"], Some("sonnet"))).unwrap();
        assert_eq!(session_of(&conn, &follows), None);

        // 校验失败时既不写配置也不清会话
        crate::store::agents::set_session(&conn, &follows, Some("又一个")).unwrap();
        assert!(save_and_reset_sessions(&conn, "claude", &config(&["opus"], Some("不存在"))).is_err());
        assert_eq!(session_of(&conn, &follows).as_deref(), Some("又一个"));
        assert_eq!(load(&conn, "claude").unwrap().default_model.as_deref(), Some("sonnet"));
    }
}
