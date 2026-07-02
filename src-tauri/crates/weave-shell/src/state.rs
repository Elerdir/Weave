use sqlx::SqlitePool;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;
use weave_application::ports::{
    attachment_store_port::AttachmentStorePort, comfy_installer_port::ComfyInstallerPort,
    image_gen_port::ImageGenPort, keychain_port::KeychainPort, llm_port::LlmPort,
    model_manager_port::ModelManagerPort,
};

pub struct AppState {
    pub pool: SqlitePool,
    pub keychain: Arc<dyn KeychainPort>,
    pub llm: Arc<dyn LlmPort>,
    pub image_gen: Arc<dyn ImageGenPort>,
    pub model_manager: Arc<dyn ModelManagerPort>,
    pub comfy_installer: Arc<dyn ComfyInstallerPort>,
    pub attachment_store: Arc<dyn AttachmentStorePort>,
    /// Token právě běžícího generování — příkaz `stop_generation` ho zruší.
    /// Appka má vždy nejvýš jedno aktivní generování (vstup je při běhu blokovaný).
    pub active_generation: Mutex<Option<CancellationToken>>,
}
