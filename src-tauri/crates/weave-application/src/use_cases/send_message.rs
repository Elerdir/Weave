use std::sync::Arc;

use tokio::sync::mpsc;
use weave_domain::{
    conversation::ConversationId,
    message::Message,
    model::IntentClassifier,
};

use crate::{
    error::{AppError, AppResult},
    ports::{
        conversation_repository::{ConversationRepository, MessageRepository},
        llm_port::{ChatRequest, LlmPort, StreamChunk},
        image_gen_port::{ImageGenPort, ImageProgress, ImageRequest, StylePreset},
    },
};

pub struct SendMessageUseCase {
    conv_repo: Arc<dyn ConversationRepository>,
    msg_repo: Arc<dyn MessageRepository>,
    llm: Arc<dyn LlmPort>,
    image_gen: Arc<dyn ImageGenPort>,
}

impl SendMessageUseCase {
    pub fn new(
        conv_repo: Arc<dyn ConversationRepository>,
        msg_repo: Arc<dyn MessageRepository>,
        llm: Arc<dyn LlmPort>,
        image_gen: Arc<dyn ImageGenPort>,
    ) -> Self {
        Self { conv_repo, msg_repo, llm, image_gen }
    }

    pub async fn execute(
        &self,
        conversation_id: ConversationId,
        content: String,
        stream_tx: mpsc::Sender<StreamChunk>,
    ) -> AppResult<()> {
        // Ověříme že konverzace existuje
        self.conv_repo
            .find_by_id(&conversation_id)
            .await?
            .ok_or_else(|| AppError::Repository(format!("Konverzace {conversation_id} neexistuje")))?;

        // Uložíme zprávu uživatele
        let user_msg = Message::user(conversation_id.clone(), &content);
        self.msg_repo.save(&user_msg).await?;

        // Klasifikace záměru → routing
        let intent = IntentClassifier::classify(&content);
        tracing::debug!(?intent, "Intent klasifikován");

        match intent {
            weave_domain::model::Intent::ImageGeneration => {
                self.handle_image(content, stream_tx).await
            }
            _ => {
                let history = self.msg_repo.list_by_conversation(&conversation_id).await?;
                let model_id = Self::model_for_intent(&intent);
                let request = ChatRequest {
                    messages: history,
                    model_id,
                    max_tokens: 2048,
                    temperature: 0.7,
                    stream: true,
                };
                self.llm.chat_stream(request, stream_tx).await
            }
        }
    }

    async fn handle_image(
        &self,
        prompt: String,
        stream_tx: mpsc::Sender<StreamChunk>,
    ) -> AppResult<()> {
        let (img_tx, mut img_rx) = mpsc::channel(32);
        let request = ImageRequest {
            prompt,
            negative_prompt: None,
            width: 1024,
            height: 1024,
            steps: 20,
            cfg_scale: 7.0,
            seed: None,
            style_preset: StylePreset::Realistic,
            reference_image_path: None,
        };

        self.image_gen.generate(request, img_tx).await?;

        while let Some(progress) = img_rx.recv().await {
            match progress {
                ImageProgress::Done { output_path } => {
                    let _ = stream_tx
                        .send(StreamChunk::Token(format!("![obrázek]({output_path})")))
                        .await;
                    let _ = stream_tx.send(StreamChunk::Done(Default::default())).await;
                }
                ImageProgress::Error(e) => {
                    let _ = stream_tx.send(StreamChunk::Error(e)).await;
                }
                ImageProgress::Progress { .. } => {}
            }
        }
        Ok(())
    }

    fn model_for_intent(intent: &weave_domain::model::Intent) -> String {
        use weave_domain::model::Intent::*;
        match intent {
            TextChat => "mistral-small-latest",
            StoryWriting => "mistral-large-latest",
            CodeAssistance => "codestral-latest",
            Reasoning => "magistral-medium-latest",
            FileAnalysis => "pixtral-large-latest",
            ImageGeneration => unreachable!("Image handled separately"),
        }
        .to_string()
    }
}
