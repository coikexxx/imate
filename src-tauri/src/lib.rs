mod chat;
mod prompts;
mod runner;
mod cli_registry;
mod commands;
mod error;
mod adapters;
mod persona;
mod project_setup;
mod rules;
mod store;
mod templates;

use std::sync::Mutex;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // 数据库放在系统的应用数据目录，与项目文件夹无关
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let conn = store::open(&data_dir.join("imates.db"))?;
            app.manage(commands::AppState {
                db: Mutex::new(conn),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_clis,
            commands::list_projects,
            commands::create_project,
            commands::get_project,
            commands::update_project,
            commands::delete_project,
            commands::create_agent,
            commands::update_agent,
            commands::delete_agent,
            commands::list_rules,
            commands::save_rule,
            commands::delete_rule,
            commands::list_rule_templates,
            commands::list_persona_templates,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
