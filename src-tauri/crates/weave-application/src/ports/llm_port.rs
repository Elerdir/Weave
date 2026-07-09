use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use weave_domain::message::{GenerationStats, Message};

use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub messages: Vec<Message>,
    pub model_id: String,
    /// `None` = žádný umělý strop, model generuje dokud sám neskončí
    /// (nebo dokud nenarazí na skutečný technický limit — kontextové okno).
    pub max_tokens: Option<u32>,
    pub temperature: f32,
    /// Kontextové okno pro tuto konverzaci (jen vestavěná inference).
    /// `None` = globální výchozí hodnota z nastavení.
    pub context_length: Option<u32>,
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamChunk {
    Token(String),
    /// Průběh přípravy/generování obrázku — frontend zobrazuje progress kartu.
    ImageStage(ImageStageInfo),
    Done(GenerationStats),
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageStageInfo {
    pub stage: ImageStage,
    /// Doplňkový popis (řádek výstupu instalace, průběh stahování…).
    pub detail: Option<String>,
    /// Skutečný průběh 0–100 (kroky sampleru z ComfyUI WebSocketu).
    /// `None` = neurčitý průběh (animovaný pruh).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageStage {
    Checking,
    Installing,
    DownloadingModel,
    StartingServer,
    /// LLM převádí požadavek uživatele na anglický Stable Diffusion prompt.
    PreparingPrompt,
    Generating,
    Finishing,
}

#[async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait LlmPort: Send + Sync {
    async fn chat_stream(
        &self,
        request: ChatRequest,
        tx: mpsc::Sender<StreamChunk>,
    ) -> AppResult<()>;

    async fn list_available_models(&self) -> AppResult<Vec<String>>;

    /// Uvolní model z paměti (VRAM), pokud nějaký drží. Vestavěná GPU
    /// inference tím na dobu generování obrázku uvolní VRAM pro ComfyUI;
    /// model se pak líně načte při další zprávě. Cloud/HTTP backendy no-op.
    async fn unload(&self) {}

    /// Drží backend právě model v (V)RAM? Pro VRAM indikátor v UI —
    /// cloud/HTTP backendy nic nedrží (výchozí `false`).
    async fn is_loaded(&self) -> bool {
        false
    }
}
