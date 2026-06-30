use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use weave_application::{
    error::{AppError, AppResult},
    ports::llm_port::{ChatRequest, LlmPort, StreamChunk},
};
use weave_domain::message::{GenerationStats, ModelBackend, Role};

const MISTRAL_API_BASE: &str = "https://api.mistral.ai/v1";

pub struct MistralClient {
    http: Client,
    api_key: String,
}

impl MistralClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            http: Client::new(),
            api_key: api_key.into(),
        }
    }
}

#[derive(Serialize)]
struct ApiMessage {
    role: &'static str,
    content: String,
}

#[derive(Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ApiMessage>,
    max_tokens: u32,
    temperature: f32,
    stream: bool,
}

#[derive(Deserialize)]
struct StreamDelta {
    content: Option<String>,
}

#[derive(Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct StreamChunkResponse {
    choices: Vec<StreamChoice>,
    usage: Option<UsageStats>,
}

#[derive(Deserialize)]
struct UsageStats {
    prompt_tokens: u32,
    completion_tokens: u32,
}

#[async_trait]
impl LlmPort for MistralClient {
    async fn chat_stream(
        &self,
        request: ChatRequest,
        tx: mpsc::Sender<StreamChunk>,
    ) -> AppResult<()> {
        let messages: Vec<ApiMessage> = request
            .messages
            .iter()
            .map(|m| ApiMessage {
                role: match m.role {
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::System => "system",
                },
                content: m.content.clone(),
            })
            .collect();

        let body = ChatCompletionRequest {
            model: request.model_id.clone(),
            messages,
            max_tokens: request.max_tokens,
            temperature: request.temperature,
            stream: true,
        };

        let response = self
            .http
            .post(format!("{MISTRAL_API_BASE}/chat/completions"))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Llm(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AppError::Llm(format!("Mistral API {status}: {text}")));
        }

        let mut stream = response.bytes_stream();
        let start = std::time::Instant::now();
        let mut completion_tokens = 0u32;
        let mut prompt_tokens = 0u32;

        while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|e| AppError::Llm(e.to_string()))?;
            let text = String::from_utf8_lossy(&bytes);

            for line in text.lines() {
                let Some(data) = line.strip_prefix("data: ") else { continue };
                if data == "[DONE]" {
                    let elapsed = start.elapsed().as_secs_f64();
                    let tps = if elapsed > 0.0 {
                        completion_tokens as f64 / elapsed
                    } else {
                        0.0
                    };
                    let _ = tx
                        .send(StreamChunk::Done(GenerationStats {
                            tokens_per_second: tps,
                            prompt_tokens,
                            completion_tokens,
                            model_id: request.model_id.clone(),
                            backend: ModelBackend::MistralApi,
                        }))
                        .await;
                    return Ok(());
                }

                if let Ok(parsed) = serde_json::from_str::<StreamChunkResponse>(data) {
                    if let Some(usage) = parsed.usage {
                        prompt_tokens = usage.prompt_tokens;
                        completion_tokens = usage.completion_tokens;
                    }
                    for choice in parsed.choices {
                        if let Some(content) = choice.delta.content {
                            completion_tokens += 1;
                            let _ = tx.send(StreamChunk::Token(content)).await;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    async fn list_available_models(&self) -> AppResult<Vec<String>> {
        #[derive(Deserialize)]
        struct Model {
            id: String,
        }
        #[derive(Deserialize)]
        struct ModelsResponse {
            data: Vec<Model>,
        }

        let resp = self
            .http
            .get(format!("{MISTRAL_API_BASE}/models"))
            .bearer_auth(&self.api_key)
            .send()
            .await
            .map_err(|e| AppError::Llm(e.to_string()))?
            .json::<ModelsResponse>()
            .await
            .map_err(|e| AppError::Llm(e.to_string()))?;

        Ok(resp.data.into_iter().map(|m| m.id).collect())
    }
}
