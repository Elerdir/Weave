//! Zhuštění konverzace (obdoba „/compact"): LLM shrne dosavadní historii
//! a ta se v DB nahradí jedinou systémovou zprávou se souhrnem. Kontextové
//! okno se tím uvolní, ale model si „pamatuje" podstatné — konverzace
//! vydrží mnohem déle.

use std::sync::Arc;

use tokio::sync::mpsc;
use weave_domain::{conversation::ConversationId, message::Message};

use crate::error::{AppError, AppResult};
use crate::ports::conversation_repository::MessageRepository;
use crate::ports::llm_port::{ChatRequest, LlmPort, StreamChunk};

/// Prefix souhrnu — frontend podle něj pozná souhrnovou bublinu.
pub const SUMMARY_PREFIX: &str = "📝 Souhrn předchozí konverzace";

const SUMMARY_SYSTEM_PROMPT: &str = "Jsi nástroj na zhušťování konverzací. Tvým úkolem je \
    vytvořit stručný, věcný souhrn dosavadní konverzace v jazyce, ve kterém probíhala. \
    Zachovej všechna podstatná fakta, jména, rozhodnutí, preference uživatele a otevřené \
    otázky — souhrn poslouží jako jediná paměť pro pokračování konverzace. \
    Neodpovídej na nic, jen shrň.";

const SUMMARY_INSTRUCTION: &str = "Shrň dosavadní konverzaci podle instrukcí. Odpověz pouze \
    souhrnem, bez úvodu a bez komentářů.";

pub struct CompactConversationUseCase {
    msg_repo: Arc<dyn MessageRepository>,
    llm: Arc<dyn LlmPort>,
}

impl CompactConversationUseCase {
    pub fn new(msg_repo: Arc<dyn MessageRepository>, llm: Arc<dyn LlmPort>) -> Self {
        Self { msg_repo, llm }
    }

    /// Shrne historii konverzace a nahradí ji souhrnem. Vrací text souhrnu.
    pub async fn execute(
        &self,
        conversation_id: ConversationId,
        model_id: String,
    ) -> AppResult<String> {
        let history = self.msg_repo.list_by_conversation(&conversation_id).await?;
        if history.len() < 2 {
            return Err(AppError::Llm(
                "Konverzace je na zhuštění příliš krátká.".into(),
            ));
        }

        // Instrukce jde jako poslední user zpráva a historie zůstává běžnou
        // historií — případné ořezávání kontextu tak zahodí nejstarší zprávy,
        // nikdy instrukci samotnou.
        let mut messages = Vec::with_capacity(history.len() + 2);
        messages.push(Message::system(
            conversation_id.clone(),
            SUMMARY_SYSTEM_PROMPT,
        ));
        messages.extend(history.iter().cloned());
        messages.push(Message::user(conversation_id.clone(), SUMMARY_INSTRUCTION));

        let request = ChatRequest {
            messages,
            model_id,
            max_tokens: Some(1024),
            context_length: None,
            temperature: 0.3,
            stream: true,
        };

        let (tx, mut rx) = mpsc::channel(64);
        self.llm.chat_stream(request, tx).await?;

        let mut summary = String::new();
        while let Some(chunk) = rx.recv().await {
            match chunk {
                StreamChunk::Token(t) => summary.push_str(&t),
                StreamChunk::Error(e) => return Err(AppError::Llm(e)),
                StreamChunk::Done(_) | StreamChunk::ImageStage(_) => {}
            }
        }
        let summary = summary.trim().to_string();
        if summary.is_empty() {
            return Err(AppError::Llm("Model nevrátil žádný souhrn.".into()));
        }

        // Teprve po úspěšném souhrnu smažeme historii — když LLM selže,
        // konverzace zůstává nedotčená.
        self.msg_repo
            .delete_by_conversation(&conversation_id)
            .await?;
        let summary_content = format!("{SUMMARY_PREFIX}:\n\n{summary}");
        self.msg_repo
            .save(&Message::system(conversation_id, &summary_content))
            .await?;
        Ok(summary_content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::conversation_repository::MockMessageRepository;
    use crate::ports::llm_port::MockLlmPort;
    use weave_domain::message::Role;

    fn history(conv: &ConversationId, len: usize) -> Vec<Message> {
        (0..len)
            .map(|i| {
                if i % 2 == 0 {
                    Message::user(conv.clone(), format!("otázka {i}"))
                } else {
                    Message::assistant(conv.clone(), format!("odpověď {i}"), None)
                }
            })
            .collect()
    }

    #[tokio::test]
    async fn replaces_history_with_system_summary() {
        let conv = ConversationId::new();
        let conv_for_list = conv.clone();

        let mut msg_repo = MockMessageRepository::new();
        msg_repo.expect_list_by_conversation().returning(move |_| {
            let h = history(&conv_for_list, 6);
            Box::pin(async move { Ok(h) })
        });
        msg_repo
            .expect_delete_by_conversation()
            .times(1)
            .returning(|_| Box::pin(async { Ok(()) }));
        msg_repo
            .expect_save()
            .times(1)
            .withf(|m: &Message| m.role == Role::System && m.content.starts_with(SUMMARY_PREFIX))
            .returning(|_| Box::pin(async { Ok(()) }));

        let mut llm = MockLlmPort::new();
        llm.expect_chat_stream().returning(|req, tx| {
            // Instrukce musí být poslední zpráva, systémový prompt první.
            assert_eq!(req.messages.first().unwrap().role, Role::System);
            assert_eq!(req.messages.last().unwrap().content, SUMMARY_INSTRUCTION);
            Box::pin(async move {
                let _ = tx
                    .send(StreamChunk::Token("Uživatel řešil X a Y.".into()))
                    .await;
                let _ = tx.send(StreamChunk::Done(Default::default())).await;
                Ok(())
            })
        });

        let uc = CompactConversationUseCase::new(Arc::new(msg_repo), Arc::new(llm));
        let summary = uc.execute(conv, "test-model".into()).await.unwrap();
        assert!(summary.contains("Uživatel řešil X a Y."));
    }

    #[tokio::test]
    async fn llm_error_keeps_history_untouched() {
        let conv = ConversationId::new();
        let conv_for_list = conv.clone();

        let mut msg_repo = MockMessageRepository::new();
        msg_repo.expect_list_by_conversation().returning(move |_| {
            let h = history(&conv_for_list, 4);
            Box::pin(async move { Ok(h) })
        });
        // Žádné delete/save — historie musí zůstat nedotčená.
        msg_repo.expect_delete_by_conversation().never();
        msg_repo.expect_save().never();

        let mut llm = MockLlmPort::new();
        llm.expect_chat_stream().returning(|_, tx| {
            Box::pin(async move {
                let _ = tx.send(StreamChunk::Error("model nedostupný".into())).await;
                Ok(())
            })
        });

        let uc = CompactConversationUseCase::new(Arc::new(msg_repo), Arc::new(llm));
        assert!(uc.execute(conv, "test-model".into()).await.is_err());
    }

    #[tokio::test]
    async fn too_short_conversation_is_rejected() {
        let conv = ConversationId::new();
        let conv_for_list = conv.clone();

        let mut msg_repo = MockMessageRepository::new();
        msg_repo.expect_list_by_conversation().returning(move |_| {
            let h = history(&conv_for_list, 1);
            Box::pin(async move { Ok(h) })
        });

        let uc = CompactConversationUseCase::new(Arc::new(msg_repo), Arc::new(MockLlmPort::new()));
        assert!(uc.execute(conv, "test-model".into()).await.is_err());
    }
}
