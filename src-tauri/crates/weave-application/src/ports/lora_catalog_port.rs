use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::AppResult;

/// Nalezená LoRA — dost informací na stažení a zapojení do workflow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoraInfo {
    /// Lidský název modelu (do logu/progress).
    pub name: String,
    /// Název souboru v `models/loras` (podle něj ho najde LoraLoader).
    pub file_name: String,
    pub download_url: String,
    /// Trigger words — přidávají se do promptu, jinak LoRA „nenaskočí".
    pub trigger_words: Vec<String>,
}

/// Katalog LoRA modelů (CivitAI). Hledá vhodnou LoRA pro daný koncept
/// a základní model (SDXL 1.0 / Pony).
#[async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait LoraCatalogPort: Send + Sync {
    /// Najde nejlépe hodnocenou LoRA pro dotaz; `None` když nic vhodného
    /// (žádný stažitelný .safetensors soubor pro daný base model).
    async fn find_lora(&self, query: &str, base_model: &str) -> AppResult<Option<LoraInfo>>;
}
