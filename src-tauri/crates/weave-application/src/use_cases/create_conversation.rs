use std::sync::Arc;

use weave_domain::conversation::{Conversation, ConversationTitle};

use crate::{error::AppResult, ports::conversation_repository::ConversationRepository};

pub struct CreateConversationUseCase {
    repo: Arc<dyn ConversationRepository>,
}

impl CreateConversationUseCase {
    pub fn new(repo: Arc<dyn ConversationRepository>) -> Self {
        Self { repo }
    }

    pub async fn execute(&self, title: impl Into<String>) -> AppResult<Conversation> {
        let title = ConversationTitle::new(title)?;
        let conversation = Conversation::new(title);
        self.repo.save(&conversation).await?;
        tracing::info!(id = %conversation.id, "Konverzace vytvořena");
        Ok(conversation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::conversation_repository::MockConversationRepository;

    #[tokio::test]
    async fn creates_and_saves_conversation() {
        let mut mock = MockConversationRepository::new();
        mock.expect_save()
            .times(1)
            .returning(|_| Box::pin(async { Ok(()) }));

        let uc = CreateConversationUseCase::new(Arc::new(mock));
        let result = uc.execute("Nová konverzace").await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().title.as_str(), "Nová konverzace");
    }

    #[tokio::test]
    async fn rejects_empty_title() {
        let mock = MockConversationRepository::new();
        let uc = CreateConversationUseCase::new(Arc::new(mock));
        let result = uc.execute("").await;
        assert!(result.is_err());
    }
}
