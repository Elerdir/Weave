use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalModel {
    pub id: String,
    pub name: String,
    pub version: String,
    pub size_bytes: u64,
    pub path: String,
    pub checksum: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DownloadProgress {
    Started { model_id: String, total_bytes: u64 },
    Progress { downloaded: u64, total: u64 },
    Verifying,
    Done { model: LocalModel },
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    pub name: String,
    pub vram_mb: u64,
    pub backend: GpuBackend,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuBackend {
    Cuda,
    Metal,
    Vulkan,
    Cpu,
}

#[async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait ModelManagerPort: Send + Sync {
    async fn list_local(&self) -> AppResult<Vec<LocalModel>>;
    async fn download(
        &self,
        model_id: &str,
        source_url: &str,
        tx: mpsc::Sender<DownloadProgress>,
    ) -> AppResult<()>;
    async fn delete(&self, model_id: &str) -> AppResult<()>;
    async fn detect_gpu(&self) -> AppResult<Option<GpuInfo>>;
    async fn check_for_updates(&self) -> AppResult<Vec<String>>;
    /// Aktuální složka, do které se stahují modely.
    async fn models_dir(&self) -> AppResult<std::path::PathBuf>;
    /// Přesune existující modely (manifest + `.gguf` soubory) do nové složky
    /// a od té chvíle do ní stahuje i vše nové — bez nutnosti restartu appky.
    async fn set_models_dir(&self, new_dir: std::path::PathBuf) -> AppResult<()>;
}
