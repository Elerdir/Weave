pub mod commands;
pub mod state;

use std::sync::Arc;
use tauri::Manager;
use tracing_subscriber::{fmt, EnvFilter};
use weave_infrastructure::{
    comfyui::ComfyUiClient,
    db,
    keychain::OsKeychain,
    llm::mistral_client::MistralClient,
    model_manager::LocalModelManager,
};
use weave_application::ports::keychain_port::{ApiService, KeychainPort};

use state::AppState;

pub fn run() {
    fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

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
                setup_state(&app_handle).await.expect("Chyba inicializace stavu");
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::conversation::list_conversations,
            commands::conversation::create_conversation,
            commands::conversation::delete_conversation,
            commands::message::list_messages,
            commands::message::send_message,
            commands::settings::get_api_key_status,
            commands::settings::store_api_key,
            commands::settings::delete_api_key,
            commands::models::list_local_models,
            commands::models::detect_gpu,
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

async fn setup_state(app: &tauri::AppHandle) -> anyhow::Result<()> {
    let data_dir = app
        .path()
        .app_data_dir()
        .expect("Nepodařilo se získat data dir");
    std::fs::create_dir_all(&data_dir)?;

    let db_url = format!(
        "sqlite://{}",
        data_dir.join("weave.db").to_string_lossy()
    );
    let pool = db::create_pool(&db_url).await?;

    let keychain = Arc::new(OsKeychain);
    let mistral_key = keychain
        .retrieve(&ApiService::Mistral)
        .await
        .unwrap_or(None)
        .unwrap_or_default();

    let models_dir = data_dir.join("models");
    let comfyui_url = "http://localhost:8188".to_string();

    let state = AppState {
        pool,
        keychain,
        llm: Arc::new(MistralClient::new(mistral_key)),
        image_gen: Arc::new(ComfyUiClient::new(comfyui_url)),
        model_manager: Arc::new(LocalModelManager::new(models_dir)),
    };

    app.manage(state);
    Ok(())
}
