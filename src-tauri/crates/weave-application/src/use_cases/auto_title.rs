//! Automatické pojmenování konverzace: po první výměně LLM vygeneruje
//! krátký výstižný název. KDY se volá, rozhoduje frontend (jen u konverzací
//! s výchozím názvem) — use case pojmenuje vždy, když má z čeho.

use std::sync::Arc;

use tokio::sync::mpsc;
use weave_domain::{
    conversation::{ConversationId, ConversationTitle},
    message::{Message, Role},
};

use crate::error::{AppError, AppResult};
use crate::ports::conversation_repository::{ConversationRepository, MessageRepository};
use crate::ports::llm_port::{ChatRequest, LlmPort, StreamChunk};

const TITLE_SYSTEM_PROMPT: &str = "Vytvoř krátký výstižný název konverzace (nejvýše 5 slov) \
    v jazyce, kterým konverzace probíhá. Odpověz POUZE názvem — bez uvozovek, bez tečky, \
    bez vysvětlování.";

/// Kolik znaků z každé zprávy poslat LLM — na název bohatě stačí začátek.
const EXCERPT_CHARS: usize = 500;

pub struct AutoTitleUseCase {
    conv_repo: Arc<dyn ConversationRepository>,
    msg_repo: Arc<dyn MessageRepository>,
    llm: Arc<dyn LlmPort>,
}

impl AutoTitleUseCase {
    pub fn new(
        conv_repo: Arc<dyn ConversationRepository>,
        msg_repo: Arc<dyn MessageRepository>,
        llm: Arc<dyn LlmPort>,
    ) -> Self {
        Self {
            conv_repo,
            msg_repo,
            llm,
        }
    }

    /// Vygeneruje a uloží název podle první výměny. Vrací nový název.
    pub async fn execute(
        &self,
        conversation_id: ConversationId,
        model_id: String,
    ) -> AppResult<String> {
        let mut conversation = self
            .conv_repo
            .find_by_id(&conversation_id)
            .await?
            .ok_or_else(|| {
                AppError::Repository(format!("Konverzace {conversation_id} neexistuje"))
            })?;

        let history = self.msg_repo.list_by_conversation(&conversation_id).await?;
        let first_user = history.iter().find(|m| m.role == Role::User);
        let first_assistant = history.iter().find(|m| m.role == Role::Assistant);
        let (Some(user_msg), Some(assistant_msg)) = (first_user, first_assistant) else {
            return Err(AppError::Repository(
                "Konverzace ještě nemá kompletní výměnu — není z čeho tvořit název".into(),
            ));
        };

        let excerpt = format!(
            "Dotaz: {}\n\nOdpověď: {}",
            truncate_chars(&user_msg.content, EXCERPT_CHARS),
            truncate_chars(&assistant_msg.content, EXCERPT_CHARS),
        );

        let messages = vec![
            Message::system(conversation_id.clone(), TITLE_SYSTEM_PROMPT),
            Message::user(conversation_id.clone(), excerpt),
        ];
        let request = ChatRequest {
            messages,
            model_id,
            max_tokens: Some(32),
            context_length: None,
            temperature: 0.3,
            stream: true,
        };

        let (tx, mut rx) = mpsc::channel(64);
        self.llm.chat_stream(request, tx).await?;
        let mut raw = String::new();
        while let Some(chunk) = rx.recv().await {
            match chunk {
                StreamChunk::Token(t) => raw.push_str(&t),
                StreamChunk::Error(e) => return Err(AppError::Llm(e)),
                StreamChunk::Done(_) | StreamChunk::ImageStage(_) => {}
            }
        }

        let title = sanitize_title(&raw)
            .ok_or_else(|| AppError::Llm("Model nevrátil použitelný název".into()))?;
        let new_title = ConversationTitle::new(&title)?;
        conversation.rename(new_title);
        self.conv_repo.save(&conversation).await?;
        Ok(title)
    }
}

fn truncate_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// První neprázdný řádek bez uvozovek/tečky na konci, zastropovaný na 60 znaků.
fn sanitize_title(raw: &str) -> Option<String> {
    let line = raw
        .lines()
        .map(|l| {
            l.trim()
                .trim_matches('"')
                .trim_matches('«')
                .trim_matches('»')
        })
        .find(|l| !l.is_empty())?;
    let line = line.trim_end_matches('.').trim();
    if line.is_empty() {
        return None;
    }
    Some(truncate_chars(line, 60))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::conversation_repository::{
        MockConversationRepository, MockMessageRepository,
    };
    use crate::ports::llm_port::MockLlmPort;
    use weave_domain::conversation::Conversation;

    fn history(conv: &ConversationId) -> Vec<Message> {
        vec![
            Message::user(conv.clone(), "Jak nastavit CUDA na Windows?"),
            Message::assistant(conv.clone(), "Postup je následující…", None),
        ]
    }

    #[tokio::test]
    async fn renames_conversation_with_sanitized_title() {
        let conv_id = ConversationId::new();
        let conv_for_list = conv_id.clone();

        let mut conv_repo = MockConversationRepository::new();
        conv_repo.expect_find_by_id().returning(|_| {
            Box::pin(async {
                Ok(Some(Conversation::new(
                    ConversationTitle::new("Nová konverzace").unwrap(),
                )))
            })
        });
        conv_repo
            .expect_save()
            .times(1)
            .withf(|c| c.title.as_str() == "Nastavení CUDA na Windows")
            .returning(|_| Box::pin(async { Ok(()) }));

        let mut msg_repo = MockMessageRepository::new();
        msg_repo.expect_list_by_conversation().returning(move |_| {
            let h = history(&conv_for_list);
            Box::pin(async move { Ok(h) })
        });

        let mut llm = MockLlmPort::new();
        llm.expect_chat_stream().returning(|_, tx| {
            Box::pin(async move {
                let _ = tx
                    .send(StreamChunk::Token("\"Nastavení CUDA na Windows.\"".into()))
                    .await;
                let _ = tx.send(StreamChunk::Done(Default::default())).await;
                Ok(())
            })
        });

        let uc = AutoTitleUseCase::new(Arc::new(conv_repo), Arc::new(msg_repo), Arc::new(llm));
        let title = uc.execute(conv_id, "test-model".into()).await.unwrap();
        assert_eq!(title, "Nastavení CUDA na Windows");
    }

    #[tokio::test]
    async fn incomplete_exchange_is_rejected_without_llm_call() {
        let conv_id = ConversationId::new();
        let conv_for_list = conv_id.clone();

        let mut conv_repo = MockConversationRepository::new();
        conv_repo.expect_find_by_id().returning(|_| {
            Box::pin(async {
                Ok(Some(Conversation::new(
                    ConversationTitle::new("Nová konverzace").unwrap(),
                )))
            })
        });
        conv_repo.expect_save().never();

        let mut msg_repo = MockMessageRepository::new();
        msg_repo.expect_list_by_conversation().returning(move |_| {
            let h = vec![Message::user(conv_for_list.clone(), "jen dotaz")];
            Box::pin(async move { Ok(h) })
        });

        let uc = AutoTitleUseCase::new(
            Arc::new(conv_repo),
            Arc::new(msg_repo),
            Arc::new(MockLlmPort::new()),
        );
        assert!(uc.execute(conv_id, "m".into()).await.is_err());
    }

    #[test]
    fn sanitize_title_cleans_quotes_dots_and_length() {
        assert_eq!(
            sanitize_title("\"Plán výletu do Alp.\"\ndalší řádek"),
            Some("Plán výletu do Alp".into())
        );
        assert_eq!(sanitize_title("  \n\n"), None);
        let long = "x".repeat(100);
        assert_eq!(sanitize_title(&long).unwrap().chars().count(), 60);
    }
}
