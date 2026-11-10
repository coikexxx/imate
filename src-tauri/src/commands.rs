use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::Connection;
use serde::Serialize;
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
    let conn = lock(&state)?;
    projects::update(&conn, &id, &name, &description, &folder_path)
}

#[tauri::command]
pub fn delete_project(state: State<'_, AppState>, id: String) -> AppResult<()> {
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
    let conn = lock(&state)?;
    let tx = conn.unchecked_transaction()?;
    let agent = agents::update(&tx, &id, &input)?;
    tx.commit()?;
    Ok(agent)
}

#[tauri::command]
pub fn delete_agent(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let conn = lock(&state)?;
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
