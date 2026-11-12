use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::time::Duration;
use std::sync::{Mutex, MutexGuard};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::watch;
use tauri::{Emitter, Manager};
use crate::adapters::{AgentEvent, CliAdapter, RunMode, Usage};
use crate::chat::{ChatService, PreparedTurn};
use crate::store::{messages, runs};
use crate::store::messages::Message;
use tauri::State;

use crate::cli_registry::{self, CliInfo};
use crate::error::{AppError, AppResult};
use crate::project_setup::{self, NewProject};
use crate::rules::{self, RuleDoc};
use crate::store::agents::{self, Agent, NewAgent};
use crate::store::projects::{self, Project, ProjectSummary};
use crate::templates::{self, PersonaTemplate};

/// 应用全局状态：一条数据库连接，用互斥锁串行访问。
pub struct AppState {
    pub db: Mutex<Connection>,
    pub runtime: Mutex<ActiveRuns>,
    pub draft_sessions: Mutex<crate::assistant::DraftSessions>,
    pub data_dir: PathBuf,
    pub watch: crate::fs_watch::WatchHub,
    rules_guard: Mutex<()>,
    instance: String,
}

fn lock<'a>(state: &'a State<'_, AppState>) -> AppResult<MutexGuard<'a, Connection>> {
    state
        .db
        .lock()
        .map_err(|_| AppError::Invalid("数据库连接不可用，请重启应用".into()))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDetail {
    pub project: Project,
    pub agents: Vec<Agent>,
    pub folder_exists: bool,
}

/// 探测要逐个启动 CLI 取版本号，耗时较长，放到后台线程执行以免卡住界面。
#[tauri::command(async)]
pub fn list_clis() -> Vec<CliInfo> {
    cli_registry::detect_all()
}

#[tauri::command]
pub fn get_network_proxy(state: State<'_, AppState>) -> AppResult<crate::proxy::ProxyState> {
    let conn = lock(&state)?;
    Ok(crate::proxy::state(crate::proxy::load(&conn)?))
}

#[tauri::command]
pub fn save_network_proxy(state: State<'_, AppState>, settings: crate::proxy::ProxySettings) -> AppResult<crate::proxy::ProxyState> {
    let conn = lock(&state)?;
    crate::proxy::save(&conn, &settings)?;
    Ok(crate::proxy::state(crate::proxy::load(&conn)?))
}

#[tauri::command]
pub fn get_dsh_settings(state: State<'_, AppState>) -> AppResult<crate::dsh_config::DshSettings> {
    let conn = lock(&state)?; crate::dsh_config::load(&conn)
}

#[tauri::command]
pub fn save_dsh_settings(state: State<'_, AppState>, settings: crate::dsh_config::DshSettings) -> AppResult<crate::dsh_config::DshSettings> {
    let conn = lock(&state)?; crate::dsh_config::save(&conn, &settings)?; Ok(settings)
}

#[tauri::command]
pub async fn list_dsh_models(state: State<'_, AppState>, profile: crate::dsh_config::Profile) -> AppResult<crate::dsh_config::DshCatalog> {
    let cli = cli_registry::resolve_cli("dsh")?;
    let mut settings = { let conn = state.db_lock()?; crate::dsh_config::load(&conn)? };
    settings.profile = profile;
    let proxy = { let conn = state.db_lock()?; crate::proxy::load(&conn)? };
    let environment = crate::proxy::resolve(&proxy)?.map(|proxy| crate::proxy::overrides(&proxy)).unwrap_or_default();
    let capture: crate::dsh_config::CatalogCapture = std::sync::Arc::new(Mutex::new(None));
    let adapter = crate::adapters::dsh::DshAdapter::new(cli.program.clone(), RunMode::Discuss).with_environment(&cli)
        .with_settings(settings).for_catalog(capture.clone());
    let adapter = crate::proxy::ProxyAdapter::new(Box::new(adapter), environment);
    let id = format!("dsh-catalog:{}", uuid::Uuid::new_v4());
    let cancellation = state.runtime_lock()?.reserve(&id, &id)?;
    let request = crate::adapters::TurnRequest { prompt: "读取模型目录".into(), session_id: None, cwd: std::env::temp_dir(), mode: RunMode::Discuss, model: None };
    let config = crate::runner::RunnerConfig { model_idle_timeout: Duration::from_secs(40), discuss_timeout: Duration::from_secs(45),
        stdout_close_grace: Duration::from_secs(3), ..Default::default() };
    let outcome = crate::runner::run_turn_managed(Box::new(adapter), request, cancellation, |_| {}, config, None, |_| Ok(())).await;
    state.runtime_lock()?.remove(&id, &id);
    if outcome.status != "ok" { return Err(AppError::Invalid("读取 Dsh 模型失败，请在 Dsh 中检查 profile、登录和网络后刷新".into())); }
    let catalog = capture.lock().map_err(|_| AppError::Invalid("Dsh 模型目录暂不可用".into()))?.take()
        .ok_or_else(|| AppError::Invalid("Dsh 没有返回可识别的模型目录".into()))?;
    Ok(catalog)
}

#[tauri::command]
pub fn list_projects(state: State<'_, AppState>) -> AppResult<Vec<ProjectSummary>> {
    let conn = lock(&state)?;
    projects::list(&conn)
}

#[tauri::command]
pub fn create_project(state: State<'_, AppState>, input: NewProject) -> AppResult<Project> {
    let conn = lock(&state)?;
    project_setup::create_full(&conn, &input)
}

#[tauri::command]
pub fn get_project(state: State<'_, AppState>, id: String) -> AppResult<ProjectDetail> {
    let conn = lock(&state)?;
    let project = projects::get(&conn, &id)?;
    let agents = agents::list(&conn, &id)?;
    let folder_exists = Path::new(&project.folder_path).is_dir();
    Ok(ProjectDetail {
        project,
        agents,
        folder_exists,
    })
}

#[tauri::command]
pub fn update_project(
    state: State<'_, AppState>,
    id: String,
    name: String,
    description: String,
    folder_path: String,
) -> AppResult<Project> {
    let runtime = state.runtime_lock()?;
    let conn = lock(&state)?;
    let project = projects::get(&conn, &id)?;
    if project.folder_path != folder_path {
        runtime.ensure_project_idle(&id)?;
        state.watch.remove(&id);
        conn.execute("DELETE FROM settings WHERE key=?1", [format!("{RULES_DIGEST_PREFIX}{id}")])?;
    }
    projects::update(&conn, &id, &name, &description, &folder_path)
}

#[tauri::command]
pub fn delete_project(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let runtime = state.runtime_lock()?;
    runtime.ensure_project_idle(&id)?;
    state.watch.remove(&id);
    let conn = lock(&state)?;
    conn.execute("DELETE FROM settings WHERE key=?1", [format!("{RULES_DIGEST_PREFIX}{id}")])?;
    projects::delete(&conn, &id)
}

#[tauri::command]
pub fn create_agent(
    state: State<'_, AppState>,
    project_id: String,
    input: NewAgent,
) -> AppResult<Agent> {
    let conn = lock(&state)?;
    crate::cli_config::ensure_selectable(&conn, &input.cli)?;
    let tx = conn.unchecked_transaction()?;
    let agent = agents::create(&tx, &project_id, &input)?;
    tx.commit()?;
    Ok(agent)
}

#[tauri::command]
pub fn update_agent(state: State<'_, AppState>, id: String, input: NewAgent) -> AppResult<Agent> {
    let runtime = state.runtime_lock()?;
    let conn = lock(&state)?;
    let old = agents::get(&conn, &id)?;
    crate::cli_config::ensure_cli_change_allowed(&conn, &old.cli, &input.cli)?;
    if old.cli != input.cli || old.model.as_deref() != input.model.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
        runtime.ensure_project_idle(&old.project_id)?;
    }
    let tx = conn.unchecked_transaction()?;
    let agent = agents::update(&tx, &id, &input)?;
    tx.commit()?;
    Ok(agent)
}

#[tauri::command]
pub fn delete_agent(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let runtime = state.runtime_lock()?;
    let conn = lock(&state)?;
    runtime.ensure_project_idle(&agents::get(&conn, &id)?.project_id)?;
    let tx = conn.unchecked_transaction()?;
    agents::delete(&tx, &id)?;
    tx.commit()?;
    Ok(())
}

/// 取项目文件夹路径。规章相关的 command 都从这里起步。
fn project_folder(state: &State<'_, AppState>, project_id: &str) -> AppResult<String> {
    let conn = lock(state)?;
    Ok(projects::get(&conn, project_id)?.folder_path)
}

#[tauri::command]
pub fn list_rules(state: State<'_, AppState>, project_id: String) -> AppResult<Vec<RuleDoc>> {
    let folder = project_folder(&state, &project_id)?;
    rules::list(Path::new(&folder))
}

fn emit_rule_notice(app: &tauri::AppHandle, state: &AppState, project_id: &str, message: Message) {
    let version = state.rules_version(project_id).unwrap_or_default();
    emit_events(app, vec![
        ("message:new", json!({"projectId": project_id, "runId": Value::Null, "message": message})),
        ("rules:changed", json!({"projectId": project_id, "version": version})),
    ]);
}

#[tauri::command]
pub fn save_rule(app: tauri::AppHandle, state: State<'_, AppState>, project_id: String, filename: String, content: String) -> AppResult<()> {
    let message = state.save_rule_for_project(&project_id, &filename, &content)?;
    emit_rule_notice(&app, state.inner(), &project_id, message);
    Ok(())
}

#[tauri::command]
pub fn delete_rule(app: tauri::AppHandle, state: State<'_, AppState>, project_id: String, filename: String) -> AppResult<()> {
    let message = state.delete_rule_for_project(&project_id, &filename)?;
    emit_rule_notice(&app, state.inner(), &project_id, message);
    Ok(())
}

#[tauri::command]
pub fn list_rule_templates() -> Vec<RuleDoc> {
    templates::rule_templates()
}

#[tauri::command]
pub fn list_persona_templates() -> Vec<PersonaTemplate> {
    templates::persona_templates()
}

#[tauri::command]
pub fn list_files(state: State<'_, AppState>, project_id: String) -> AppResult<crate::files::FileListing> {
    let folder = project_folder(&state, &project_id)?;
    crate::files::list_files(Path::new(&folder))
}

#[tauri::command]
pub fn open_file(app: tauri::AppHandle, state: State<'_, AppState>, project_id: String, path: String) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let folder = project_folder(&state, &project_id)?;
    let target = crate::files::resolve_inside(Path::new(&folder), &path)?;
    app.opener()
        .open_path(crate::files::display_path(&target), None::<&str>)
        .map_err(|error| AppError::Invalid(format!("无法打开文件：{error}")))
}

#[tauri::command]
pub fn watch_project(app: tauri::AppHandle, state: State<'_, AppState>, project_id: String) -> AppResult<Vec<String>> {
    ensure_project_watch(&app, state.inner(), &project_id)?;
    Ok(state.watch.changed(&project_id))
}

/// 外部规章变化：写通知并告诉前端刷新。没有变化时什么都不发。
fn emit_rule_sync(app: &tauri::AppHandle, state: &AppState, project_id: &str) {
    if let Ok(Some((message, version))) = state.sync_rules(project_id) {
        emit_events(app, vec![
            ("message:new", json!({"projectId": project_id, "runId": Value::Null, "message": message})),
            ("rules:changed", json!({"projectId": project_id, "version": version})),
        ]);
    }
}

/// 确保项目文件夹正在被监听。重复调用没有副作用。
pub fn ensure_project_watch(app: &tauri::AppHandle, state: &AppState, project_id: &str) -> AppResult<()> {
    let folder = {
        let conn = state.db_lock()?;
        projects::get(&conn, project_id)?.folder_path
    };
    let handle = app.clone();
    let id = project_id.to_string();
    let created = state.watch.ensure(project_id, Path::new(&folder), move |batch| {
        let state = handle.state::<AppState>();
        if !batch.paths.is_empty() {
            emit_events(&handle, vec![("files:changed", json!({
                "projectId": id, "paths": batch.paths, "changed": state.watch.changed(&id)}))]);
        }
        if batch.rules_touched {
            emit_rule_sync(&handle, state.inner(), &id);
        }
    })?;
    if created {
        // 应用没开着的时候规章也可能被改过，开始监听时先对一次账。
        emit_rule_sync(app, state, project_id);
    }
    Ok(())
}

/// 一条新指令开始：之前的高亮作废。监听启动失败不挡住发送。
fn begin_change_window(app: &tauri::AppHandle, project_id: &str) {
    let state = app.state::<AppState>();
    let _ = ensure_project_watch(app, state.inner(), project_id);
    state.watch.reset_changed(project_id);
    emit_events(app, vec![("files:changed", json!({"projectId": project_id, "paths": [], "changed": []}))]);
}

#[cfg(test)]
mod runtime_tests {
    use super::{finished_status, live_status, ActiveRuns, RuntimeSnapshot};

    #[test]
    fn reservation_is_per_project_and_old_finish_cannot_remove_new_run() {
        let mut active = ActiveRuns::default();
        let first = active.reserve("项目甲", "运行一").unwrap();
        assert!(active.reserve("项目甲", "运行二").is_err());
        assert!(active.reserve("项目乙", "运行三").is_ok());
        active.cancel("项目甲");
        assert!(*first.borrow());
        active.remove("项目甲", "运行一");
        let second = active.reserve("项目甲", "运行二").unwrap();
        active.remove("项目甲", "运行一");
        assert!(active.reserve("项目甲", "运行四").is_err());
        assert!(!*second.borrow());
    }

    #[test]
    fn execution_directory_is_shared_between_projects() {
        let mut active = ActiveRuns::default();
        let dir = std::path::Path::new("/临时项目");
        active.reserve("项目甲", "运行一").unwrap();
        active.reserve_directory(dir, "运行一").unwrap();
        active.reserve("项目乙", "运行二").unwrap();
        assert!(active.reserve_directory(dir, "运行二").is_err());
        active.remove("项目乙", "运行二");
        assert!(active.reserve_directory(dir, "运行三").is_err());
        active.remove("项目甲", "运行一");
        active.reserve_directory(dir, "运行三").unwrap();
    }

    fn snapshot(project: &str, run: &str, agent: &str) -> RuntimeSnapshot {
        RuntimeSnapshot { project_id: project.into(), run_id: run.into(), agent_id: agent.into(),
            status: "thinking".into(), seq: 0, partial_text: String::new() }
    }

    #[test]
    fn team_hold_blocks_edits_between_turns_and_one_cancel_reaches_every_member() {
        let mut active = ActiveRuns::default();
        let hold = "hold:项目甲";
        let first = active.reserve("项目甲", hold).unwrap();
        let second = first.clone();
        active.reserve_directory(std::path::Path::new("/临时项目"), hold).unwrap();
        // 回合之间没有任何成员在跑，项目仍然算运行中。
        assert!(active.ensure_project_idle("项目甲").is_err());
        assert!(active.reserve("项目甲", "另一次发送").is_err());
        assert!(active.active_snapshot("项目甲").is_none());

        active.team_started("项目甲", snapshot("项目甲", "run-1", "成员一"));
        active.team_started("项目甲", snapshot("项目甲", "run-2", "成员二"));
        active.team_status("项目甲", "run-1", "talking");
        assert_eq!(active.active_snapshot("项目甲").unwrap().run_id, "run-2", "返回最近启动的一个");
        active.team_finished("项目甲", "run-2");
        let left = active.active_snapshot("项目甲").unwrap();
        assert_eq!((left.run_id.as_str(), left.status.as_str()), ("run-1", "talking"));

        active.cancel("项目甲");
        assert!(*first.borrow() && *second.borrow(), "同一批成员共用一个取消信号");
        active.team_finished("项目甲", "run-1");
        active.remove("项目甲", hold);
        assert!(active.ensure_project_idle("项目甲").is_ok());
        assert!(active.active_snapshot("项目甲").is_none());
        active.reserve_directory(std::path::Path::new("/临时项目"), "下一次").unwrap();
        // 预留号可以重复使用，取消信号是新的。
        assert!(!*active.reserve("项目甲", hold).unwrap().borrow());
    }

    #[test]
    fn unclean_member_cleanup_keeps_the_directory_blocked_after_the_hold_ends() {
        let mut active = ActiveRuns::default();
        let dir = std::path::PathBuf::from("/临时项目");
        active.reserve("项目甲", "hold:项目甲").unwrap();
        active.reserve_directory(&dir, "hold:项目甲").unwrap();
        active.block_directory(dir.clone(), "run-残留");
        active.remove("项目甲", "hold:项目甲");
        assert!(active.reserve_directory(&dir, "下一次").is_err(), "残留未确认前目录仍然被占");
    }

    #[test]
    fn live_status_follows_mode_and_keeps_errors() {
        use crate::adapters::{AgentEvent, RunMode};
        let text = AgentEvent::TextDelta { text: "发言".into() };
        let tool = AgentEvent::ToolState { active: true };
        let session = AgentEvent::SessionStarted { session_id: "s".into() };
        let error = AgentEvent::Error { message: "失败".into() };
        // 讨论：有文本才算发言，工具和会话事件不改变状态。
        assert_eq!(live_status(RunMode::Discuss, &session, "thinking"), "thinking");
        assert_eq!(live_status(RunMode::Discuss, &tool, "thinking"), "thinking");
        assert_eq!(live_status(RunMode::Discuss, &text, "thinking"), "talking");
        assert_eq!(live_status(RunMode::Discuss, &tool, "talking"), "talking");
        // 执行：一有输出就是干活。
        assert_eq!(live_status(RunMode::Execute, &session, "thinking"), "working");
        assert_eq!(live_status(RunMode::Execute, &text, "working"), "working");
        // 出错后不被后续事件改回。
        assert_eq!(live_status(RunMode::Execute, &error, "working"), "error");
        assert_eq!(live_status(RunMode::Discuss, &text, "error"), "error");
    }

    #[test]
    fn finished_status_never_reports_ok_when_the_outcome_was_not_applied() {
        assert_eq!(finished_status("ok", true), "ok");
        assert_eq!(finished_status("ok", false), "failed");
        assert_eq!(finished_status("cancelled", false), "cancelled");
        assert_eq!(finished_status("timeout", false), "timeout");
        assert_eq!(finished_status("failed", true), "failed");
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSnapshot {
    pub project_id: String,
    pub run_id: String,
    pub agent_id: String,
    pub status: String,
    pub seq: u64,
    pub partial_text: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupNotice {
    pub run_id: String,
    pub project_id: Option<String>,
    pub folder_path: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRuntime { pub active: Option<RuntimeSnapshot>, pub cleanup_required: Vec<CleanupNotice> }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeOwner {
    pub run_id: String,
    pub project_id: Option<String>,
    pub draft_id: Option<String>,
    pub folder_path: String,
    pub pid: Option<u32>,
    pub process_group: Option<u32>,
    pub nonce: String,
    pub instance: String,
    pub started_at: i64,
    pub cleanup_required: bool,
}
const OWNER_PREFIX: &str = "runtime:";
pub fn save_owner(conn: &Connection, owner: &RuntimeOwner) -> AppResult<()> {
    let value = serde_json::to_string(owner).map_err(|e| AppError::Invalid(format!("运行所有权序列化失败：{e}")))?;
    conn.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        rusqlite::params![format!("{OWNER_PREFIX}{}", owner.run_id), value])?;
    Ok(())
}
pub fn load_owners(conn: &Connection) -> AppResult<Vec<RuntimeOwner>> {
    let mut statement = conn.prepare("SELECT value FROM settings WHERE key LIKE 'runtime:%'")?;
    let mut owners = Vec::new();
    for row in statement.query_map([], |row| row.get::<_, String>(0))? {
        owners.push(serde_json::from_str(&row?).map_err(|e| AppError::Invalid(format!("运行清理记录损坏，请检查应用数据库：{e}")))?);
    }
    Ok(owners)
}
pub fn remove_owner(conn: &Connection, run_id: &str) -> AppResult<()> {
    conn.execute("DELETE FROM settings WHERE key=?1", [format!("{OWNER_PREFIX}{run_id}")])?;
    Ok(())
}
const RULES_DIGEST_PREFIX: &str = "rules_digest:";
fn load_rule_digest(conn: &Connection, project_id: &str) -> AppResult<Option<std::collections::BTreeMap<String, String>>> {
    use rusqlite::OptionalExtension;
    let value: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key=?1", [format!("{RULES_DIGEST_PREFIX}{project_id}")], |row| row.get(0))
        .optional()?;
    // 记录损坏时当作没有基线，下一次只重新记录，不误报。
    Ok(value.and_then(|text| serde_json::from_str(&text).ok()))
}
fn save_rule_digest(conn: &Connection, project_id: &str, digest: &std::collections::BTreeMap<String, String>) -> AppResult<()> {
    let value = serde_json::to_string(digest).map_err(|e| AppError::Invalid(format!("规章摘要序列化失败：{e}")))?;
    conn.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        rusqlite::params![format!("{RULES_DIGEST_PREFIX}{project_id}"), value])?;
    Ok(())
}

#[derive(Default)]
pub struct ActiveRuns {
    runs: HashMap<String, (String, watch::Sender<bool>)>,
    directories: HashMap<PathBuf, String>,
    sessions: HashMap<String, String>,
    snapshots: HashMap<String, RuntimeSnapshot>,
    /// 团队指令里正在运行的回合。并行讨论时不止一个。
    team: HashMap<String, Vec<RuntimeSnapshot>>,
    generations: HashMap<String, u64>,
    pub shutting_down: bool,
}
impl ActiveRuns {
    pub fn reserve(&mut self, key: &str, run_id: &str) -> AppResult<watch::Receiver<bool>> {
        if self.shutting_down { return Err(AppError::Invalid("应用正在退出，不能启动新回合".into())); }
        if self.runs.contains_key(key) { return Err(AppError::Invalid("项目或草稿正在运行，请先停止或等待完成".into())); }
        let (sender, receiver) = watch::channel(false);
        self.runs.insert(key.into(), (run_id.into(), sender));
        self.generations.insert(run_id.into(), 0);
        Ok(receiver)
    }
    pub fn cancel(&mut self, key: &str) {
        if let Some((run_id, sender)) = self.runs.get(key) {
            if !*sender.borrow() { *self.generations.entry(run_id.clone()).or_default() += 1; }
            ChatService::stop_project(sender);
        }
    }
    pub fn reserve_directory(&mut self, canonical: &Path, run_id: &str) -> AppResult<()> {
        if self.directories.contains_key(canonical) { return Err(AppError::Invalid("该工作目录正在运行其他回合，或有尚未确认的进程清理".into())); }
        self.directories.insert(canonical.into(), run_id.into()); Ok(())
    }
    pub fn reserve_session(&mut self, cli: &str, session: Option<&str>, run_id: &str) -> AppResult<()> {
        if let Some(session) = session {
            let key = format!("{cli}:{session}");
            if self.sessions.contains_key(&key) { return Err(AppError::Invalid("该 CLI 会话正在被其他回合续接".into())); }
            self.sessions.insert(key, run_id.into());
        }
        Ok(())
    }
    pub fn current(&self, key: &str, run_id: &str) -> bool {
        self.runs.get(key).is_some_and(|(id, _)| id == run_id)
    }
    pub fn accepts_events(&self, key: &str, run_id: &str) -> bool {
        self.current(key, run_id) && self.generations.get(run_id) == Some(&0)
    }
    pub fn remove(&mut self, key: &str, run_id: &str) {
        if self.current(key, run_id) {
            self.runs.remove(key); self.directories.retain(|_, id| id != run_id);
            self.sessions.retain(|_, id| id != run_id); self.snapshots.remove(key); self.generations.remove(run_id);
            self.team.remove(key);
        }
    }
    pub fn ensure_project_idle(&self, project_id: &str) -> AppResult<()> {
        if self.runs.contains_key(project_id) { Err(AppError::Invalid("项目正在运行，请先停止或等待完成".into())) } else { Ok(()) }
    }
    pub fn cancel_all(&mut self) {
        self.shutting_down = true;
        let keys: Vec<_> = self.runs.keys().cloned().collect();
        for key in keys { self.cancel(&key); }
    }
    pub fn is_empty(&self) -> bool { self.runs.is_empty() }
    pub fn team_started(&mut self, project_id: &str, snapshot: RuntimeSnapshot) {
        self.team.entry(project_id.to_string()).or_default().push(snapshot);
    }
    pub fn team_status(&mut self, project_id: &str, run_id: &str, status: &str) {
        if let Some(snapshot) = self.team.get_mut(project_id).and_then(|list| list.iter_mut().find(|item| item.run_id == run_id)) {
            snapshot.status = status.to_string();
        }
    }
    pub fn team_finished(&mut self, project_id: &str, run_id: &str) {
        if let Some(list) = self.team.get_mut(project_id) {
            list.retain(|item| item.run_id != run_id);
            if list.is_empty() { self.team.remove(project_id); }
        }
    }
    /// 给界面恢复用的运行快照：直接点名的回合优先，否则取团队里最近启动的一个。
    pub fn active_snapshot(&self, project_id: &str) -> Option<RuntimeSnapshot> {
        self.snapshots.get(project_id).cloned()
            .or_else(|| self.team.get(project_id).and_then(|list| list.last().cloned()))
    }
    /// 进程树没清干净：目录改挂到这个 run 名下，直到用户确认。
    pub fn block_directory(&mut self, canonical: PathBuf, run_id: &str) {
        self.directories.insert(canonical, run_id.to_string());
    }
    pub fn run_for(&self, key: &str) -> Option<String> { self.runs.get(key).map(|(id, _)| id.clone()) }
    pub fn replace_reservation(&mut self, key: &str, old_id: &str, run_id: &str) {
        if let Some((id, _)) = self.runs.get_mut(key) { *id = run_id.to_string(); }
        for id in self.directories.values_mut() { if id == old_id { *id = run_id.to_string(); } }
        for id in self.sessions.values_mut() { if id == old_id { *id = run_id.to_string(); } }
        self.generations.remove(old_id); self.generations.insert(run_id.to_string(), 0);
    }
}

pub struct StartedMessage {
    pub prepared: PreparedTurn,
    pub request: crate::adapters::TurnRequest,
    pub run_id: String,
    pub user_message: Message,
    pub agent_message_id: i64,
    pub cancellation: watch::Receiver<bool>,
    pub baseline: Option<Usage>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageResponse { pub run_id: String, pub user_message: Message, pub agent_message_id: i64 }

impl AppState {
    pub fn new(conn: Connection, data_dir: PathBuf) -> AppResult<Self> {
        let state = Self { db: Mutex::new(conn), runtime: Mutex::new(ActiveRuns::default()),
            draft_sessions: Mutex::new(crate::assistant::DraftSessions::default()), data_dir,
            watch: crate::fs_watch::WatchHub::default(), rules_guard: Mutex::new(()), instance: crate::store::new_id() };
        state.recover()?; Ok(state)
    }
    pub fn db_lock(&self) -> AppResult<MutexGuard<'_, Connection>> {
        self.db.lock().map_err(|_| AppError::Invalid("数据库连接不可用，请重启应用".into()))
    }
    pub fn runtime_lock(&self) -> AppResult<MutexGuard<'_, ActiveRuns>> {
        self.runtime.lock().map_err(|_| AppError::Invalid("运行状态不可用，请重启应用".into()))
    }
    fn rules_lock(&self) -> AppResult<MutexGuard<'_, ()>> {
        self.rules_guard.lock().map_err(|_| AppError::Invalid("规章状态不可用，请重启应用".into()))
    }

    pub fn save_rule_for_project(&self, project_id: &str, filename: &str, content: &str) -> AppResult<Message> {
        // 写文件、写通知、记摘要在同一把锁里，监听回调不会在中间插进来重复通知。
        let _guard = self.rules_lock()?;
        let folder = {
            let conn = self.db_lock()?;
            projects::get(&conn, project_id)?.folder_path
        };
        rules::validate_filename(filename)?;
        let path = Path::new(&folder);
        let existed = rules::rules_dir(path).join(filename).is_file();
        rules::save(path, filename, content)?;
        let operation = if existed { "替换" } else { "新增" };
        self.record_rule_notice(project_id, path, operation, filename, Some(content))
    }

    pub fn delete_rule_for_project(&self, project_id: &str, filename: &str) -> AppResult<Message> {
        let _guard = self.rules_lock()?;
        let folder = {
            let conn = self.db_lock()?;
            projects::get(&conn, project_id)?.folder_path
        };
        rules::validate_filename(filename)?;
        let path = Path::new(&folder);
        rules::delete(path, filename)?;
        self.record_rule_notice(project_id, path, "删除", filename, None)
    }

    fn record_rule_notice(&self, project_id: &str, folder: &Path, operation: &str, filename: &str, content: Option<&str>) -> AppResult<Message> {
        let current = rules::list(folder)?;
        let notice = rules::change_notice(operation, filename, content, &current);
        let conn = self.db_lock()?;
        let tx = conn.unchecked_transaction()?;
        let message = messages::insert(&tx, project_id, "system", None, "system", &notice)?;
        save_rule_digest(&tx, project_id, &rules::digest_map(&current))?;
        tx.commit()?;
        Ok(message)
    }

    /// 比对磁盘上的规章和上次记录的摘要。有外部变化时写一条系统消息并返回它。
    pub fn sync_rules(&self, project_id: &str) -> AppResult<Option<(Message, String)>> {
        let _guard = self.rules_lock()?;
        let folder = {
            let conn = self.db_lock()?;
            projects::get(&conn, project_id)?.folder_path
        };
        // 项目文件夹整个不在时由项目页提示重新选择，不当成规章被清空。
        if !Path::new(&folder).is_dir() {
            return Ok(None);
        }
        let current = rules::list(Path::new(&folder))?;
        let digest = rules::digest_map(&current);
        let conn = self.db_lock()?;
        let Some(previous) = load_rule_digest(&conn, project_id)? else {
            save_rule_digest(&conn, project_id, &digest)?;
            return Ok(None);
        };
        let Some(notice) = rules::external_change_notice(&previous, &current) else {
            return Ok(None);
        };
        let tx = conn.unchecked_transaction()?;
        let message = messages::insert(&tx, project_id, "system", None, "system", &notice)?;
        save_rule_digest(&tx, project_id, &digest)?;
        tx.commit()?;
        Ok(Some((message, rules::fingerprint(&current))))
    }

    pub fn rules_version(&self, project_id: &str) -> AppResult<String> {
        let folder = {
            let conn = self.db_lock()?;
            projects::get(&conn, project_id)?.folder_path
        };
        Ok(rules::fingerprint(&rules::list(Path::new(&folder))?))
    }

    fn recover(&self) -> AppResult<()> {
        let mut runtime = self.runtime_lock()?;
        let conn = self.db_lock()?;
        let tx = conn.unchecked_transaction()?;
        let interrupted = runs::interrupt_running_in_transaction(&tx)?;
        let mut owners = load_owners(&tx)?;
        // 兼容只有旧 running 记录的数据库，也必须阻塞对应目录。
        for run in interrupted {
            if owners.iter().any(|owner| owner.run_id == run.id) { continue; }
            if let Some(agent_id) = run.agent_id.as_deref() {
                if let Ok(agent) = agents::get(&tx, agent_id) {
                    let project = projects::get(&tx, &agent.project_id)?;
                    let folder = Path::new(&project.folder_path).canonicalize().unwrap_or_else(|_| project.folder_path.into());
                    owners.push(RuntimeOwner { run_id: run.id, project_id: Some(agent.project_id), draft_id: None,
                        folder_path: folder.to_string_lossy().into_owned(), pid: None, process_group: None,
                        nonce: crate::store::new_id(), instance: "未知旧进程".into(), started_at: run.started_at, cleanup_required: true });
                }
            }
        }
        for owner in &mut owners {
            owner.cleanup_required = true;
            tx.execute("UPDATE runs SET status='interrupted',ended_at=COALESCE(ended_at,?1) WHERE id=?2", rusqlite::params![crate::store::now_ms(), owner.run_id])?;
            save_owner(&tx, owner)?;
            if let Some(project_id) = &owner.project_id {
                tx.execute("UPDATE projects SET status='waiting_user' WHERE id=?1", [project_id])?;
            }
            runtime.directories.entry(PathBuf::from(&owner.folder_path)).or_insert_with(|| owner.run_id.clone());
        }
        // 不调用 runner，也不按 PID 杀进程。只解释中断前的任务和队列，不改写已发出的提示词。
        let now = crate::store::now_ms();
        let reason = "应用已重启，运行中断。执行中的任务记为失败，未自动重跑。部分文件可能已改动。";
        tx.execute(
            "UPDATE tasks SET status = 'failed', finished_at = ?1 WHERE status = 'running'",
            [now],
        )?;
        tx.execute(
            "UPDATE tasks SET status = 'cancelled', finished_at = ?1 \
             WHERE status = 'pending' AND instruction_id IN \
               (SELECT id FROM instructions WHERE status IN ('running', 'waiting_user'))",
            [now],
        )?;
        tx.execute(
            "UPDATE instruction_inputs SET status = 'cancelled', finished_at = ?1 \
             WHERE status = 'queued' AND instruction_id IN \
               (SELECT id FROM instructions WHERE status IN ('running', 'waiting_user'))",
            [now],
        )?;
        tx.execute(
            "UPDATE projects SET status = 'waiting_user', updated_at = ?1 \
             WHERE id IN (SELECT project_id FROM instructions WHERE status = 'running')",
            [now],
        )?;
        tx.execute(
            "UPDATE instructions SET status = 'waiting_user', wait_reason = ?1, updated_at = ?2 \
             WHERE status = 'running'",
            rusqlite::params![reason, now],
        )?;
        tx.commit()?; Ok(())
    }
    pub fn new_owner(&self, run_id: &str, project_id: Option<&str>, draft_id: Option<&str>, folder: &Path) -> RuntimeOwner {
        RuntimeOwner { run_id: run_id.into(), project_id: project_id.map(str::to_owned), draft_id: draft_id.map(str::to_owned),
            folder_path: folder.to_string_lossy().into_owned(), pid: None, process_group: None,
            nonce: crate::store::new_id(), instance: self.instance.clone(), started_at: crate::store::now_ms(), cleanup_required: false }
    }
    pub fn mark_started(&self, key: &str, run_id: &str, pid: u32) -> Result<(), String> {
        self.mark_started_under(key, run_id, run_id, pid)
    }
    /// 团队回合挂在项目的整段预留之下：`holder` 是预留号，`run_id` 是这次回合。
    pub fn mark_started_under(&self, key: &str, holder: &str, run_id: &str, pid: u32) -> Result<(), String> {
        let runtime = self.runtime_lock().map_err(|e| e.to_string())?;
        if !runtime.current(key, holder) { return Err("运行预留已失效".into()); }
        let conn = self.db_lock().map_err(|e| e.to_string())?;
        let mut owner = load_owners(&conn).map_err(|e| e.to_string())?.into_iter().find(|o| o.run_id == run_id)
            .ok_or("缺少进程启动前运行记录")?;
        if owner.instance != self.instance { return Err("运行所有权不属于当前应用".into()); }
        owner.pid = Some(pid); owner.process_group = Some(pid);
        save_owner(&conn, &owner).map_err(|e| e.to_string())
    }
    pub fn prepare_send(&self, project_id: &str, content: &str, cli: &crate::cli_registry::ResolvedCli) -> AppResult<StartedMessage> {
        let mut runtime = self.runtime_lock()?;
        runtime.ensure_project_idle(project_id)?;
        let conn = self.db_lock()?;
        let project = projects::get(&conn, project_id)?;
        let folder = Path::new(&project.folder_path).canonicalize().map_err(|_| AppError::Invalid("项目目录不存在或无法读取".into()))?;
        if !folder.is_dir() { return Err(AppError::Invalid("项目路径不是文件夹".into())); }
        let roster = agents::list(&conn, project_id)?;
        let (agent, _) = crate::chat::resolve_recipient(content, &roster)?;
        if cli.info.id != agent.cli || !cli.info.supported { return Err(AppError::Invalid("成员 CLI 在发送前已改变，请重新发送".into())); }
        let reservation = crate::store::new_id();
        let cancellation = runtime.reserve(project_id, &reservation)?;
        if let Err(error) = runtime.reserve_directory(&folder, &reservation) {
            runtime.remove(project_id, &reservation); return Err(error);
        }
        let prepared = (|| {
            let tx = conn.unchecked_transaction()?;
            let user = messages::insert(&tx, project_id, "user", None, "chat", content.trim())?;
            let prepared = ChatService::send(&tx, project_id, user.id, false)?;
            let request = ChatService::request(&prepared, folder.clone());
            runtime.reserve_session(&prepared.agent.cli, request.session_id.as_deref(), &reservation)?;
            let baseline = request.session_id.as_deref().map(|id| runs::latest_usage(&tx, &prepared.agent.id, id)).transpose()?.flatten()
                .and_then(|value| serde_json::from_value(value).ok());
            let adapter = make_adapter(cli, RunMode::Execute, baseline.clone())?;
            let spec = adapter.build_command(&request);
            let command = serde_json::to_string(&json!({"program":spec.program, "args":spec.args}))
                .map_err(|e| AppError::Invalid(e.to_string()))?;
            let run = runs::insert_running(&tx, Some(&prepared.agent.id), "execute", &command, request.session_id.as_deref())?;
            runs::set_requested_model(&tx, &run.id, request.model.as_deref())?;
            let placeholder = messages::insert(&tx, project_id, "assistant", Some(&prepared.agent.id), "chat", "")?;
            save_owner(&tx, &self.new_owner(&run.id, Some(project_id), None, &folder))?;
            tx.execute("UPDATE projects SET status='executing',updated_at=?1 WHERE id=?2", rusqlite::params![crate::store::now_ms(), project_id])?;
            tx.commit()?;
            Ok::<_, AppError>((prepared, request, baseline, run.id, placeholder.id, user))
        })();
        let (prepared, request, baseline, run_id, agent_message_id, user_message) = match prepared {
            Ok(value) => value,
            Err(error) => { runtime.remove(project_id, &reservation); return Err(error); }
        };
        // 用实际数据库 runId 替换原子预留号，取消 sender 保持不变。
        if let Some((id, _)) = runtime.runs.get_mut(project_id) { *id = run_id.clone(); }
        for id in runtime.directories.values_mut() { if id == &reservation { *id = run_id.clone(); } }
        for id in runtime.sessions.values_mut() { if id == &reservation { *id = run_id.clone(); } }
        runtime.generations.remove(&reservation); runtime.generations.insert(run_id.clone(), 0);
        runtime.snapshots.insert(project_id.into(), RuntimeSnapshot { project_id: project_id.into(), run_id: run_id.clone(),
            agent_id: prepared.agent.id.clone(), status: "thinking".into(), seq: 0, partial_text: String::new() });
        if content.trim().starts_with('@') {
            if let Err(error) = crate::orchestrator::open_direct_instruction(&conn, project_id, user_message.id, &prepared.agent.id) {
                runtime.remove(project_id, &run_id);
                return Err(error);
            }
        }
        Ok(StartedMessage { prepared, request, run_id, user_message, agent_message_id, cancellation, baseline })
    }
    /// 用户消息和直接指令已经落库。这里只补运行预留、占位消息和执行回合。
    pub fn launch_existing_message(&self, project_id: &str, user_message_id: i64, cli: &crate::cli_registry::ResolvedCli) -> AppResult<StartedMessage> {
        let mut runtime = self.runtime_lock()?;
        runtime.ensure_project_idle(project_id)?;
        let conn = self.db_lock()?;
        let project = projects::get(&conn, project_id)?;
        let folder = Path::new(&project.folder_path).canonicalize().map_err(|_| AppError::Invalid("项目目录不存在或无法读取".into()))?;
        if !folder.is_dir() { return Err(AppError::Invalid("项目路径不是文件夹".into())); }
        let roster = agents::list(&conn, project_id)?;
        let user_message = messages::get(&conn, user_message_id)?;
        let (agent, _) = crate::chat::resolve_recipient(&user_message.content, &roster)?;
        if cli.info.id != agent.cli || !cli.info.supported { return Err(AppError::Invalid("成员 CLI 在发送前已改变，请重新发送".into())); }
        let reservation = crate::store::new_id();
        let cancellation = runtime.reserve(project_id, &reservation)?;
        if let Err(error) = runtime.reserve_directory(&folder, &reservation) {
            runtime.remove(project_id, &reservation);
            return Err(error);
        }
        let prepared_result = (|| {
            let tx = conn.unchecked_transaction()?;
            let prepared = ChatService::send(&tx, project_id, user_message_id, false)?;
            let request = ChatService::request(&prepared, folder.clone());
            runtime.reserve_session(&prepared.agent.cli, request.session_id.as_deref(), &reservation)?;
            let baseline = request.session_id.as_deref().map(|id| runs::latest_usage(&tx, &prepared.agent.id, id)).transpose()?.flatten()
                .and_then(|value| serde_json::from_value(value).ok());
            let adapter = make_adapter(cli, RunMode::Execute, baseline.clone())?;
            let spec = adapter.build_command(&request);
            let command = serde_json::to_string(&json!({"program": spec.program, "args": spec.args})).map_err(|e| AppError::Invalid(e.to_string()))?;
            let run = runs::insert_running(&tx, Some(&prepared.agent.id), "execute", &command, request.session_id.as_deref())?;
            runs::set_requested_model(&tx, &run.id, request.model.as_deref())?;
            let placeholder = messages::insert(&tx, project_id, "assistant", Some(&prepared.agent.id), "chat", "")?;
            save_owner(&tx, &self.new_owner(&run.id, Some(project_id), None, &folder))?;
            tx.execute("UPDATE projects SET status='executing',updated_at=?1 WHERE id=?2", rusqlite::params![crate::store::now_ms(), project_id])?;
            tx.commit()?;
            Ok::<_, AppError>((prepared, request, baseline, run.id, placeholder.id))
        })();
        let (prepared, request, baseline, run_id, agent_message_id) = match prepared_result {
            Ok(value) => value,
            Err(error) => { runtime.remove(project_id, &reservation); return Err(error); }
        };
        if let Some((id, _)) = runtime.runs.get_mut(project_id) { *id = run_id.clone(); }
        for id in runtime.directories.values_mut() { if id == &reservation { *id = run_id.clone(); } }
        for id in runtime.sessions.values_mut() { if id == &reservation { *id = run_id.clone(); } }
        runtime.generations.remove(&reservation); runtime.generations.insert(run_id.clone(), 0);
        runtime.snapshots.insert(project_id.into(), RuntimeSnapshot { project_id: project_id.into(), run_id: run_id.clone(),
            agent_id: prepared.agent.id.clone(), status: "thinking".into(), seq: 0, partial_text: String::new() });
        Ok(StartedMessage { prepared, request, run_id, user_message, agent_message_id, cancellation, baseline })
    }
    pub fn runtime_snapshot(&self, project_id: &str) -> AppResult<ProjectRuntime> {
        let runtime = self.runtime_lock()?; let conn = self.db_lock()?;
        let project = projects::get(&conn, project_id)?;
        let folder = Path::new(&project.folder_path).canonicalize().unwrap_or_else(|_| project.folder_path.into());
        let cleanup_required = load_owners(&conn)?.into_iter().filter(|owner| owner.cleanup_required &&
            (owner.project_id.as_deref() == Some(project_id) || Path::new(&owner.folder_path) == folder))
            .map(|owner| CleanupNotice { run_id: owner.run_id, project_id: owner.project_id, folder_path: owner.folder_path }).collect();
        Ok(ProjectRuntime { active: runtime.active_snapshot(project_id), cleanup_required })
    }
    pub fn confirm_cleanup(&self, run_id: &str) -> AppResult<()> {
        let mut runtime = self.runtime_lock()?; let conn = self.db_lock()?;
        if runtime.runs.values().any(|(id, _)| id == run_id) { return Err(AppError::Invalid("不能确认活动回合的清理".into())); }
        let owner = load_owners(&conn)?.into_iter().find(|owner| owner.run_id == run_id).ok_or_else(|| AppError::NotFound("待清理运行".into()))?;
        if !owner.cleanup_required || runs::get(&conn, run_id)?.status != "interrupted" { return Err(AppError::Invalid("只能确认已中断运行的清理".into())); }
        remove_owner(&conn, run_id)?;
        runtime.directories.retain(|_, id| id != run_id);
        // 同目录有多个残留时仍保持阻塞，直到逐一确认。
        for pending in load_owners(&conn)?.into_iter().filter(|o| o.cleanup_required) {
            runtime.directories.entry(PathBuf::from(pending.folder_path)).or_insert(pending.run_id);
        }
        Ok(())
    }
}

pub fn make_adapter(cli: &crate::cli_registry::ResolvedCli, mode: RunMode, baseline: Option<Usage>) -> AppResult<Box<dyn CliAdapter>> {
    match cli.info.id.as_str() {
        "claude" => Ok(Box::new(crate::adapters::claude::ClaudeAdapter::new(cli.program.clone(), mode).with_environment(cli))),
        "codex" => Ok(Box::new(crate::adapters::codex::CodexAdapter::new(cli.program.clone(), mode, baseline).with_environment(cli))),
        "grok" => Ok(Box::new(crate::adapters::grok::GrokAdapter::new(cli.program.clone(), mode).with_environment(cli))),
        "dsh" => Ok(Box::new(crate::adapters::dsh::DshAdapter::new(cli.program.clone(), mode).with_environment(cli))),
        id => Err(AppError::Invalid(format!("CLI {id} 尚未接入，计划在 M5 实现"))),
    }
}

/// 团队回合的展示状态。讨论有文本才算发言，执行一有输出就算干活，出错后不再改回。
pub fn live_status(mode: RunMode, event: &AgentEvent, current: &'static str) -> &'static str {
    if current == "error" || matches!(event, AgentEvent::Error { .. }) {
        return "error";
    }
    match (mode, event) {
        (RunMode::Execute, _) => "working",
        (RunMode::Discuss, AgentEvent::TextDelta { .. }) => "talking",
        (RunMode::Discuss, _) => current,
    }
}

/// 团队回合开始时记下请求的模型。运行记录由编排器先创建，所以这里只补写。写失败不影响回合。
fn record_requested_model(state: &AppState, run_id: &str, model: Option<&str>) {
    if let Ok(conn) = state.db_lock() { let _ = runs::set_requested_model(&conn, run_id, model); }
}

/// 团队回合的事件处理：CLI 回报模型时写进运行记录（会话与用量仍在回合结束时由编排器落库）。
fn record_reported_model(state: &AppState, run_id: &str, event: &AgentEvent) {
    let AgentEvent::ModelReported { model } = event else { return };
    if let Ok(conn) = state.db_lock() { let _ = runs::set_reported_model(&conn, run_id, model); }
}

/// 结果没能落库（已停止等）时，不能把回合报成成功。
pub fn finished_status(outcome_status: &str, applied: bool) -> &str {
    if !applied && outcome_status == "ok" { "failed" } else { outcome_status }
}

impl AppState {
    pub fn make_runtime_adapter(&self, cli: &crate::cli_registry::ResolvedCli, mode: RunMode, baseline: Option<Usage>) -> AppResult<Box<dyn CliAdapter>> {
        let adapter = if cli.info.id == "dsh" {
            let settings = { let conn = self.db_lock()?; crate::dsh_config::load(&conn)? };
            Box::new(crate::adapters::dsh::DshAdapter::new(cli.program.clone(), mode).with_environment(cli).with_settings(settings)) as Box<dyn CliAdapter>
        } else { make_adapter(cli, mode, baseline)? };
        let settings = { let conn = self.db_lock()?; crate::proxy::load(&conn)? };
        let environment = crate::proxy::resolve(&settings)?.map(|proxy| crate::proxy::overrides(&proxy)).unwrap_or_default();
        Ok(Box::new(crate::proxy::ProxyAdapter::new(adapter, environment)))
    }
    pub fn apply_project_event(&self, started: &StartedMessage, event: AgentEvent) -> AppResult<Vec<(&'static str, Value)>> {
        let mut runtime = self.runtime_lock()?;
        let project_id = &started.prepared.agent.project_id;
        if !runtime.accepts_events(project_id, &started.run_id) { return Ok(vec![]); }
        let conn = self.db_lock()?;
        ChatService::on_event(&conn, &started.prepared, &started.run_id, &event)?;
        let Some(snapshot) = runtime.snapshots.get_mut(project_id) else { return Ok(vec![]); };
        let mut emitted = Vec::new();
        match event {
            AgentEvent::TextDelta { text } => {
                snapshot.seq += 1; snapshot.partial_text.push_str(&text); snapshot.status = "talking".into();
                messages::update_content(&conn, started.agent_message_id, &snapshot.partial_text)?;
                emitted.push(("message:delta", json!({"projectId": project_id, "runId": started.run_id,
                    "agentId": started.prepared.agent.id, "seq": snapshot.seq, "text": text})));
            }
            AgentEvent::ToolUse { .. } | AgentEvent::ToolState { active: true } => snapshot.status = "working".into(),
            AgentEvent::ToolState { active: false } => snapshot.status = "thinking".into(),
            AgentEvent::Error { .. } => snapshot.status = "error".into(),
            _ => {}
        }
        emitted.push(("agent:status", json!({"projectId": project_id, "runId": started.run_id,
            "agentId": started.prepared.agent.id, "status": snapshot.status})));
        Ok(emitted)
    }
    pub fn finish_project(&self, started: &StartedMessage, mut outcome: crate::runner::RunOutcome) -> AppResult<Vec<(&'static str, Value)>> {
        let mut runtime = self.runtime_lock()?;
        let project_id = &started.prepared.agent.project_id;
        if !runtime.current(project_id, &started.run_id) { return Ok(vec![]); }
        let was_cancelled = !runtime.accepts_events(project_id, &started.run_id);
        if was_cancelled && outcome.status == "ok" {
            outcome.status = "cancelled".into(); outcome.error = Some("回合已停止，文件修改不会自动撤销".into());
        }
        let conn = self.db_lock()?;
        let tx = conn.unchecked_transaction()?;
        let status = if outcome.cleanup_complete { outcome.status.as_str() } else { "interrupted" };
        let usage = outcome.usage.as_ref().map(serde_json::to_string).transpose().map_err(|e| AppError::Invalid(e.to_string()))?;
        if !runs::finish(&tx, &started.run_id, status, outcome.exit_code, usage.as_deref(), outcome.error.as_deref())? {
            // 停止已经把 run 写成终态时，仍然释放内存中的这一代。
            runtime.remove(project_id, &started.run_id);
            return Ok(vec![]);
        }
        let content = if outcome.status == "ok" { &outcome.final_text } else { &outcome.partial_text };
        let message = messages::update_content(&tx, started.agent_message_id, content)?;
        if outcome.status == "ok" { agents::finish_context(&tx, &started.prepared.agent.id,
            &started.prepared.agent.persona, started.prepared.snapshot_message_id)?; }
        let mut events = vec![("message:new", json!({"projectId":project_id,"runId":started.run_id,"message":message}))];
        if outcome.status != "ok" {
            let error = outcome.error.as_deref().unwrap_or("CLI 回合未完成");
            let system = messages::insert(&tx, project_id, "system", None, "system", error)?;
            events.push(("message:new", json!({"projectId":project_id,"runId":started.run_id,"message":system})));
        }
        tx.execute("UPDATE projects SET status=?1,updated_at=?2 WHERE id=?3",
            rusqlite::params![if outcome.status == "ok" { "idle" } else { "waiting_user" }, crate::store::now_ms(), project_id])?;
        let pending_owner = if outcome.cleanup_complete {
            remove_owner(&tx, &started.run_id)?; None
        } else {
            let mut owner = load_owners(&tx)?.into_iter().find(|owner| owner.run_id == started.run_id)
                .ok_or_else(|| AppError::Invalid("缺少待清理运行记录".into()))?;
            owner.cleanup_required = true; save_owner(&tx, &owner)?; Some(owner)
        };
        tx.commit()?;
        if let Ok(Some(active)) = crate::store::instructions::active_instruction(&conn, project_id) {
            if active.kind == "direct" {
                let _ = crate::orchestrator::finish_direct_instruction(&conn, &active.id, outcome.status == "ok");
            }
        }
        let seq = runtime.snapshots.get(project_id).map(|s| s.seq + 1).unwrap_or(1);
        runtime.remove(project_id, &started.run_id);
        if let Some(owner) = pending_owner { runtime.directories.insert(owner.folder_path.into(), owner.run_id); }
        events.push(("agent:status", json!({"projectId":project_id,"runId":started.run_id,
            "agentId":started.prepared.agent.id,"status":if outcome.status == "ok" || outcome.status == "cancelled" {"idle"} else {"error"}})));
        events.push(("run:finished", json!({"projectId":project_id,"runId":started.run_id,
            "agentId":started.prepared.agent.id,"seq":seq,"status":outcome.status,"error":outcome.error})));
        Ok(events)
    }
    pub fn rebuild_started(&self, started: &mut StartedMessage, cli: &crate::cli_registry::ResolvedCli, outcome: &crate::runner::RunOutcome) -> AppResult<Vec<(&'static str, Value)>> {
        let mut runtime = self.runtime_lock()?;
        let project_id = &started.prepared.agent.project_id;
        if !runtime.accepts_events(project_id, &started.run_id) { return Err(AppError::Invalid("回合已经停止".into())); }
        let conn = self.db_lock()?; let tx = conn.unchecked_transaction()?;
        runs::finish(&tx, &started.run_id, "failed", outcome.exit_code, None, outcome.error.as_deref())?;
        remove_owner(&tx, &started.run_id)?;
        agents::set_session(&tx, &started.prepared.agent.id, None)?;
        tx.execute("UPDATE projects SET status='idle' WHERE id=?1", [project_id])?;
        let prepared = ChatService::send(&tx, project_id, started.user_message.id, true)?;
        let request = ChatService::request(&prepared, started.request.cwd.clone());
        let spec = make_adapter(cli, RunMode::Execute, None)?.build_command(&request);
        let run = runs::insert_running(&tx, Some(&prepared.agent.id), "execute",
            &serde_json::to_string(&json!({"program":spec.program,"args":spec.args})).map_err(|e| AppError::Invalid(e.to_string()))?, None)?;
        runs::set_requested_model(&tx, &run.id, request.model.as_deref())?;
        save_owner(&tx, &self.new_owner(&run.id, Some(project_id), None, &request.cwd))?;
        tx.execute("UPDATE projects SET status='executing' WHERE id=?1", [project_id])?; tx.commit()?;
        let old_id = started.run_id.clone();
        let seq = runtime.snapshots.get(project_id).map(|s| s.seq + 1).unwrap_or(1);
        if let Some((id, _)) = runtime.runs.get_mut(project_id) { *id = run.id.clone(); }
        for id in runtime.directories.values_mut() { if id == &old_id { *id = run.id.clone(); } }
        runtime.sessions.retain(|_, id| id != &old_id);
        runtime.generations.remove(&old_id); runtime.generations.insert(run.id.clone(), 0);
        runtime.snapshots.insert(project_id.clone(), RuntimeSnapshot { project_id: project_id.clone(), run_id: run.id.clone(), agent_id: prepared.agent.id.clone(), status: "thinking".into(), seq: 0, partial_text: String::new() });
        let events = vec![("run:finished", json!({"projectId":project_id,"runId":old_id,"agentId":prepared.agent.id,
            "seq":seq,"status":"failed","error":null,
            "replacement":{"runId":run.id,"messageId":started.agent_message_id}}))];
        started.run_id = run.id; started.prepared = prepared; started.request = request; started.baseline = None;
        Ok(events)
    }
}

pub fn emit_events(app: &tauri::AppHandle, events: Vec<(&str, Value)>) {
    for (name, payload) in events { let _ = app.emit(name, payload); }
}

#[tauri::command]
pub fn list_messages(state: State<'_, AppState>, project_id: String, before_id: Option<i64>, limit: Option<usize>) -> AppResult<Vec<Message>> {
    let conn = lock(&state)?; projects::get(&conn, &project_id)?;
    messages::list(&conn, &project_id, before_id, limit.unwrap_or(50))
}
#[tauri::command]
pub fn get_project_runtime(state: State<'_, AppState>, project_id: String) -> AppResult<ProjectRuntime> {
    state.runtime_snapshot(&project_id)
}
#[tauri::command]
pub fn confirm_runtime_cleanup(state: State<'_, AppState>, run_id: String) -> AppResult<()> { state.confirm_cleanup(&run_id) }

#[tauri::command]
pub async fn send_message(app: tauri::AppHandle, state: State<'_, AppState>, project_id: String, content: String) -> AppResult<SendMessageResponse> {
    let text = content.trim().to_string();
    if text.is_empty() { return Err(AppError::Invalid("消息不能为空".into())); }
    let active_status = {
        let conn = state.db_lock()?;
        crate::store::instructions::active_instruction(&conn, &project_id)?.map(|item| item.status)
    };
    if active_status.as_deref() == Some("running") {
        let message = {
            let conn = state.db_lock()?;
            crate::orchestrator::queue_user_text(&conn, &project_id, &text)?
        };
        let run_id = {
            let conn = state.db_lock()?;
            conn.query_row(
                "SELECT runs.id FROM runs JOIN instructions ON instructions.id = runs.instruction_id \
                 WHERE instructions.project_id = ?1 AND runs.status = 'running' ORDER BY runs.started_at DESC LIMIT 1",
                [&project_id], |row| row.get(0),
            ).ok()
        }.unwrap_or_default();
        emit_events(&app, vec![("message:new", json!({"projectId": project_id, "runId": run_id, "message": &message}))]);
        return Ok(SendMessageResponse { run_id, user_message: message, agent_message_id: 0 });
    }
    let mentioned = text.starts_with('@');
    if mentioned && active_status.is_none() {
        return launch_direct_message(app, state, project_id, text).await;
    }
    if mentioned {
        let instruction = {
            let conn = state.db_lock()?;
            crate::orchestrator::begin_user_instruction(&conn, &project_id, &text)?
        };
        let cli_id = {
            let conn = state.db_lock()?;
            agents::get(&conn, instruction.target_agent_id.as_deref().unwrap_or(""))?.cli
        };
        let cli = tauri::async_runtime::spawn_blocking(move || cli_registry::resolve_cli(&cli_id)).await
            .map_err(|e| AppError::Invalid(format!("CLI 探测失败：{e}")))??;
        let started = state.launch_existing_message(&project_id, instruction.source_message_id, &cli)?;
        return spawn_direct(app, project_id, started, cli);
    }
    let instruction = {
        let conn = state.db_lock()?;
        crate::orchestrator::begin_user_instruction(&conn, &project_id, &text)?
    };
    begin_change_window(&app, &project_id);
    let user_message = {
        let conn = state.db_lock()?;
        messages::get(&conn, instruction.source_message_id)?
    };
    let specs = {
        let conn = state.db_lock()?;
        match crate::orchestrator::advance(&conn, &project_id) {
            Ok(specs) => specs,
            Err(error) => {
                let _ = crate::orchestrator::stop_instruction(&conn, &project_id);
                return Err(error);
            }
        }
    };
    let run_id = specs.first().map(|spec| spec.run_id.clone()).unwrap_or_default();
    emit_events(&app, vec![("message:new", json!({"projectId": &project_id, "runId": &run_id, "message": &user_message}))]);
    let worker_app = app.clone();
    let worker_project = project_id.clone();
    tauri::async_runtime::spawn(async move { drive_team(worker_app, worker_project, specs).await; });
    Ok(SendMessageResponse { run_id, user_message, agent_message_id: 0 })
}

async fn launch_direct_message(app: tauri::AppHandle, state: State<'_, AppState>, project_id: String, content: String) -> AppResult<SendMessageResponse> {
    let cli_id = {
        let conn = state.db_lock()?;
        let roster = agents::list(&conn, &project_id)?;
        crate::chat::resolve_recipient(&content, &roster)?.0.cli.clone()
    };
    let cli = tauri::async_runtime::spawn_blocking(move || cli_registry::resolve_cli(&cli_id)).await
        .map_err(|e| AppError::Invalid(format!("CLI 探测失败：{e}")))??;
    let started = state.prepare_send(&project_id, &content, &cli)?;
    spawn_direct(app, project_id, started, cli)
}

fn spawn_direct(app: tauri::AppHandle, project_id: String, started: StartedMessage, cli: crate::cli_registry::ResolvedCli) -> AppResult<SendMessageResponse> {
    begin_change_window(&app, &project_id);
    let response = SendMessageResponse { run_id: started.run_id.clone(), user_message: started.user_message.clone(), agent_message_id: started.agent_message_id };
    emit_events(&app, vec![("message:new", json!({"projectId":project_id,"runId":started.run_id,"message":started.user_message})),
        ("agent:status", json!({"projectId":project_id,"runId":started.run_id,"agentId":started.prepared.agent.id,"status":"thinking"}))]);
    let rescue_project = project_id; let rescue_id = started.run_id.clone();
    let worker_app = app.clone();
    let worker = tauri::async_runtime::spawn(async move { execute_project(worker_app, started, cli).await; });
    tauri::async_runtime::spawn(async move {
        if worker.await.is_err() {
            let state = app.state::<AppState>();
            if let (Ok(mut runtime), Ok(conn)) = (state.runtime_lock(), state.db_lock()) {
                if runtime.current(&rescue_project, &rescue_id) {
                    let _ = runs::finish(&conn, &rescue_id, "interrupted", None, None, Some("运行任务异常退出，请人工检查残留进程"));
                    if let Ok(owners) = load_owners(&conn) {
                        if let Some(mut owner) = owners.into_iter().find(|o| o.run_id == rescue_id) {
                            owner.cleanup_required = true; let _ = save_owner(&conn, &owner);
                            runtime.remove(&rescue_project, &rescue_id); runtime.directories.insert(owner.folder_path.into(), rescue_id.clone());
                        }
                    }
                    let _ = conn.execute("UPDATE projects SET status='waiting_user' WHERE id=?1", [&rescue_project]);
                }
            }
        }
    });
    Ok(response)
}

async fn execute_project(app: tauri::AppHandle, mut started: StartedMessage, cli: crate::cli_registry::ResolvedCli) {
    let mut retried = false;
    loop {
        let adapter = match app.state::<AppState>().make_runtime_adapter(&cli, RunMode::Execute, started.baseline.clone()) {
            Ok(adapter) => adapter,
            Err(error) => {
                let outcome = crate::runner::RunOutcome { status: "failed".into(), session_id: started.request.session_id.clone(),
                    final_text: String::new(), partial_text: String::new(), usage: None, exit_code: None,
                    error: Some(error.to_string()), tool_seen: false, cleanup_complete: true };
                if let Ok(events) = app.state::<AppState>().finish_project(&started, outcome) { emit_events(&app, events); }
                return;
            }
        };
        let outcome = crate::runner::run_turn_managed(adapter, started.request.clone(), started.cancellation.clone(), |event| {
            let state = app.state::<AppState>();
            match state.apply_project_event(&started, event) {
                Ok(events) => emit_events(&app, events),
                Err(_) => if let Ok(mut runtime) = state.runtime_lock() { runtime.cancel(&started.prepared.agent.project_id); },
            }
        }, crate::runner::RunnerConfig::default(), Some(app.state::<AppState>().data_dir.join("runs").join(format!("{}.log", started.run_id))),
        |pid| app.state::<AppState>().mark_started(&started.prepared.agent.project_id, &started.run_id, pid)).await;
        if crate::chat::should_rebuild_session(&outcome, started.request.session_id.is_some(), retried) {
            if let Ok(events) = app.state::<AppState>().rebuild_started(&mut started, &cli, &outcome) {
                emit_events(&app, events); retried = true; continue;
            }
        }
        if let Ok(events) = app.state::<AppState>().finish_project(&started, outcome) { emit_events(&app, events); }
        break;
    }
    let project_id = started.prepared.agent.project_id.clone();
    let still_running = app.state::<AppState>().db_lock().ok().and_then(|conn| {
        crate::store::instructions::active_instruction(&conn, &project_id).ok().flatten()
            .filter(|item| item.status == "running").map(|_| ())
    }).is_some();
    if still_running {
        drive_team(app, project_id, Vec::new()).await;
    }
}

enum StepResult { Outcome(crate::runner::RunOutcome), NotStarted(String) }

fn emit_run_finished(app: &tauri::AppHandle, project_id: &str, spec: &crate::orchestrator::TurnSpec, status: &str, error: Option<String>) {
    emit_events(app, vec![("run:finished", json!({"projectId": project_id, "runId": spec.run_id,
        "agentId": spec.agent_id, "seq": 1, "status": status, "error": error}))]);
}

/// 占住项目和工作目录，直到这条指令的调度结束。回合之间也不放开。
fn begin_team_hold(app: &tauri::AppHandle, project_id: &str, hold: &str) -> Result<watch::Receiver<bool>, String> {
    let state = app.state::<AppState>();
    let folder = {
        let conn = state.db_lock().map_err(|e| e.to_string())?;
        let project = projects::get(&conn, project_id).map_err(|e| e.to_string())?;
        Path::new(&project.folder_path).canonicalize().map_err(|_| "项目目录不存在或无法读取".to_string())?
    };
    let mut runtime = state.runtime_lock().map_err(|e| e.to_string())?;
    let receiver = runtime.reserve(project_id, hold).map_err(|e| e.to_string())?;
    if let Err(error) = runtime.reserve_directory(&folder, hold) {
        runtime.remove(project_id, hold);
        return Err(error.to_string());
    }
    Ok(receiver)
}

fn end_team_hold(app: &tauri::AppHandle, project_id: &str, hold: &str) {
    if let Ok(mut runtime) = app.state::<AppState>().runtime_lock() { runtime.remove(project_id, hold); }
}

/// 调度没能开始：已经预留的回合全部记为没有启动，指令等待用户。
fn park_team(app: &tauri::AppHandle, project_id: &str, pending: &[crate::orchestrator::TurnSpec], reason: &str) {
    if let Ok(conn) = app.state::<AppState>().db_lock() {
        for spec in pending { let _ = crate::orchestrator::fail_launch(&conn, &spec.run_id, reason); }
        let _ = wait_instruction(&conn, project_id, reason);
    }
    for spec in pending { emit_run_finished(app, project_id, spec, "failed", Some(reason.to_string())); }
}

async fn drive_team(app: tauri::AppHandle, project_id: String, mut pending: Vec<crate::orchestrator::TurnSpec>) {
    let hold = format!("hold:{project_id}");
    let cancellation = match begin_team_hold(&app, &project_id, &hold) {
        Ok(receiver) => receiver,
        Err(reason) => { park_team(&app, &project_id, &pending, &reason); return; }
    };
    loop {
        if pending.is_empty() {
            let next = {
                let state = app.state::<AppState>();
                let Ok(conn) = state.db_lock() else { break };
                let Ok(active) = crate::store::instructions::active_instruction(&conn, &project_id) else { break };
                if active.as_ref().map(|item| item.status.as_str()) != Some("running") { break; }
                crate::orchestrator::advance(&conn, &project_id)
            };
            match next {
                Ok(specs) if !specs.is_empty() => pending = specs,
                _ => break,
            }
        }
        let batch = std::mem::take(&mut pending);
        if batch.iter().any(|spec| !instruction_running(&app, &spec.instruction_id)) { break; }
        // 同一批（并行讨论）一起启动。执行和组长回合每批只有一个。
        let handles: Vec<_> = batch.iter().cloned().map(|spec| {
            let (app, project_id, hold, cancellation) = (app.clone(), project_id.clone(), hold.clone(), cancellation.clone());
            tauri::async_runtime::spawn(async move { run_with_rebuild(&app, &project_id, spec, &hold, cancellation).await })
        }).collect();
        // 结果按批内顺序依次落库，消息顺序可预期。有回合出问题也要等完其余的。
        let mut halted = false;
        for (handle, original) in handles.into_iter().zip(batch.iter()) {
            match handle.await {
                Ok((spec, StepResult::Outcome(outcome))) => {
                    let state = app.state::<AppState>();
                    let Ok(conn) = state.db_lock() else { halted = true; continue };
                    let floor = max_message_id(&conn, &project_id).unwrap_or(0);
                    let applied = crate::orchestrator::apply_outcome(&conn, &spec.run_id, &outcome);
                    emit_messages_after(&app, &conn, &project_id, &spec, floor);
                    drop(conn);
                    let error = match &applied {
                        Ok(()) => outcome.error.clone(),
                        Err(failure) => outcome.error.clone().or_else(|| Some(failure.to_string())),
                    };
                    emit_run_finished(&app, &project_id, &spec, finished_status(&outcome.status, applied.is_ok()), error);
                    if applied.is_err() { halted = true; }
                }
                Ok((spec, StepResult::NotStarted(reason))) => {
                    emit_run_finished(&app, &project_id, &spec, "failed", Some(reason));
                    halted = true;
                }
                Err(_) => {
                    let reason = "运行任务异常退出，请人工检查残留进程";
                    if let Ok(conn) = app.state::<AppState>().db_lock() {
                        let _ = crate::orchestrator::fail_launch(&conn, &original.run_id, reason);
                        let _ = wait_instruction(&conn, &project_id, reason);
                    }
                    emit_run_finished(&app, &project_id, original, "failed", Some(reason.to_string()));
                    halted = true;
                }
            }
        }
        if halted { break; }
    }
    end_team_hold(&app, &project_id, &hold);
}

/// 跑一个回合。会话在启动阶段被拒绝时重建一次再跑。
async fn run_with_rebuild(
    app: &tauri::AppHandle, project_id: &str, mut spec: crate::orchestrator::TurnSpec, hold: &str, cancellation: watch::Receiver<bool>,
) -> (crate::orchestrator::TurnSpec, StepResult) {
    loop {
        let step = run_orchestrated(app, project_id, &spec, hold, cancellation.clone()).await;
        let rebuilt = match &step {
            StepResult::Outcome(outcome) => app.state::<AppState>().db_lock().ok()
                .and_then(|conn| crate::orchestrator::rebuild_turn(&conn, &spec.run_id, outcome).ok().flatten()),
            StepResult::NotStarted(_) => None,
        };
        match rebuilt {
            // rebuild_turn 第二次一定返回 None，这个循环最多多跑一圈。
            Some(next) => {
                emit_run_finished(app, project_id, &spec, "failed", Some("旧会话不可用，已清空并重开一次".into()));
                spec = next;
            }
            None => return (spec, step),
        }
    }
}

async fn run_orchestrated(
    app: &tauri::AppHandle, project_id: &str, spec: &crate::orchestrator::TurnSpec, hold: &str, cancellation: watch::Receiver<bool>,
) -> StepResult {
    let state = app.state::<AppState>();
    // 回合没能启动：结束这次 run，指令等待用户，并让同批已经在跑的成员停下。
    // 调用它之前必须已经放开数据库锁。
    let refuse = |reason: &str| -> StepResult {
        if let Ok(conn) = state.db_lock() {
            let _ = crate::orchestrator::fail_launch(&conn, &spec.run_id, reason);
            let _ = wait_instruction(&conn, project_id, reason);
        }
        if let Ok(mut runtime) = state.runtime_lock() { runtime.cancel(project_id); }
        StepResult::NotStarted(reason.to_string())
    };
    let prepared: Result<(Agent, PathBuf, Option<String>), &'static str> = (|| {
        let conn = state.db_lock().map_err(|_| "数据库连接不可用")?;
        let mut agent = agents::get(&conn, &spec.agent_id).map_err(|_| "成员不存在")?;
        agent.model = crate::cli_config::effective_model(&conn, &agent.cli, agent.model.as_deref())
            .map_err(|_| "无法读取应用配置")?;
        let project = projects::get(&conn, project_id).map_err(|_| "项目不存在")?;
        let folder = Path::new(&project.folder_path).canonicalize().map_err(|_| "项目目录不存在或无法读取")?;
        let session = runs::resumable_session(&conn, &agent, &spec.mode)
            .map_err(|_| "无法读取会话的运行模式")?.map(str::to_owned);
        Ok((agent, folder, session))
    })();
    let (agent, folder, session) = match prepared {
        Ok(value) => value,
        Err(reason) => return refuse(reason),
    };
    let resolved = tauri::async_runtime::spawn_blocking({
        let cli_id = agent.cli.clone();
        move || cli_registry::resolve_cli(&cli_id)
    }).await;
    let cli = match resolved {
        Ok(Ok(cli)) => cli,
        Ok(Err(error)) => return refuse(&format!("CLI 不可用：{error}")),
        Err(_) => return refuse("CLI 不可用"),
    };
    let mode = if spec.mode == "discuss" { RunMode::Discuss } else { RunMode::Execute };
    let baseline = state.db_lock().ok()
        .and_then(|conn| session.as_deref().and_then(|id| runs::latest_usage(&conn, &agent.id, id).ok().flatten()))
        .and_then(|value| serde_json::from_value(value).ok());
    // 与直接点名路径一致：带上代理环境和 Dsh 设置。它自己会拿数据库锁，调用时不能持有锁。
    let adapter = match state.make_runtime_adapter(&cli, mode, baseline) {
        Ok(adapter) => adapter,
        Err(error) => return refuse(&error.to_string()),
    };
    let request = crate::adapters::TurnRequest { prompt: spec.prompt.clone(), session_id: session,
        cwd: folder.clone(), mode, model: agent.model.clone() };
    record_requested_model(&state, &spec.run_id, request.model.as_deref());
    let run_id = spec.run_id.clone();
    let agent_id = spec.agent_id.clone();
    let registered = match state.runtime_lock() {
        Ok(mut runtime) => {
            if runtime.current(project_id, hold) {
                runtime.team_started(project_id, RuntimeSnapshot { project_id: project_id.to_string(), run_id: run_id.clone(),
                    agent_id: agent_id.clone(), status: "thinking".into(), seq: 0, partial_text: String::new() });
                true
            } else { false }
        }
        Err(_) => false,
    };
    if !registered { return refuse("运行预留已失效"); }
    if let Ok(conn) = state.db_lock() {
        let _ = save_owner(&conn, &state.new_owner(&run_id, Some(project_id), None, &folder));
    }
    emit_events(app, vec![("agent:status", json!({"projectId": project_id, "runId": run_id, "agentId": agent_id, "status": "thinking"}))]);
    let mut shown: &'static str = "thinking";
    let outcome = crate::runner::run_turn_managed(adapter, request, cancellation, |event| {
        record_reported_model(&app.state::<AppState>(), &run_id, &event);
        let next = live_status(mode, &event, shown);
        if next == shown { return; }
        shown = next;
        if let Ok(mut runtime) = app.state::<AppState>().runtime_lock() { runtime.team_status(project_id, &run_id, next); }
        emit_events(app, vec![("agent:status", json!({"projectId": project_id, "runId": run_id, "agentId": agent_id, "status": next}))]);
    }, crate::runner::RunnerConfig::default(),
        Some(state.data_dir.join("runs").join(format!("{run_id}.log"))),
        |pid| app.state::<AppState>().mark_started_under(project_id, hold, &run_id, pid)).await;
    if let Ok(mut runtime) = state.runtime_lock() { runtime.team_finished(project_id, &run_id); }
    // 两把锁分开拿，顺序与别处一致（先运行状态，后数据库），不嵌套。
    if outcome.cleanup_complete {
        if let Ok(conn) = state.db_lock() { let _ = remove_owner(&conn, &run_id); }
    } else {
        let marked = state.db_lock().ok().and_then(|conn| {
            let mut owner = load_owners(&conn).ok()?.into_iter().find(|owner| owner.run_id == run_id)?;
            owner.cleanup_required = true;
            save_owner(&conn, &owner).ok()
        }).is_some();
        if marked {
            if let Ok(mut runtime) = state.runtime_lock() { runtime.block_directory(folder, &run_id); }
        }
    }
    StepResult::Outcome(outcome)
}

fn instruction_running(app: &tauri::AppHandle, instruction_id: &str) -> bool {
    app.state::<AppState>().db_lock().ok().and_then(|conn| {
        crate::store::instructions::get(&conn, instruction_id).ok()
    }).is_some_and(|item| item.status == "running")
}

fn max_message_id(conn: &Connection, project_id: &str) -> AppResult<i64> {
    Ok(conn.query_row("SELECT COALESCE(MAX(id), 0) FROM messages WHERE project_id = ?1", [project_id], |row| row.get(0))?)
}

fn emit_messages_after(app: &tauri::AppHandle, conn: &Connection, project_id: &str, spec: &crate::orchestrator::TurnSpec, floor: i64) {
    let ids = conn.prepare("SELECT id FROM messages WHERE project_id = ?1 AND id > ?2 ORDER BY id")
        .and_then(|mut statement| {
            let rows = statement.query_map(rusqlite::params![project_id, floor], |row| row.get::<_, i64>(0))?;
            rows.collect::<Result<Vec<_>, _>>()
        });
    let Ok(ids) = ids else { return };
    for id in ids {
        if let Ok(message) = messages::get(conn, id) {
            emit_events(app, vec![("message:new", json!({"projectId": project_id, "runId": spec.run_id, "message": message}))]);
        }
    }
}

fn wait_instruction(conn: &Connection, project_id: &str, reason: &str) -> AppResult<()> {
    let now = crate::store::now_ms();
    conn.execute(
        "UPDATE instructions SET status = 'waiting_user', wait_reason = ?1, updated_at = ?2 \
         WHERE project_id = ?3 AND status = 'running'",
        rusqlite::params![reason, now, project_id],
    )?;
    conn.execute("UPDATE runs SET status = 'cancelled', ended_at = ?1, error = ?2 WHERE instruction_id IN \
        (SELECT id FROM instructions WHERE project_id = ?3) AND status = 'running'",
        rusqlite::params![now, reason, project_id])?;
    conn.execute("UPDATE projects SET status = 'waiting_user', updated_at = ?1 WHERE id = ?2", rusqlite::params![now, project_id])?;
    Ok(())
}

#[tauri::command]
pub fn list_instructions(state: State<'_, AppState>, project_id: String) -> AppResult<Vec<crate::store::instructions::Instruction>> {
    let conn = state.db_lock()?;
    Ok(crate::store::instructions::active_instruction(&conn, &project_id)?.into_iter().collect())
}

#[tauri::command]
pub fn list_tasks(state: State<'_, AppState>, instruction_id: String) -> AppResult<Vec<crate::store::instructions::TaskRecord>> {
    let conn = state.db_lock()?;
    crate::store::instructions::list_tasks(&conn, &instruction_id)
}

#[tauri::command]
pub async fn resume_instruction(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    instruction_id: String,
    action: String,
    note: Option<String>,
) -> AppResult<crate::store::instructions::Instruction> {
    let instruction = {
        let conn = state.db_lock()?;
        crate::orchestrator::resume_instruction(&conn, &instruction_id, &action, note.as_deref())?
    };
    if instruction.status != "running" {
        return Ok(instruction);
    }
    let project_id = instruction.project_id.clone();
    let specs = {
        let conn = state.db_lock()?;
        match crate::orchestrator::advance(&conn, &project_id) {
            Ok(specs) => specs,
            Err(error) => {
                let _ = crate::orchestrator::stop_instruction(&conn, &project_id);
                return Err(error);
            }
        }
    };
    let worker_app = app.clone();
    tauri::async_runtime::spawn(async move { drive_team(worker_app, project_id, specs).await; });
    Ok(instruction)
}

#[tauri::command]
pub async fn stop_project(state: State<'_, AppState>, project_id: String) -> AppResult<()> {
    let run_id = { let mut runtime = state.runtime_lock()?;
        let id = runtime.runs.get(&project_id).map(|(id, _)| id.clone()); runtime.cancel(&project_id); id };
    {
        let conn = state.db_lock()?;
        crate::orchestrator::stop_instruction(&conn, &project_id)?;
    }
    if let Some(run_id) = run_id {
        let start = tokio::time::Instant::now();
        while state.runtime_lock()?.current(&project_id, &run_id) {
            if start.elapsed() > Duration::from_secs(8) { return Err(AppError::Invalid("停止仍在清理中，请检查运行状态".into())); }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn draft_rules(app: tauri::AppHandle, state: State<'_, AppState>, input: crate::assistant::DraftInput) -> AppResult<crate::assistant::DraftResponse> {
    crate::assistant::run_draft(app, &state, crate::assistant::DraftKind::Rules, input).await
}

#[tauri::command]
pub async fn draft_persona(app: tauri::AppHandle, state: State<'_, AppState>, input: crate::assistant::DraftInput) -> AppResult<crate::assistant::DraftResponse> {
    crate::assistant::run_draft(app, &state, crate::assistant::DraftKind::Persona, input).await
}

#[tauri::command]
pub async fn cancel_draft(state: State<'_, AppState>, draft_id: String) -> AppResult<()> {
    let run_id = state.close_draft(&draft_id)?;
    if let Some(run_id) = run_id {
        let key = format!("draft:{draft_id}");
        let start = tokio::time::Instant::now();
        while state.runtime_lock()?.current(&key, &run_id) {
            if start.elapsed() >= Duration::from_secs(8) { return Err(AppError::Invalid("助理仍在清理中，请检查残留进程".into())); }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
    Ok(())
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    fn setup() -> (AppState, tempfile::TempDir, String, String, crate::cli_registry::ResolvedCli) {
        let folder = tempfile::tempdir().unwrap(); let conn = crate::store::open_in_memory().unwrap();
        let first = projects::create(&conn, "甲", "", folder.path().to_str().unwrap()).unwrap();
        let second = projects::create(&conn, "乙", "", folder.path().to_str().unwrap()).unwrap();
        let input = NewAgent { name: "组长".into(), role: "开发".into(),
            persona: "## 定位\n开发\n## 职责\n测试\n## 边界\n不越界\n## 工作风格\n务实".into(), cli: "codex".into(), model: None, is_leader: true };
        agents::create(&conn, &first.id, &input).unwrap(); agents::create(&conn, &second.id, &input).unwrap();
        let cli = crate::cli_registry::ResolvedCli { info: CliInfo { id: "codex".into(), label: "Codex".into(), installed: true,
            supported: true, path: Some("/bin/test".into()), version: None }, program: "/bin/test".into(), path_env: "/bin".into(), environment_source: crate::cli_registry::EnvironmentSource::Process };
        (AppState::new(conn, folder.path().into()).unwrap(), folder, first.id, second.id, cli)
    }
    fn outcome(status: &str) -> crate::runner::RunOutcome {
        crate::runner::RunOutcome { status: status.into(), session_id: Some("thread".into()), final_text: "最终正文".into(),
            partial_text: "部分正文".into(), usage: None, exit_code: Some(0), error: None, tool_seen: false, cleanup_complete: true }
    }
    #[test]
    fn persisted_run_and_ownership_precede_start_and_failures_roll_back_reservation() {
        let (state, _folder, first, second, cli) = setup();
        assert!(state.prepare_send(&first, "@不存在 内容", &cli).is_err());
        assert_eq!(messages::list(&state.db_lock().unwrap(), &first, None, 10).unwrap().len(), 0);
        let started = state.prepare_send(&first, "执行任务", &cli).unwrap();
        let conn = state.db_lock().unwrap();
        assert_eq!(runs::get(&conn, &started.run_id).unwrap().status, "running");
        assert!(load_owners(&conn).unwrap().iter().any(|o| o.run_id == started.run_id && o.pid.is_none() && !o.cleanup_required));
        let history = messages::list(&conn, &first, None, 10).unwrap();
        assert_eq!(history.len(), 2); assert!(history[1].content.is_empty()); drop(conn);
        assert!(state.prepare_send(&first, "再次发送", &cli).is_err());
        assert!(state.prepare_send(&second, "同目录", &cli).is_err());
        assert!(!state.runtime_lock().unwrap().runs.contains_key(&second));
        state.mark_started(&first, &started.run_id, 123456).unwrap();
        let owner = load_owners(&state.db_lock().unwrap()).unwrap().pop().unwrap();
        assert_eq!(owner.pid, Some(123456)); assert!(!owner.nonce.is_empty());
        assert!(!serde_json::to_string(&owner).unwrap().contains("执行任务"));
        assert!(crate::store::instructions::active_instruction(&state.db_lock().unwrap(), &first).unwrap().is_none());
    }
    #[test]
    fn idle_mention_records_a_direct_instruction_and_completion_closes_it() {
        let (state, _folder, first, _second, cli) = setup();
        let started = state.prepare_send(&first, "@组长 改一句文案", &cli).unwrap();
        let conn = state.db_lock().unwrap();
        let instruction = crate::store::instructions::active_instruction(&conn, &first).unwrap().unwrap();
        assert_eq!(instruction.kind, "direct");
        assert_eq!(instruction.phase, "direct");
        assert_eq!(runs::get(&conn, &started.run_id).unwrap().status, "running");
        drop(conn);
        state.finish_project(&started, outcome("ok")).unwrap();
        let conn = state.db_lock().unwrap();
        assert_eq!(crate::store::instructions::get(&conn, &instruction.id).unwrap().status, "completed");
        assert_eq!(projects::get(&conn, &first).unwrap().status, "idle");
        assert!(crate::store::instructions::active_instruction(&conn, &first).unwrap().is_none());
    }
    fn run_models(state: &AppState, run_id: &str) -> (Option<String>, Option<String>) {
        state.db_lock().unwrap().query_row("SELECT requested_model, reported_model FROM runs WHERE id=?1", [run_id],
            |row| Ok((row.get(0)?, row.get(1)?))).unwrap()
    }
    #[test]
    fn direct_run_records_requested_model_at_start_and_reported_model_from_events() {
        let (state, _folder, first, _second, cli) = setup();
        // 没有指定模型、也没有 CLI 默认模型：请求值为空。
        let plain = state.prepare_send(&first, "无模型", &cli).unwrap();
        assert_eq!(run_models(&state, &plain.run_id), (None, None));
        state.finish_project(&plain, outcome("ok")).unwrap();

        let agent_id = agents::list(&state.db_lock().unwrap(), &first).unwrap().remove(0).id;
        state.db_lock().unwrap().execute("UPDATE agents SET model='成员模型' WHERE id=?1", [&agent_id]).unwrap();
        let started = state.prepare_send(&first, "有模型", &cli).unwrap();
        assert_eq!(run_models(&state, &started.run_id), (Some("成员模型".into()), None));
        state.apply_project_event(&started, AgentEvent::ModelReported { model: "实际模型".into() }).unwrap();
        assert_eq!(run_models(&state, &started.run_id), (Some("成员模型".into()), Some("实际模型".into())));
        state.finish_project(&started, outcome("ok")).unwrap();
        // 运行结束后迟到的事件不再改写。
        assert!(state.apply_project_event(&started, AgentEvent::ModelReported { model: "迟到".into() }).unwrap().is_empty());
        assert_eq!(run_models(&state, &started.run_id), (Some("成员模型".into()), Some("实际模型".into())));
    }
    #[test]
    fn team_turn_records_reported_model_on_its_run_and_ignores_other_events() {
        let (state, _folder, first, _second, cli) = setup();
        let started = state.prepare_send(&first, "团队回合", &cli).unwrap();
        record_reported_model(&state, &started.run_id, &AgentEvent::TextDelta { text: "正文".into() });
        record_reported_model(&state, &started.run_id, &AgentEvent::SessionStarted { session_id: "s".into() });
        assert_eq!(run_models(&state, &started.run_id), (None, None));
        record_reported_model(&state, &started.run_id, &AgentEvent::ModelReported { model: "claude-sonnet-5-5".into() });
        assert_eq!(run_models(&state, &started.run_id), (None, Some("claude-sonnet-5-5".into())));
        record_requested_model(&state, &started.run_id, Some("sonnet"));
        assert_eq!(run_models(&state, &started.run_id), (Some("sonnet".into()), Some("claude-sonnet-5-5".into())));
        record_requested_model(&state, &started.run_id, None);
        assert_eq!(run_models(&state, &started.run_id), (None, Some("claude-sonnet-5-5".into())));
    }
    #[test]
    fn launched_existing_message_records_requested_model() {
        let (state, _folder, first, _second, cli) = setup();
        let user = {
            let conn = state.db_lock().unwrap();
            let agent_id = agents::list(&conn, &first).unwrap().remove(0).id;
            conn.execute("UPDATE agents SET model='成员模型' WHERE id=?1", [&agent_id]).unwrap();
            messages::insert(&conn, &first, "user", None, "chat", "@组长 已落库的消息").unwrap()
        };
        let started = state.launch_existing_message(&first, user.id, &cli).unwrap();
        assert_eq!(run_models(&state, &started.run_id), (Some("成员模型".into()), None));
    }
    #[test]
    #[cfg(unix)]
    fn symlink_directory_alias_cannot_execute_concurrently() {
        let (state, folder, first, second, cli) = setup();
        let alias_root = tempfile::tempdir().unwrap(); let alias = alias_root.path().join("别名");
        std::os::unix::fs::symlink(folder.path(), &alias).unwrap();
        projects::update(&state.db_lock().unwrap(), &second, "乙", "", alias.to_str().unwrap()).unwrap();
        state.prepare_send(&first, "开始", &cli).unwrap();
        assert!(state.prepare_send(&second, "别名启动", &cli).is_err());
    }
    #[test]
    fn same_cli_session_is_exclusive_even_across_different_directories() {
        let (state, _folder, first, second, cli) = setup(); let other = tempfile::tempdir().unwrap();
        {
            let conn = state.db_lock().unwrap(); projects::update(&conn, &second, "乙", "", other.path().to_str().unwrap()).unwrap();
            for pid in [&first, &second] { let agent = agents::list(&conn, pid).unwrap().remove(0); agents::set_session(&conn, &agent.id, Some("same-thread")).unwrap(); }
        }
        state.prepare_send(&first, "开始", &cli).unwrap();
        assert!(state.prepare_send(&second, "重复续接", &cli).is_err());
    }
    #[test]
    fn cancellation_generation_rejects_late_deltas_and_completion_cannot_revive() {
        let (state, _folder, first, _second, cli) = setup(); let started = state.prepare_send(&first, "开始", &cli).unwrap();
        let events = state.apply_project_event(&started, AgentEvent::TextDelta { text: "部分".into() }).unwrap();
        assert_eq!(events[0].1["seq"], 1); assert_eq!(events[0].1["projectId"], first);
        state.runtime_lock().unwrap().cancel(&first); state.runtime_lock().unwrap().cancel(&first);
        assert_eq!(state.runtime_lock().unwrap().generations[&started.run_id], 1);
        assert!(state.apply_project_event(&started, AgentEvent::TextDelta { text: "迟到".into() }).unwrap().is_empty());
        let mut out = outcome("ok"); out.partial_text = "部分".into();
        let finished = state.finish_project(&started, out).unwrap();
        let conn = state.db_lock().unwrap();
        assert_eq!(runs::get(&conn, &started.run_id).unwrap().status, "cancelled");
        assert_eq!(messages::get(&conn, started.agent_message_id).unwrap().content, "部分");
        assert!(load_owners(&conn).unwrap().is_empty()); drop(conn);
        assert_eq!(finished.last().unwrap().1["status"], "cancelled");
        assert!(state.finish_project(&started, outcome("ok")).unwrap().is_empty());
        assert!(state.runtime_snapshot(&first).unwrap().active.is_none());
    }
    #[test]
    fn final_message_replaces_partial_and_old_run_cannot_remove_new() {
        let (state, _folder, first, _second, cli) = setup(); let started = state.prepare_send(&first, "开始", &cli).unwrap();
        state.apply_project_event(&started, AgentEvent::SessionStarted { session_id: "session-1".into() }).unwrap();
        state.apply_project_event(&started, AgentEvent::TextDelta { text: "临时过程".into() }).unwrap();
        state.finish_project(&started, outcome("ok")).unwrap();
        {
            let conn = state.db_lock().unwrap();
            assert_eq!(messages::get(&conn, started.agent_message_id).unwrap().content, "最终正文");
            let agent = agents::get(&conn, &started.prepared.agent.id).unwrap();
            assert_eq!(agent.session_id.as_deref(), Some("session-1"));
            assert_eq!(agent.last_seen_message_id, Some(started.user_message.id));
        }
        let new = state.prepare_send(&first, "下一回合", &cli).unwrap();
        assert!(state.finish_project(&started, outcome("failed")).unwrap().is_empty());
        assert_eq!(state.runtime_snapshot(&first).unwrap().active.unwrap().run_id, new.run_id);
    }
    #[test]
    fn recover_fails_in_progress_tasks_and_does_not_rewrite_sent_prompts() {
        let (state, _folder, first, second, cli) = setup();
        let started = state.prepare_send(&first, "开始执行", &cli).unwrap();
        state.mark_started(&first, &started.run_id, 424242).unwrap();
        let (instruction_id, running_task, pending_task, done_task, queued_id, done_input, prompt) = {
            let conn = state.db_lock().unwrap();
            let prompt = runs::get(&conn, &started.run_id).unwrap().command;
            let agent = agents::list(&conn, &first).unwrap().remove(0);
            let source = messages::insert(&conn, &first, "user", None, "instruction", "做首页").unwrap();
            let instruction = crate::store::instructions::insert_instruction(&conn, &first, source.id, "team", None, "execute").unwrap();
            let batch = crate::store::instructions::insert_batch(
                &conn,
                &instruction.id,
                Some(&agent.id),
                "先做三步",
                &[
                    crate::store::instructions::NewTask { assignee_agent_id: agent.id.clone(), title: "正在写".into(), detail: "执行中".into(), replaces_task_id: None },
                    crate::store::instructions::NewTask { assignee_agent_id: agent.id.clone(), title: "还没开始".into(), detail: "未启动".into(), replaces_task_id: None },
                    crate::store::instructions::NewTask { assignee_agent_id: agent.id.clone(), title: "已经做完".into(), detail: "保持完成".into(), replaces_task_id: None },
                ],
            ).unwrap();
            conn.execute("UPDATE tasks SET status = 'running' WHERE id = ?1", [&batch.tasks[0].id]).unwrap();
            conn.execute("UPDATE tasks SET status = 'done', finished_at = 1 WHERE id = ?1", [&batch.tasks[2].id]).unwrap();
            conn.execute("UPDATE instructions SET turn_count = 4 WHERE id = ?1", [&instruction.id]).unwrap();
            let queued_message = messages::insert(&conn, &first, "user", None, "chat", "排队的补充").unwrap();
            let queued = crate::store::instructions::enqueue_input(&conn, &instruction.id, queued_message.id, "supplement", None).unwrap();
            let done_message = messages::insert(&conn, &first, "user", None, "chat", "已经入队并处理").unwrap();
            let done_input = crate::store::instructions::enqueue_input(&conn, &instruction.id, done_message.id, "supplement", None).unwrap();
            conn.execute("UPDATE instruction_inputs SET status = 'done' WHERE id = ?1", [&done_input.id]).unwrap();
            (instruction.id, batch.tasks[0].id.clone(), batch.tasks[1].id.clone(), batch.tasks[2].id.clone(), queued.id, done_input.id, prompt)
        };
        state.recover().unwrap();
        let conn = state.db_lock().unwrap();
        assert_eq!(crate::store::instructions::get_task(&conn, &running_task).unwrap().status, "failed");
        assert_eq!(crate::store::instructions::get_task(&conn, &pending_task).unwrap().status, "cancelled");
        assert_eq!(crate::store::instructions::get_task(&conn, &done_task).unwrap().status, "done");
        assert_eq!(crate::store::instructions::list_inputs(&conn, &instruction_id).unwrap().iter().find(|input| input.id == queued_id).unwrap().status, "cancelled");
        assert_eq!(crate::store::instructions::list_inputs(&conn, &instruction_id).unwrap().iter().find(|input| input.id == done_input).unwrap().status, "done");
        let instruction = crate::store::instructions::get(&conn, &instruction_id).unwrap();
        assert_eq!(instruction.status, "waiting_user");
        assert_eq!(instruction.phase, "execute");
        assert_eq!(instruction.turn_count, 4);
        assert!(instruction.wait_reason.unwrap_or_default().contains("未自动重跑"));
        let run = runs::get(&conn, &started.run_id).unwrap();
        assert_eq!(run.status, "interrupted");
        assert_eq!(run.command, prompt);
        assert_eq!(projects::get(&conn, &first).unwrap().status, "waiting_user");
        let owner = load_owners(&conn).unwrap().into_iter().find(|owner| owner.run_id == started.run_id).unwrap();
        assert_eq!(owner.pid, Some(424242));
        assert!(owner.cleanup_required);
        drop(conn);
        state.runtime_lock().unwrap().runs.clear();
        state.runtime_lock().unwrap().snapshots.clear();
        assert!(state.prepare_send(&second, "必须阻塞", &cli).is_err());
    }

    #[test]
    fn rule_changes_append_a_folded_notice_without_rewriting_sent_prompts() {
        let (state, _folder, first, _second, _cli) = setup();
        let (run_id, sent_id) = {
            let conn = state.db_lock().unwrap();
            let agent = agents::list(&conn, &first).unwrap().remove(0);
            let run = runs::insert_running(&conn, Some(&agent.id), "execute", "已发出的提示词标记", None).unwrap();
            let sent = messages::insert(&conn, &first, "agent", Some(&agent.id), "chat", "已发出的提示词标记").unwrap();
            (run.id, sent.id)
        };
        let added = "# 协作\n这是新增全文";
        state.save_rule_for_project(&first, "协作规范.md", added).unwrap();
        state.save_rule_for_project(&first, "协作规范.md", "# 协作\n这是替换全文").unwrap();
        state.save_rule_for_project(&first, "开发规范.md", "开发正文").unwrap();
        state.delete_rule_for_project(&first, "开发规范.md").unwrap();
        state.delete_rule_for_project(&first, "协作规范.md").unwrap();
        assert!(state.save_rule_for_project(&first, "../逃逸.md", "x").is_err());
        let conn = state.db_lock().unwrap();
        assert_eq!(runs::get(&conn, &run_id).unwrap().command, "已发出的提示词标记");
        assert_eq!(runs::get(&conn, &run_id).unwrap().status, "running");
        assert_eq!(messages::get(&conn, sent_id).unwrap().content, "已发出的提示词标记");
        let notices: Vec<_> = messages::list(&conn, &first, None, 20).unwrap().into_iter().filter(|message| message.kind == "system" && message.sender_type == "system").collect();
        assert_eq!(notices.len(), 5);
        assert!(notices[0].content.contains("操作：新增") && notices[0].content.contains(added) && notices[0].content.contains("失效"));
        assert!(notices[1].content.contains("操作：替换") && notices[1].content.contains("这是替换全文"));
        assert!(notices[2].content.contains("操作：新增") && notices[2].content.contains("开发规范.md") && notices[2].content.contains("协作规范.md"));
        assert!(notices[3].content.contains("操作：删除") && notices[3].content.contains("协作规范.md") && !notices[3].content.contains("开发正文"));
        assert!(notices[4].content.contains("旧规章全部失效，当前暂无规章"), "{}", notices[4].content);
        assert!(notices[4].content.contains("规章版本：none"), "{}", notices[4].content);
        let replaced_version = format!("规章版本：协作规范.md:{}", "# 协作\n这是替换全文".len());
        assert!(notices[1].content.contains(&replaced_version), "{}", notices[1].content);
    }

    #[test]
    fn restart_and_uncertain_cleanup_block_directory_until_explicit_confirmation() {
        let (state, _folder, first, second, cli) = setup(); let started = state.prepare_send(&first, "开始", &cli).unwrap();
        state.recover().unwrap();
        state.runtime_lock().unwrap().runs.clear(); state.runtime_lock().unwrap().snapshots.clear();
        assert_eq!(runs::get(&state.db_lock().unwrap(), &started.run_id).unwrap().status, "interrupted");
        assert_eq!(projects::get(&state.db_lock().unwrap(), &first).unwrap().status, "waiting_user");
        assert_eq!(state.runtime_snapshot(&second).unwrap().cleanup_required.len(), 1);
        assert!(state.prepare_send(&second, "必须阻塞", &cli).is_err());
        state.confirm_cleanup(&started.run_id).unwrap();
        assert!(state.runtime_snapshot(&second).unwrap().cleanup_required.is_empty());
        let next = state.prepare_send(&second, "已人工处理", &cli).unwrap();
        assert!(state.confirm_cleanup(&next.run_id).is_err());
        let mut out = outcome("failed"); out.cleanup_complete = false;
        state.finish_project(&next, out).unwrap();
        assert_eq!(runs::get(&state.db_lock().unwrap(), &next.run_id).unwrap().status, "interrupted");
        assert_eq!(state.runtime_snapshot(&first).unwrap().cleanup_required.len(), 1);
    }
    #[test]
    fn shutdown_cancels_every_owner_and_rejects_new_reservations() {
        let mut runtime = ActiveRuns::default(); let project = runtime.reserve("project", "run-1").unwrap();
        let draft = runtime.reserve("draft:test", "run-2").unwrap();
        runtime.cancel_all(); assert!(*project.borrow() && *draft.borrow());
        assert!(runtime.reserve("new", "run-3").is_err());
    }

    #[test]
    #[ignore = "仅供隔离子进程调用"]
    fn crash_helper() {
        let db = std::env::var("IMATES_CRASH_DB").unwrap();
        let folder = std::fs::canonicalize(std::env::var("IMATES_CRASH_DIR").unwrap()).unwrap();
        let conn = crate::store::open(std::path::Path::new(&db)).unwrap();
        let project = projects::create(&conn, "甲", "", folder.to_str().unwrap()).unwrap();
        let input = NewAgent { name: "组长".into(), role: "开发".into(),
            persona: "## 定位\n开发\n## 职责\n测试\n## 边界\n不越界\n## 工作风格\n务实".into(),
            cli: "codex".into(), model: None, is_leader: true };
        let agent = agents::create(&conn, &project.id, &input).unwrap();
        let run = runs::insert_running(&conn, Some(&agent.id), "execute", "probe", None).unwrap();
        let mut sleeper = {
            #[cfg(windows)]
            { let mut command = std::process::Command::new("cmd.exe"); command.args(["/D", "/C", "ping -n 30 127.0.0.1 >NUL"]); command }
            #[cfg(unix)]
            { let mut command = std::process::Command::new("sleep"); command.arg("30"); command }
        };
        sleeper.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        #[cfg(windows)]
        { use std::os::windows::process::CommandExt; sleeper.creation_flags(0x0800_0000); }
        let sleeper = sleeper.spawn().unwrap();
        let owner = RuntimeOwner { run_id: run.id.clone(), project_id: Some(project.id.clone()), draft_id: None,
            folder_path: folder.to_string_lossy().into_owned(), pid: Some(sleeper.id()), process_group: Some(sleeper.id()),
            nonce: "nonce".into(), instance: "crash-test".into(), started_at: 1, cleanup_required: false };
        save_owner(&conn, &owner).unwrap();
        drop(conn);
        std::fs::write(folder.join("ready"), format!("{}\n{}\n{}", run.id, project.id, sleeper.id())).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(40));
    }

    #[test]
    fn killed_process_reopen_blocks_directory_until_confirm() {
        let root = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let db = root.path().join("imates.db");
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        child.args(["--ignored", "--exact", "commands::integration_tests::crash_helper", "--nocapture"]);
        child.env("IMATES_CRASH_DB", &db).env("IMATES_CRASH_DIR", folder.path());
        child.stdin(std::process::Stdio::null());
        let mut child = child.spawn().unwrap();
        let ready = folder.path().join("ready");
        let started = std::time::Instant::now();
        while !ready.exists() && started.elapsed() < std::time::Duration::from_secs(15) {
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        assert!(ready.exists(), "崩溃辅助进程没有写好数据库");
        let text = std::fs::read_to_string(&ready).unwrap();
        let mut lines = text.lines();
        let run_id = lines.next().unwrap().to_string();
        let project_id = lines.next().unwrap().to_string();
        let sleeper_pid: u32 = lines.next().unwrap().trim().parse().unwrap();
        kill_pid(child.id(), false);
        let _ = child.wait();
        let cli = crate::cli_registry::ResolvedCli { info: CliInfo { id: "codex".into(), label: "Codex".into(), installed: true,
            supported: true, path: Some("/bin/test".into()), version: None }, program: "/bin/test".into(), path_env: "/bin".into(),
            environment_source: crate::cli_registry::EnvironmentSource::Process };
        let state = AppState::new(crate::store::open(&db).unwrap(), root.path().into()).unwrap();
        assert_eq!(runs::get(&state.db_lock().unwrap(), &run_id).unwrap().status, "interrupted");
        assert_eq!(projects::get(&state.db_lock().unwrap(), &project_id).unwrap().status, "waiting_user");
        assert!(state.runtime_snapshot(&project_id).unwrap().active.is_none());
        assert_eq!(state.runtime_snapshot(&project_id).unwrap().cleanup_required.len(), 1);
        assert!(state.prepare_send(&project_id, "必须阻塞", &cli).is_err());
        let sleeper_alive = process_alive(sleeper_pid);
        state.confirm_cleanup(&run_id).unwrap();
        assert!(state.runtime_snapshot(&project_id).unwrap().cleanup_required.is_empty());
        assert!(state.prepare_send(&project_id, "已人工处理", &cli).is_ok());
        assert!(sleeper_alive, "数据库标成 interrupted 时，残留子进程应仍在；测试环境若用作业对象连带结束它，这条会失败");
        assert!(process_alive(sleeper_pid), "确认清理不应把残留进程杀掉");
        kill_pid(sleeper_pid, true);
    }

    fn process_alive(pid: u32) -> bool {
        #[cfg(windows)]
        {
            let output = std::process::Command::new("tasklist").args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"]).output();
            let Ok(output) = output else { return false };
            let text = String::from_utf8_lossy(&output.stdout);
            return text.split(',').any(|field| field.trim().trim_matches('"') == pid.to_string());
        }
        #[cfg(unix)]
        {
            std::process::Command::new("/bin/ps").args(["-p", &pid.to_string()]).output().is_ok_and(|output| output.status.success())
        }
    }
    fn kill_pid(pid: u32, tree: bool) {
        #[cfg(windows)]
        {
            let mut args = vec!["/PID".to_string(), pid.to_string(), "/F".into()];
            if tree { args.push("/T".into()); }
            let _ = std::process::Command::new("taskkill").args(&args).stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
        }
        #[cfg(unix)]
        {
            let _ = tree;
            unsafe { libc::kill(pid as i32, libc::SIGKILL); }
        }
    }
    #[test]
    fn proxy_runtime_settings_apply_to_all_clis_and_only_future_adapters() {
        let folder = tempfile::tempdir().unwrap();
        let state = AppState::new(crate::store::open_in_memory().unwrap(), folder.path().into()).unwrap();
        crate::proxy::save(&state.db_lock().unwrap(), &crate::proxy::ProxySettings {
            mode: crate::proxy::ProxyMode::Manual, url: "http://127.0.0.1:7890".into(),
        }).unwrap();
        for id in ["claude", "codex", "grok", "dsh"] {
            let cli = crate::cli_registry::ResolvedCli {
                info: crate::cli_registry::CliInfo { id: id.into(), label: id.into(), installed: true,
                    supported: true, path: Some("/bin/test".into()), version: None },
                program: "/bin/test".into(), path_env: "/bin".into(),
                environment_source: crate::cli_registry::EnvironmentSource::Process,
            };
            for mode in [RunMode::Discuss, RunMode::Execute] {
                let adapter = state.make_runtime_adapter(&cli, mode, None).unwrap();
                let environment = adapter.environment();
                for key in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] {
                    assert!(environment.contains(&(key.into(), Some("http://127.0.0.1:7890".into()))), "{id}: {key}");
                }
                crate::proxy::save(&state.db_lock().unwrap(), &crate::proxy::ProxySettings {
                    mode: crate::proxy::ProxyMode::Direct, url: String::new(),
                }).unwrap();
                assert_eq!(adapter.environment(), environment);
                let direct = state.make_runtime_adapter(&cli, mode, None).unwrap().environment();
                for key in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] {
                    assert!(direct.contains(&(key.into(), None)), "{id}: {key}");
                }
                crate::proxy::save(&state.db_lock().unwrap(), &crate::proxy::ProxySettings {
                    mode: crate::proxy::ProxyMode::Manual, url: "http://127.0.0.1:7890".into(),
                }).unwrap();
            }
        }
    }
    #[test]
    fn dsh_runtime_settings_are_shared_but_existing_adapters_keep_their_snapshot() {
        use crate::dsh_config::{self, Authentication, DshSettings, Profile};
        let folder = tempfile::tempdir().unwrap();
        let state = AppState::new(crate::store::open_in_memory().unwrap(), folder.path().into()).unwrap();
        let cli = crate::cli_registry::ResolvedCli {
            info: crate::cli_registry::CliInfo { id: "dsh".into(), label: "Dsh".into(), installed: true,
                supported: true, path: Some("/bin/test".into()), version: None },
            program: "/bin/test".into(), path_env: "/bin".into(),
            environment_source: crate::cli_registry::EnvironmentSource::Process,
        };
        for mode in [RunMode::Discuss, RunMode::Execute] {
            dsh_config::save(&state.db_lock().unwrap(), &DshSettings::default()).unwrap();
            let request = crate::adapters::TurnRequest { prompt: "测试".into(), session_id: None,
                cwd: folder.path().into(), mode, model: None };
            let legacy = state.make_runtime_adapter(&cli, mode, None).unwrap();
            dsh_config::save(&state.db_lock().unwrap(), &DshSettings { authentication: Authentication::Account, profile: Profile::Web }).unwrap();
            let account = state.make_runtime_adapter(&cli, mode, None).unwrap();
            assert!(legacy.build_command(&request).support_files.is_empty());
            let spec = account.build_command(&request);
            assert_eq!(spec.support_files.len(), 2);
            let patch: Value = serde_json::from_str(&spec.support_files[1].content).unwrap();
            assert!(patch.as_array().unwrap().iter().any(|row| row["id"] == "agent-default-model" && row["config"]["provider"] == "deepseek-account"));
            dsh_config::save(&state.db_lock().unwrap(), &DshSettings::default()).unwrap();
            assert_eq!(account.build_command(&request).support_files.len(), 2);
            assert!(state.make_runtime_adapter(&cli, mode, None).unwrap().build_command(&request).support_files.is_empty());
        }
    }

    #[test]
    fn make_adapter_covers_every_supported_cli_and_rejects_pi() {
        use crate::adapters::RunMode;
        let resolved = |id: &str| crate::cli_registry::ResolvedCli {
            info: crate::cli_registry::CliInfo { id: id.into(), label: id.into(), installed: true,
                supported: crate::cli_registry::is_supported(id), path: Some("/bin/test".into()), version: None },
            program: "/bin/test".into(), path_env: "/bin".into(),
            environment_source: crate::cli_registry::EnvironmentSource::Process,
        };
        for id in ["claude", "codex", "grok", "dsh"] {
            let adapter = super::make_adapter(&resolved(id), RunMode::Discuss, None).unwrap();
            assert_eq!(adapter.id(), id);
        }
        assert!(super::make_adapter(&resolved("pi"), RunMode::Discuss, None).is_err());
    }

    #[test]
    fn external_rule_edits_write_one_notice_and_app_saves_are_not_repeated() {
        let (state, folder, first, _second, _cli) = setup();
        let dir = rules::rules_dir(folder.path());
        assert!(state.sync_rules(&first).unwrap().is_none(), "首次只记录基线");
        let saved = state.save_rule_for_project(&first, "协作规范.md", "甲").unwrap();
        assert!(saved.content.contains("操作：新增"));
        assert!(state.sync_rules(&first).unwrap().is_none(), "应用内保存已经通知过，不重复");

        std::fs::write(dir.join("协作规范.md"), "乙").unwrap();
        std::fs::write(dir.join("开发规范.md"), "开发").unwrap();
        let (message, version) = state.sync_rules(&first).unwrap().unwrap();
        assert_eq!(message.sender_type, "system");
        assert_eq!(message.kind, "system");
        assert!(message.content.contains("操作：外部修改"));
        assert!(message.content.contains("替换：协作规范.md"));
        assert!(message.content.contains("新增：开发规范.md"));
        assert!(message.content.contains("协作规范.md 全文：\n乙"));
        assert_eq!(version, state.rules_version(&first).unwrap());
        assert!(state.sync_rules(&first).unwrap().is_none(), "同一变化只通知一次");

        std::fs::remove_file(dir.join("协作规范.md")).unwrap();
        std::fs::remove_file(dir.join("开发规范.md")).unwrap();
        let (cleared, version) = state.sync_rules(&first).unwrap().unwrap();
        assert!(cleared.content.contains("旧规章全部失效，当前暂无规章"));
        assert_eq!(version, "none");

        let deleted = {
            state.save_rule_for_project(&first, "写作规范.md", "写作").unwrap();
            state.delete_rule_for_project(&first, "写作规范.md").unwrap()
        };
        assert!(deleted.content.contains("操作：删除"));
        assert!(state.sync_rules(&first).unwrap().is_none());
    }

    #[test]
    fn missing_project_folder_is_not_reported_as_cleared_rules() {
        let (state, folder, first, _second, _cli) = setup();
        state.save_rule_for_project(&first, "协作规范.md", "甲").unwrap();
        let gone = folder.path().join("已移走");
        projects::update(&state.db_lock().unwrap(), &first, "甲", "", gone.to_str().unwrap()).unwrap();
        assert!(state.sync_rules(&first).unwrap().is_none());
    }
}
