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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StylePreset {
    Realistic,
    Anime,
    Artistic,
    ThreeD,
}

impl StylePreset {
    /// Odhadne styl obrázku z textu promptu (klíčová slova, cz+en).
    /// Výchozí je realistický styl.
    pub fn classify(prompt: &str) -> Self {
        let lower = prompt.to_lowercase();
        const ANIME: &[&str] = &["anime", "manga", "chibi", "waifu", "ghibli"];
        const THREE_D: &[&str] = &["3d", "render", "blender", "pixar", "low poly", "voxel"];
        const ARTISTIC: &[&str] = &[
            "malba",
            "obraz ve stylu",
            "akvarel",
            "olejomalba",
            "painting",
            "watercolor",
            "oil on canvas",
            "skica",
            "sketch",
            "ilustrace",
            "illustration",
        ];

        if ANIME.iter().any(|k| lower.contains(k)) {
            StylePreset::Anime
        } else if THREE_D.iter().any(|k| lower.contains(k)) {
            StylePreset::ThreeD
        } else if ARTISTIC.iter().any(|k| lower.contains(k)) {
            StylePreset::Artistic
        } else {
            StylePreset::Realistic
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_detects_anime_and_3d_and_artistic() {
        assert_eq!(
            StylePreset::classify("nakresli mi anime postavu s mečem"),
            StylePreset::Anime
        );
        assert_eq!(
            StylePreset::classify("3D render of a castle, blender style"),
            StylePreset::ThreeD
        );
        assert_eq!(
            StylePreset::classify("olejomalba západu slunce nad mořem"),
            StylePreset::Artistic
        );
    }

    #[test]
    fn classify_defaults_to_realistic() {
        assert_eq!(
            StylePreset::classify("fotka kočky na zahradě"),
            StylePreset::Realistic
        );
    }
}
