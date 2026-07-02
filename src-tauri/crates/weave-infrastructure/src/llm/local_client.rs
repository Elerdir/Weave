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

/// Klient pro lokální OpenAI-kompatibilní server (llama.cpp `llama-server`,
/// LM Studio, Ollama `/v1` apod.). Nevestavuje inference — mluví přes HTTP.
pub struct LocalLlmClient {
    http: Client,
    base_url: String,
}

impl LocalLlmClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            http: Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
        }
    }

    /// Ověří dostupnost serveru (GET /v1/models).
    pub async fn is_available(&self) -> bool {
        self.http
            .get(format!("{}/v1/models", self.base_url))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
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
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
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
impl LlmPort for LocalLlmClient {
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
            .post(format!("{}/v1/chat/completions", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Llm(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AppError::Llm(format!("Lokální LLM {status}: {text}")));
        }

        let mut stream = response.bytes_stream();
        let start = std::time::Instant::now();
        let mut completion_tokens = 0u32;
        let mut prompt_tokens = 0u32;

        while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|e| AppError::Llm(e.to_string()))?;
            let text = String::from_utf8_lossy(&bytes);

            for line in text.lines() {
                let Some(data) = line.strip_prefix("data: ") else {
                    continue;
                };
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
                            backend: ModelBackend::LocalCpu,
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
                            if tx.send(StreamChunk::Token(content)).await.is_err() {
                                // Příjemce zmizel (uživatel zastavil generování)
                                // — ukončením se zavře i HTTP stream.
                                return Ok(());
                            }
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
            .get(format!("{}/v1/models", self.base_url))
            .send()
            .await
            .map_err(|e| AppError::Llm(e.to_string()))?
            .json::<ModelsResponse>()
            .await
            .map_err(|e| AppError::Llm(e.to_string()))?;

        Ok(resp.data.into_iter().map(|m| m.id).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use weave_domain::{conversation::ConversationId, message::Message};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn chat_request() -> ChatRequest {
        ChatRequest {
            messages: vec![Message::user(ConversationId::new(), "Ahoj")],
            model_id: "local-model".into(),
            max_tokens: Some(128),
            context_length: None,
            temperature: 0.7,
            stream: true,
        }
    }

    #[tokio::test]
    async fn streams_tokens_and_finishes_with_stats() {
        let server = MockServer::start().await;
        // OpenAI-kompatibilní SSE stream
        let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"Ahoj\"}}]}\n\
                   data: {\"choices\":[{\"delta\":{\"content\":\" světe\"}}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2}}\n\
                   data: [DONE]\n";
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_string(sse))
            .mount(&server)
            .await;

        let client = LocalLlmClient::new(server.uri());
        let (tx, mut rx) = mpsc::channel(16);
        client.chat_stream(chat_request(), tx).await.unwrap();

        let mut tokens = String::new();
        let mut done_stats = None;
        while let Some(chunk) = rx.recv().await {
            match chunk {
                StreamChunk::Token(t) => tokens.push_str(&t),
                StreamChunk::Done(s) => done_stats = Some(s),
                StreamChunk::Error(e) => panic!("nečekaná chyba: {e}"),
            }
        }

        assert_eq!(tokens, "Ahoj světe");
        let stats = done_stats.expect("chybí Done se statistikami");
        assert_eq!(stats.prompt_tokens, 5);
        assert!(matches!(stats.backend, ModelBackend::LocalCpu));
    }

    #[tokio::test]
    async fn error_status_is_propagated() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
            .mount(&server)
            .await;

        let client = LocalLlmClient::new(server.uri());
        let (tx, _rx) = mpsc::channel(16);
        let result = client.chat_stream(chat_request(), tx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn stops_cleanly_when_receiver_is_dropped() {
        let server = MockServer::start().await;
        let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"Ahoj\"}}]}\n\
                   data: {\"choices\":[{\"delta\":{\"content\":\" světe\"}}]}\n\
                   data: [DONE]\n";
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_string(sse))
            .mount(&server)
            .await;

        let client = LocalLlmClient::new(server.uri());
        let (tx, rx) = mpsc::channel(16);
        drop(rx); // uživatel zastavil generování — příjemce zmizel

        // Nesmí to skončit chybou ani viset — jen tiše přestat streamovat.
        client.chat_stream(chat_request(), tx).await.unwrap();
    }

    #[test]
    fn max_tokens_none_is_omitted_from_request_body() {
        let body = ChatCompletionRequest {
            model: "local-model".into(),
            messages: vec![],
            max_tokens: None,
            temperature: 0.7,
            stream: true,
        };

        let json = serde_json::to_string(&body).unwrap();

        assert!(
            !json.contains("max_tokens"),
            "max_tokens: None se nesmí posílat — server pak generuje bez umělého omezení"
        );
    }

    #[tokio::test]
    async fn is_available_true_when_models_ok() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{\"data\":[]}"))
            .mount(&server)
            .await;

        let client = LocalLlmClient::new(server.uri());
        assert!(client.is_available().await);
    }
}
