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

/// Druh modelu při procházení katalogu obrázkových modelů.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageModelKind {
    Checkpoint,
    Lora,
}

impl ImageModelKind {
    /// Hodnota parametru `types` v CivitAI API.
    pub fn api_type(&self) -> &'static str {
        match self {
            ImageModelKind::Checkpoint => "Checkpoint",
            ImageModelKind::Lora => "LORA",
        }
    }
}

/// Položka z procházení katalogu (checkpoint nebo LoRA) — dost informací
/// na zobrazení karty s náhledem i na stažení.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogBrowseItem {
    pub name: String,
    pub creator: String,
    pub kind: ImageModelKind,
    /// Základní architektura verze (SDXL 1.0, Pony…) — s ní musí ladit
    /// checkpoint/PuLID větev workflow.
    pub base_model: String,
    pub preview_image_url: Option<String>,
    pub downloads: u64,
    pub nsfw: bool,
    /// Název souboru v ComfyUI models složce (checkpoints/ nebo loras/).
    pub file_name: String,
    pub download_url: String,
    pub size_bytes: u64,
    /// Trigger words (jen LoRA) — bez nich se LoRA v promptu neprojeví.
    pub trigger_words: Vec<String>,
}

/// Katalog obrázkových modelů (CivitAI). Hledá vhodnou LoRA pro daný koncept
/// a základní model (SDXL 1.0 / Pony) a umí procházet checkpointy/LoRA.
#[async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait LoraCatalogPort: Send + Sync {
    /// Najde nejlépe hodnocenou LoRA pro dotaz; `None` když nic vhodného
    /// (žádný stažitelný .safetensors soubor pro daný base model).
    async fn find_lora(&self, query: &str, base_model: &str) -> AppResult<Option<LoraInfo>>;

    /// Fulltext procházení katalogu (checkpointy/LoRA) s náhledy — řazeno
    /// podle počtu stažení. `base_model` = volitelný filtr architektury.
    async fn browse(
        &self,
        query: &str,
        kind: ImageModelKind,
        base_model: Option<&str>,
    ) -> AppResult<Vec<CatalogBrowseItem>>;
}
