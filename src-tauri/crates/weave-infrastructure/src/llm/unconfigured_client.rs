use async_trait::async_trait;
use tokio::sync::mpsc;
use weave_application::{
    error::{AppError, AppResult},
    ports::llm_port::{ChatRequest, LlmPort, StreamChunk},
};

/// Používá se, když není nastavený žádný funkční backend (chybí GPU model
///) — dřív se v tomhle stavu tiše zkoušel Mistral s prázdným API
/// klíčem, což skončilo matoucí chybou z cizího API. Tohle dá rovnou jasnou
/// instrukci, co v appce udělat.
pub struct UnconfiguredLlmClient;

#[async_trait]
impl LlmPort for UnconfiguredLlmClient {
    async fn chat_stream(
        &self,
        _request: ChatRequest,
        _tx: mpsc::Sender<StreamChunk>,
    ) -> AppResult<()> {
        Err(AppError::Llm(
            "Není nastavený žádný AI model. V Nastavení → AI model dokonči nastavení GPU \
             modelu (.gguf)."
                .to_string(),
        ))
    }

    async fn list_available_models(&self) -> AppResult<Vec<String>> {
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn chat_stream_returns_clear_configuration_error() {
        let client = UnconfiguredLlmClient;
        let (tx, _rx) = mpsc::channel(1);
        let request = ChatRequest {
            messages: vec![],
            model_id: "local-model".into(),
            max_tokens: None,
            temperature: 0.7,
            context_length: None,
            stream: true,
        };
        let err = client.chat_stream(request, tx).await.unwrap_err();
        assert!(err.to_string().contains("Není nastavený žádný AI model"));
    }
}
