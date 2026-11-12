mod fs_watch;
mod files;
mod proxy;
mod dsh_config;
mod chat;
mod orchestrator;
mod assistant;
mod prompts;
mod runner;
mod cli_registry;
mod cli_config;
mod provider_models;
mod commands;
mod error;
mod adapters;
mod persona;
mod project_setup;
mod rules;
mod store;
mod templates;


use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let application = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // 数据库放在系统的应用数据目录，与项目文件夹无关
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let conn = store::open(&data_dir.join("imates.db"))?;
            cli_config::load_path_overrides(&conn);
            app.manage(commands::AppState::new(conn, data_dir)?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_network_proxy,
            commands::save_network_proxy,
            commands::get_dsh_settings,
            commands::save_dsh_settings,
            commands::list_dsh_models,
            commands::draft_rules,
            commands::draft_persona,
            commands::cancel_draft,
            commands::list_messages,
            commands::send_message,
            commands::stop_project,
            commands::list_instructions,
            commands::resume_instruction,
            commands::list_tasks,
            commands::get_project_runtime,
            commands::confirm_runtime_cleanup,
            commands::list_clis,
            cli_config::list_cli_configs,
            cli_config::save_cli_config,
            cli_config::list_cli_models,
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
            commands::list_files,
            commands::open_file,
            commands::watch_project,
        ])
        .build(tauri::generate_context!())
        .expect("无法构建 imates 应用");
    let exiting = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    application.run(move |app, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            use std::sync::atomic::Ordering;
            if !exiting.swap(true, Ordering::SeqCst) {
                api.prevent_exit();
                let handle = app.clone();
                if let Ok(mut runtime) = handle.state::<commands::AppState>().runtime_lock() { runtime.cancel_all(); }
                tauri::async_runtime::spawn(async move {
                    let start = tokio::time::Instant::now();
                    loop {
                        let empty = handle.state::<commands::AppState>().runtime_lock().map(|r| r.is_empty()).unwrap_or(false);
                        if empty || start.elapsed() >= std::time::Duration::from_secs(8) { break; }
                        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                    }
                    handle.exit(0);
                });
            }
        }
    });
}
