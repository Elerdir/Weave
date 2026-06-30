use sqlx::SqlitePool;
use std::sync::Arc;
use weave_application::ports::{
    image_gen_port::ImageGenPort, keychain_port::KeychainPort, llm_port::LlmPort,
    model_manager_port::ModelManagerPort,
};

pub struct AppState {
    pub pool: SqlitePool,
    pub keychain: Arc<dyn KeychainPort>,
    pub llm: Arc<dyn LlmPort>,
    pub image_gen: Arc<dyn ImageGenPort>,
    pub model_manager: Arc<dyn ModelManagerPort>,
}
