pub mod commands;
pub mod state;

use std::sync::Arc;
use tauri::Manager;
use weave_application::ports::keychain_port::{ApiService, KeychainPort};
use weave_infrastructure::{
    attachment_store::LocalAttachmentStore, comfy_installer::LocalComfyInstaller,
    comfyui::ComfyUiClient, db, hf_catalog::HuggingFaceCatalog, keychain::OsKeychain,
    llm::mistral_client::MistralClient, model_manager::LocalModelManager,
};

use state::AppState;

/// Inicializuje aplikační stav (DB pool, adaptery) a vloží ho do Tauri.
/// Volá se z `setup` hooku v kompozičním kořeni (binárka `weave-app`).
pub async fn setup_state(app: &tauri::AppHandle) -> anyhow::Result<()> {
    let data_dir = app
        .path()
        .app_data_dir()
        .expect("Nepodařilo se získat data dir");
    std::fs::create_dir_all(&data_dir)?;

    let db_url = format!("sqlite://{}", data_dir.join("weave.db").to_string_lossy());
    let pool = db::create_pool(&db_url).await?;

    let keychain = Arc::new(OsKeychain::new(data_dir.join("secure")));
    let mistral_key = keychain
        .retrieve(&ApiService::Mistral)
        .await
        .unwrap_or(None)
        .unwrap_or_default();

    // Uživatel může v nastavení/wizardu přesměrovat stahování modelů jinam
    // než na výchozí app-data disk (typicky C:) — pokud je uloženo, použije se.
    let models_dir = match db::app_config::get(&pool, commands::models::MODELS_DIR_KEY).await {
        Ok(Some(custom)) if !custom.is_empty() => std::path::PathBuf::from(custom),
        _ => data_dir.join("models"),
    };
    if let Ok(Some(value)) =
        db::app_config::get(&pool, commands::models::DOWNLOAD_SEGMENTS_KEY).await
    {
        if let Ok(segments) = value.parse::<u64>() {
            weave_infrastructure::parallel_download::set_max_segments_override(segments);
        }
    }
    // Musí odpovídat portu, na kterém LocalComfyInstaller spouští server.
    let comfyui_url = format!(
        "http://localhost:{}",
        weave_infrastructure::comfy_installer::COMFYUI_DEFAULT_PORT
    );
    let comfyui_install_dir = data_dir.join("comfyui");
    let reference_images_dir = data_dir.join("weave").join("reference-images");

    let state = AppState {
        pool,
        keychain,
        llm: Arc::new(MistralClient::new(mistral_key)),
        // Galerie musí být uvnitř assetProtocol scope ($APPDATA/weave/**),
        // jinak se náhledy vygenerovaných obrázků v chatu nezobrazí.
        image_gen: Arc::new(
            ComfyUiClient::new(comfyui_url)
                .with_gallery_dir(data_dir.join("weave").join("gallery")),
        ),
        model_manager: Arc::new(LocalModelManager::new(models_dir)),
        model_catalog: Arc::new(HuggingFaceCatalog::default()),
        comfy_installer: Arc::new(LocalComfyInstaller::new(comfyui_install_dir)),
        attachment_store: Arc::new(LocalAttachmentStore::new(reference_images_dir)),
        active_generation: std::sync::Mutex::new(None),
        embedded_llm: std::sync::Mutex::new(None),
        log_dir: data_dir.join("logs"),
    };

    app.manage(state);
    Ok(())
}
