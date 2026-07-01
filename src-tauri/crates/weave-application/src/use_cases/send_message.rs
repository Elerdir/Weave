use std::sync::Arc;

use tokio::sync::mpsc;
use weave_domain::{
    conversation::ConversationId,
    message::{Attachment, Message},
    model::IntentClassifier,
};

use crate::{
    error::{AppError, AppResult},
    ports::{
        attachment_store_port::AttachmentStorePort,
        conversation_repository::{ConversationRepository, MessageRepository},
        image_gen_port::{ImageGenPort, ImageProgress, ImageRequest, StylePreset},
        llm_port::{ChatRequest, LlmPort, StreamChunk},
        persona_repository::PersonaRepository,
        workspace_port::WorkspaceRepository,
    },
};

pub struct SendMessageUseCase {
    conv_repo: Arc<dyn ConversationRepository>,
    msg_repo: Arc<dyn MessageRepository>,
    llm: Arc<dyn LlmPort>,
    image_gen: Arc<dyn ImageGenPort>,
    workspace_repo: Arc<dyn WorkspaceRepository>,
    persona_repo: Arc<dyn PersonaRepository>,
    attachment_store: Arc<dyn AttachmentStorePort>,
}

impl SendMessageUseCase {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        conv_repo: Arc<dyn ConversationRepository>,
        msg_repo: Arc<dyn MessageRepository>,
        llm: Arc<dyn LlmPort>,
        image_gen: Arc<dyn ImageGenPort>,
        workspace_repo: Arc<dyn WorkspaceRepository>,
        persona_repo: Arc<dyn PersonaRepository>,
        attachment_store: Arc<dyn AttachmentStorePort>,
    ) -> Self {
        Self {
            conv_repo,
            msg_repo,
            llm,
            image_gen,
            workspace_repo,
            persona_repo,
            attachment_store,
        }
    }

    pub async fn execute(
        &self,
        conversation_id: ConversationId,
        content: String,
        file_refs: Vec<String>,
        reference_images: Vec<String>,
        stream_tx: mpsc::Sender<StreamChunk>,
    ) -> AppResult<()> {
        // Ověříme že konverzace existuje a získáme její personu
        let conversation = self
            .conv_repo
            .find_by_id(&conversation_id)
            .await?
            .ok_or_else(|| {
                AppError::Repository(format!("Konverzace {conversation_id} neexistuje"))
            })?;

        // Referenční obrázky zkopírujeme do vlastního úložiště appky (přežije
        // i smazání/přesun originálu) a uložíme je jako přílohy zprávy.
        let mut attachments = Vec::with_capacity(reference_images.len());
        for path in &reference_images {
            let stored = self.attachment_store.store_reference_image(path).await?;
            attachments.push(Attachment::Image {
                path: stored.path,
                mime: stored.mime,
            });
        }

        // Uložíme zprávu uživatele
        let user_msg =
            Message::user(conversation_id.clone(), &content).with_attachments(attachments.clone());
        self.msg_repo.save(&user_msg).await?;

        // Klasifikace záměru → routing
        let intent = IntentClassifier::classify(&content);
        tracing::debug!(?intent, "Intent klasifikován");

        match intent {
            weave_domain::model::Intent::ImageGeneration => {
                let reference_image_path = attachments.iter().find_map(|a| match a {
                    Attachment::Image { path, .. } => Some(path.clone()),
                    Attachment::Document { .. } => None,
                });
                self.handle_image(content, reference_image_path, stream_tx)
                    .await
            }
            _ => {
                let mut history = self.msg_repo.list_by_conversation(&conversation_id).await?;

                // Přiložené @soubory → system kontext na začátku (neukládá se do historie)
                if let Some(context) = self.build_file_context(&file_refs).await? {
                    history.insert(0, Message::system(conversation_id.clone(), context));
                }

                // Persona konverzace → system prompt úplně na začátek
                if let Some(prompt) = self
                    .resolve_persona_prompt(&conversation.persona_id)
                    .await?
                {
                    history.insert(0, Message::system(conversation_id.clone(), prompt));
                }

                let model_id = Self::model_for_intent(&intent);
                let request = ChatRequest {
                    messages: history,
                    model_id,
                    max_tokens: None,
                    temperature: 0.7,
                    stream: true,
                };
                self.llm.chat_stream(request, stream_tx).await
            }
        }
    }

    /// Vyřeší system prompt persony konverzace (vestavěná z domény, vlastní z repo).
    async fn resolve_persona_prompt(
        &self,
        persona_id: &Option<String>,
    ) -> AppResult<Option<String>> {
        let Some(id) = persona_id else {
            return Ok(None);
        };

        if weave_domain::persona::Persona::is_builtin(id) {
            return Ok(weave_domain::persona::builtin_personas()
                .into_iter()
                .find(|p| &p.id == id)
                .map(|p| p.system_prompt));
        }

        Ok(self
            .persona_repo
            .find_by_id(id)
            .await?
            .map(|p| p.system_prompt))
    }

    /// Sestaví system kontext z obsahu @souborů (z workspace indexu).
    /// Vrátí None pokud nejsou žádné reference nebo žádný soubor nemá obsah.
    async fn build_file_context(&self, file_refs: &[String]) -> AppResult<Option<String>> {
        if file_refs.is_empty() {
            return Ok(None);
        }

        let mut context = String::from("Uživatel přiložil tyto soubory jako kontext k dotazu:\n\n");
        let mut any = false;

        for path in file_refs {
            if let Some(file) = self.workspace_repo.get_file(path).await? {
                if file.text_content.is_empty() {
                    continue;
                }
                context.push_str(&format!(
                    "### {}\n```\n{}\n```\n\n",
                    file.name, file.text_content
                ));
                any = true;
            }
        }

        Ok(if any { Some(context) } else { None })
    }

    async fn handle_image(
        &self,
        prompt: String,
        reference_image_path: Option<String>,
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
            reference_image_path,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{
        attachment_store_port::{MockAttachmentStorePort, StoredImage},
        conversation_repository::{MockConversationRepository, MockMessageRepository},
        image_gen_port::MockImageGenPort,
        llm_port::MockLlmPort,
        persona_repository::MockPersonaRepository,
        workspace_port::MockWorkspaceRepository,
    };
    use weave_domain::{conversation::Conversation, workspace::IndexedFile};

    fn make_uc(ws: MockWorkspaceRepository) -> SendMessageUseCase {
        SendMessageUseCase::new(
            Arc::new(MockConversationRepository::new()),
            Arc::new(MockMessageRepository::new()),
            Arc::new(MockLlmPort::new()),
            Arc::new(MockImageGenPort::new()),
            Arc::new(ws),
            Arc::new(MockPersonaRepository::new()),
            Arc::new(MockAttachmentStorePort::new()),
        )
    }

    fn dummy_conversation() -> Conversation {
        Conversation::new(weave_domain::conversation::ConversationTitle::new("Test").unwrap())
    }

    #[allow(clippy::too_many_arguments)]
    fn make_full_uc(
        conv_repo: MockConversationRepository,
        msg_repo: MockMessageRepository,
        llm: MockLlmPort,
        image_gen: MockImageGenPort,
        attachment_store: MockAttachmentStorePort,
    ) -> SendMessageUseCase {
        SendMessageUseCase::new(
            Arc::new(conv_repo),
            Arc::new(msg_repo),
            Arc::new(llm),
            Arc::new(image_gen),
            Arc::new(MockWorkspaceRepository::new()),
            Arc::new(MockPersonaRepository::new()),
            Arc::new(attachment_store),
        )
    }

    #[tokio::test]
    async fn build_file_context_includes_file_content() {
        let mut ws = MockWorkspaceRepository::new();
        ws.expect_get_file().returning(|path: &str| {
            let name = path.rsplit(['/', '\\']).next().unwrap_or("f").to_string();
            Box::pin(async move {
                Ok(Some(IndexedFile {
                    path: format!("/ws/{name}"),
                    name,
                    extension: Some("txt".into()),
                    size_bytes: 12,
                    modified_at: chrono::Utc::now(),
                    indexed_at: chrono::Utc::now(),
                    text_content: "obsah souboru".into(),
                }))
            })
        });

        let uc = make_uc(ws);
        let ctx = uc
            .build_file_context(&["/ws/poznamky.txt".into()])
            .await
            .unwrap();

        assert!(ctx.is_some());
        let ctx = ctx.unwrap();
        assert!(ctx.contains("poznamky.txt"));
        assert!(ctx.contains("obsah souboru"));
    }

    #[tokio::test]
    async fn build_file_context_none_for_no_refs() {
        let uc = make_uc(MockWorkspaceRepository::new());
        assert!(uc.build_file_context(&[]).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn build_file_context_none_when_files_have_no_text() {
        let mut ws = MockWorkspaceRepository::new();
        ws.expect_get_file()
            .returning(|_| Box::pin(async { Ok(None) }));
        let uc = make_uc(ws);
        assert!(uc
            .build_file_context(&["/ws/binary.png".into()])
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn execute_stores_reference_image_as_attachment_on_text_chat() {
        let mut conv_repo = MockConversationRepository::new();
        conv_repo
            .expect_find_by_id()
            .returning(|_| Box::pin(async { Ok(Some(dummy_conversation())) }));

        let mut msg_repo = MockMessageRepository::new();
        msg_repo
            .expect_save()
            .withf(|m: &Message| {
                m.role == weave_domain::message::Role::User
                    && m.attachments
                        == vec![Attachment::Image {
                            path: "/data/weave/reference-images/stored.png".into(),
                            mime: "image/png".into(),
                        }]
            })
            .returning(|_| Box::pin(async { Ok(()) }));
        msg_repo
            .expect_list_by_conversation()
            .returning(|_| Box::pin(async { Ok(vec![]) }));

        let mut llm = MockLlmPort::new();
        llm.expect_chat_stream()
            .returning(|_, _| Box::pin(async { Ok(()) }));

        let mut attachment_store = MockAttachmentStorePort::new();
        attachment_store
            .expect_store_reference_image()
            .withf(|p: &str| p == "/tmp/original.png")
            .returning(|_| {
                Box::pin(async {
                    Ok(StoredImage {
                        path: "/data/weave/reference-images/stored.png".into(),
                        mime: "image/png".into(),
                    })
                })
            });

        let uc = make_full_uc(
            conv_repo,
            msg_repo,
            llm,
            MockImageGenPort::new(),
            attachment_store,
        );
        let (tx, _rx) = mpsc::channel(8);

        uc.execute(
            ConversationId::new(),
            "jak se dnes máš?".into(),
            vec![],
            vec!["/tmp/original.png".into()],
            tx,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn execute_passes_reference_image_to_image_request_on_image_intent() {
        let mut conv_repo = MockConversationRepository::new();
        conv_repo
            .expect_find_by_id()
            .returning(|_| Box::pin(async { Ok(Some(dummy_conversation())) }));

        let mut msg_repo = MockMessageRepository::new();
        msg_repo
            .expect_save()
            .returning(|_| Box::pin(async { Ok(()) }));

        let mut image_gen = MockImageGenPort::new();
        image_gen
            .expect_generate()
            .withf(|req: &ImageRequest, _tx| {
                req.reference_image_path.as_deref()
                    == Some("/data/weave/reference-images/stored.png")
            })
            .returning(|_, _| Box::pin(async { Ok(()) }));

        let mut attachment_store = MockAttachmentStorePort::new();
        attachment_store
            .expect_store_reference_image()
            .returning(|_| {
                Box::pin(async {
                    Ok(StoredImage {
                        path: "/data/weave/reference-images/stored.png".into(),
                        mime: "image/png".into(),
                    })
                })
            });

        let uc = make_full_uc(
            conv_repo,
            msg_repo,
            MockLlmPort::new(),
            image_gen,
            attachment_store,
        );
        let (tx, _rx) = mpsc::channel(8);

        uc.execute(
            ConversationId::new(),
            "nakresli mě jako rytíře".into(),
            vec![],
            vec!["/tmp/selfie.png".into()],
            tx,
        )
        .await
        .unwrap();
    }
}
