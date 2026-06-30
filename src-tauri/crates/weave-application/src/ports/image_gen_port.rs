use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageRequest {
    pub prompt: String,
    pub negative_prompt: Option<String>,
    pub width: u32,
    pub height: u32,
    pub steps: u32,
    pub cfg_scale: f32,
    pub seed: Option<i64>,
    pub style_preset: StylePreset,
    pub reference_image_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StylePreset {
    Realistic,
    Anime,
    Artistic,
    ThreeD,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ImageProgress {
    Progress { step: u32, total: u32 },
    Done { output_path: String },
    Error(String),
}

#[async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait ImageGenPort: Send + Sync {
    async fn generate(
        &self,
        request: ImageRequest,
        tx: mpsc::Sender<ImageProgress>,
    ) -> AppResult<()>;

    async fn is_available(&self) -> bool;
}
