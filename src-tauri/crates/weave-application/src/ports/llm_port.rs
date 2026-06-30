use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use weave_domain::message::{GenerationStats, Message};

use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub messages: Vec<Message>,
    pub model_id: String,
    pub max_tokens: u32,
    pub temperature: f32,
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamChunk {
    Token(String),
    Done(GenerationStats),
    Error(String),
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
}
