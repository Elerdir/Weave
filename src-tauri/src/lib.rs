use tracing_subscriber::{fmt, EnvFilter};
use weave_shell::{commands, setup_state};

pub fn run() {
    fmt().with_env_filter(EnvFilter::from_default_env()).init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let app_handle = app.handle().clone();
            tauri::async_runtime::block_on(async move {
                setup_state(&app_handle)
                    .await
                    .expect("Chyba inicializace stavu");
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::conversation::list_conversations,
            commands::conversation::create_conversation,
            commands::conversation::delete_conversation,
            commands::conversation::suggest_export_filename,
            commands::conversation::export_conversation,
            commands::message::list_messages,
            commands::message::send_message,
            commands::settings::get_api_key_status,
            commands::settings::store_api_key,
            commands::settings::delete_api_key,
            commands::settings::get_masked_api_key,
            commands::settings::get_app_setting,
            commands::settings::set_app_setting,
            commands::settings::test_comfyui_connection,
            commands::models::list_local_models,
            commands::models::detect_gpu,
            commands::models::download_model,
            commands::models::delete_model,
            commands::personas::list_personas,
            commands::personas::create_persona,
            commands::personas::delete_persona,
            commands::personas::set_conversation_persona,
            commands::workspace::set_workspace,
            commands::workspace::get_workspace,
            commands::workspace::index_workspace,
            commands::workspace::list_workspace_children,
            commands::workspace::read_workspace_file,
            commands::workspace::write_workspace_file,
            commands::workspace::create_workspace_entry,
            commands::workspace::delete_workspace_entry,
            commands::workspace::rename_workspace_entry,
            commands::workspace::search_workspace,
        ])
        .run(tauri::generate_context!())
        .expect("Chyba spuštění Tauri aplikace");
}
