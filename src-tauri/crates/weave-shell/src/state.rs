use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;
use weave_application::ports::{
    attachment_store_port::AttachmentStorePort, comfy_installer_port::ComfyInstallerPort,
    image_gen_port::ImageGenPort, keychain_port::KeychainPort, llm_port::LlmPort,
    model_catalog_port::ModelCatalogPort, model_manager_port::ModelManagerPort,
};

/// Klíč kešované vestavěné inference: (cesta k modelu, GPU vrstvy, kontext).
/// Při změně kterékoli hodnoty se model přenahraje.
pub type EmbeddedLlmKey = (String, u32, u32);

pub struct AppState {
    pub pool: SqlitePool,
    pub keychain: Arc<dyn KeychainPort>,
    pub llm: Arc<dyn LlmPort>,
    pub image_gen: Arc<dyn ImageGenPort>,
    pub model_manager: Arc<dyn ModelManagerPort>,
    /// Vyhledávání modelů na HuggingFace Hub (read-only katalog).
    pub model_catalog: Arc<dyn ModelCatalogPort>,
    pub comfy_installer: Arc<dyn ComfyInstallerPort>,
    pub attachment_store: Arc<dyn AttachmentStorePort>,
    /// Token právě běžícího generování — příkaz `stop_generation` ho zruší.
    /// Appka má vždy nejvýš jedno aktivní generování (vstup je při běhu blokovaný).
    pub active_generation: Mutex<Option<CancellationToken>>,
    /// Kešovaný klient vestavěné inference — model zůstává načtený ve VRAM
    /// mezi zprávami místo přenahrávání při každé z nich. Uvolní se při
    /// změně klíče nebo přepnutí na jiný backend.
    pub embedded_llm: Mutex<Option<(EmbeddedLlmKey, Arc<dyn LlmPort>)>>,
    /// Složka se soubory aplikačních logů (denní rotace) — čte ji log viewer.
    pub log_dir: PathBuf,
}
