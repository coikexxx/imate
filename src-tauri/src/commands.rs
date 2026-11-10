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
    if project.folder_path != folder_path { runtime.ensure_project_idle(&id)?; }
    projects::update(&conn, &id, &name, &description, &folder_path)
}

#[tauri::command]
pub fn delete_project(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let runtime = state.runtime_lock()?;
    runtime.ensure_project_idle(&id)?;
    let conn = lock(&state)?;
    projects::delete(&conn, &id)
}

#[tauri::command]
pub fn create_agent(
    state: State<'_, AppState>,
    project_id: String,
    input: NewAgent,
) -> AppResult<Agent> {
    let conn = lock(&state)?;
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

#[tauri::command]
pub fn save_rule(
    state: State<'_, AppState>,
    project_id: String,
    filename: String,
    content: String,
) -> AppResult<()> {
    let folder = project_folder(&state, &project_id)?;
    rules::save(Path::new(&folder), &filename, &content)
}

#[tauri::command]
pub fn delete_rule(
    state: State<'_, AppState>,
    project_id: String,
    filename: String,
) -> AppResult<()> {
    let folder = project_folder(&state, &project_id)?;
    rules::delete(Path::new(&folder), &filename)
}

#[tauri::command]
pub fn list_rule_templates() -> Vec<RuleDoc> {
    templates::rule_templates()
}

#[tauri::command]
pub fn list_persona_templates() -> Vec<PersonaTemplate> {
    templates::persona_templates()
}

#[cfg(test)]
mod runtime_tests {
    use super::ActiveRuns;

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

#[derive(Default)]
pub struct ActiveRuns {
    runs: HashMap<String, (String, watch::Sender<bool>)>,
    directories: HashMap<PathBuf, String>,
    sessions: HashMap<String, String>,
    snapshots: HashMap<String, RuntimeSnapshot>,
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
            draft_sessions: Mutex::new(crate::assistant::DraftSessions::default()), data_dir, instance: crate::store::new_id() };
        state.recover()?; Ok(state)
    }
    pub fn db_lock(&self) -> AppResult<MutexGuard<'_, Connection>> {
        self.db.lock().map_err(|_| AppError::Invalid("数据库连接不可用，请重启应用".into()))
    }
    pub fn runtime_lock(&self) -> AppResult<MutexGuard<'_, ActiveRuns>> {
        self.runtime.lock().map_err(|_| AppError::Invalid("运行状态不可用，请重启应用".into()))
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
        tx.commit()?; Ok(())
    }
    pub fn new_owner(&self, run_id: &str, project_id: Option<&str>, draft_id: Option<&str>, folder: &Path) -> RuntimeOwner {
        RuntimeOwner { run_id: run_id.into(), project_id: project_id.map(str::to_owned), draft_id: draft_id.map(str::to_owned),
            folder_path: folder.to_string_lossy().into_owned(), pid: None, process_group: None,
            nonce: crate::store::new_id(), instance: self.instance.clone(), started_at: crate::store::now_ms(), cleanup_required: false }
    }
    pub fn mark_started(&self, key: &str, run_id: &str, pid: u32) -> Result<(), String> {
        let runtime = self.runtime_lock().map_err(|e| e.to_string())?;
        if !runtime.current(key, run_id) { return Err("运行预留已失效".into()); }
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
        if let Err(error) = runtime.reserve_directory(&folder, &reservation)
            .and_then(|_| runtime.reserve_session(&agent.cli, agent.session_id.as_deref(), &reservation)) {
            runtime.remove(project_id, &reservation); return Err(error);
        }
        let prepared = (|| {
            let tx = conn.unchecked_transaction()?;
            let user = messages::insert(&tx, project_id, "user", None, "chat", content.trim())?;
            let prepared = ChatService::send(&tx, project_id, user.id, false)?;
            let request = ChatService::request(&prepared, folder.clone());
            let baseline = prepared.agent.session_id.as_deref().map(|id| runs::latest_usage(&tx, &prepared.agent.id, id)).transpose()?.flatten()
                .and_then(|value| serde_json::from_value(value).ok());
            let adapter = make_adapter(cli, RunMode::Execute, baseline.clone())?;
            let spec = adapter.build_command(&request);
            let command = serde_json::to_string(&json!({"program":spec.program, "args":spec.args}))
                .map_err(|e| AppError::Invalid(e.to_string()))?;
            let run = runs::insert_running(&tx, Some(&prepared.agent.id), "execute", &command, request.session_id.as_deref())?;
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
        Ok(StartedMessage { prepared, request, run_id, user_message, agent_message_id, cancellation, baseline })
    }
    pub fn runtime_snapshot(&self, project_id: &str) -> AppResult<ProjectRuntime> {
        let runtime = self.runtime_lock()?; let conn = self.db_lock()?;
        let project = projects::get(&conn, project_id)?;
        let folder = Path::new(&project.folder_path).canonicalize().unwrap_or_else(|_| project.folder_path.into());
        let cleanup_required = load_owners(&conn)?.into_iter().filter(|owner| owner.cleanup_required &&
            (owner.project_id.as_deref() == Some(project_id) || Path::new(&owner.folder_path) == folder))
            .map(|owner| CleanupNotice { run_id: owner.run_id, project_id: owner.project_id, folder_path: owner.folder_path }).collect();
        Ok(ProjectRuntime { active: runtime.snapshots.get(project_id).cloned(), cleanup_required })
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
        id => Err(AppError::Invalid(format!("CLI {id} 尚未接入，计划在 M5 实现"))),
    }
}

impl AppState {
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
            "seq":seq,"status":"failed","error":"旧会话不可用，已重建一次"})),
            ("agent:status", json!({"projectId":project_id,"runId":run.id,"agentId":prepared.agent.id,"status":"thinking"}))];
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
    let cli_id = {
        let conn = state.db_lock()?;
        let roster = agents::list(&conn, &project_id)?;
        crate::chat::resolve_recipient(&content, &roster)?.0.cli.clone()
    };
    let cli = tauri::async_runtime::spawn_blocking(move || cli_registry::resolve_cli(&cli_id)).await
        .map_err(|e| AppError::Invalid(format!("CLI 探测失败：{e}")))??;
    let started = state.prepare_send(&project_id, &content, &cli)?;
    let response = SendMessageResponse { run_id: started.run_id.clone(), user_message: started.user_message.clone(), agent_message_id: started.agent_message_id };
    emit_events(&app, vec![("message:new", json!({"projectId":project_id,"runId":started.run_id,"message":started.user_message})),
        ("agent:status", json!({"projectId":project_id,"runId":started.run_id,"agentId":started.prepared.agent.id,"status":"thinking"}))]);
    let rescue_project = project_id; let rescue_id = started.run_id.clone();
    let worker_app = app.clone();
    let worker = tauri::async_runtime::spawn(async move { execute_project(worker_app, started, cli).await; });
    tauri::async_runtime::spawn(async move {
        if worker.await.is_err() {
            // panic 后保留进程所有权，不能伪称已经完成树清理。
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
        let adapter = match make_adapter(&cli, RunMode::Execute, started.baseline.clone()) {
            Ok(adapter) => adapter, Err(_) => return,
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
}

#[tauri::command]
pub async fn stop_project(state: State<'_, AppState>, project_id: String) -> AppResult<()> {
    let run_id = { let mut runtime = state.runtime_lock()?;
        let id = runtime.runs.get(&project_id).map(|(id, _)| id.clone()); runtime.cancel(&project_id); id };
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
}
