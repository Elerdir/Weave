use sqlx::SqlitePool;
use std::sync::Arc;
use weave_application::ports::{
    comfy_installer_port::ComfyInstallerPort, image_gen_port::ImageGenPort,
    keychain_port::KeychainPort, llm_port::LlmPort, model_manager_port::ModelManagerPort,
};

pub struct AppState {
    pub pool: SqlitePool,
    pub keychain: Arc<dyn KeychainPort>,
    pub llm: Arc<dyn LlmPort>,
    pub image_gen: Arc<dyn ImageGenPort>,
    pub model_manager: Arc<dyn ModelManagerPort>,
    pub comfy_installer: Arc<dyn ComfyInstallerPort>,
}
