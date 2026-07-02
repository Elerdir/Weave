use async_trait::async_trait;
use sqlx::SqlitePool;
use uuid::Uuid;
use weave_application::{
    error::{AppError, AppResult},
    ports::conversation_repository::MessageRepository,
};
use weave_domain::{
    conversation::ConversationId,
    message::{Attachment, Message, MessageId, Role},
};

pub struct SqliteMessageRepository {
    pool: SqlitePool,
}

impl SqliteMessageRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl MessageRepository for SqliteMessageRepository {
    async fn save(&self, m: &Message) -> AppResult<()> {
        let role = match m.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
        };
        let attachments = serde_json::to_string(&m.attachments)
            .map_err(|e| AppError::Repository(e.to_string()))?;
        let stats = m
            .stats
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| AppError::Repository(e.to_string()))?;

        let id = m.id.to_string();
        let conversation_id = m.conversation_id.as_uuid().to_string();
        let created_at = m.created_at.to_rfc3339();

        sqlx::query!(
            r#"
            INSERT INTO messages (id, conversation_id, role, content, attachments, stats, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
            id,
            conversation_id,
            role,
            m.content,
            attachments,
            stats,
            created_at,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Repository(e.to_string()))?;
        Ok(())
    }

    async fn list_by_conversation(&self, id: &ConversationId) -> AppResult<Vec<Message>> {
        let id_str = id.as_uuid().to_string();
        let rows = sqlx::query!(
            "SELECT id, conversation_id, role, content, attachments, stats, created_at FROM messages WHERE conversation_id = ? ORDER BY created_at ASC",
            id_str
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Repository(e.to_string()))?;

        rows.into_iter()
            .map(|r| {
                let role = match r.role.as_str() {
                    "assistant" => Role::Assistant,
                    "system" => Role::System,
                    _ => Role::User,
                };
                let attachments: Vec<Attachment> = serde_json::from_str(&r.attachments)
                    .map_err(|e| AppError::Repository(e.to_string()))?;
                let stats = r
                    .stats
                    .map(|s| serde_json::from_str(&s))
                    .transpose()
                    .map_err(|e| AppError::Repository(e.to_string()))?;

                Ok(Message {
                    id: MessageId::default(),
                    conversation_id: ConversationId::from_uuid(
                        Uuid::parse_str(&r.conversation_id).unwrap(),
                    ),
                    role,
                    content: r.content,
                    attachments,
                    stats,
                    created_at: r.created_at.parse().unwrap(),
                })
            })
            .collect()
    }

    async fn delete_by_conversation(&self, id: &ConversationId) -> AppResult<()> {
        let id_str = id.as_uuid().to_string();
        sqlx::query!("DELETE FROM messages WHERE conversation_id = ?", id_str)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Repository(e.to_string()))?;
        Ok(())
    }

    async fn delete_trailing_assistant_messages(&self, id: &ConversationId) -> AppResult<()> {
        let id_str = id.as_uuid().to_string();
        // Bez zprávy uživatele je poddotaz NULL a nesmaže se nic.
        // Runtime query (ne query! makro), ať není potřeba obnovovat .sqlx cache.
        sqlx::query(
            "DELETE FROM messages
             WHERE conversation_id = ?
               AND created_at > (
                   SELECT MAX(created_at) FROM messages
                   WHERE conversation_id = ? AND role = 'user'
               )",
        )
        .bind(&id_str)
        .bind(&id_str)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Repository(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::conversation_repo::SqliteConversationRepository;
    use weave_application::ports::conversation_repository::ConversationRepository;
    use weave_domain::conversation::{Conversation, ConversationTitle};
    use weave_domain::message::GenerationStats;

    async fn test_pool() -> SqlitePool {
        let dir = std::env::temp_dir().join(format!("weave_msg_repo_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let url = format!("sqlite://{}", dir.join("test.db").to_string_lossy());
        crate::db::create_pool(&url).await.unwrap()
    }

    /// Založí konverzaci v DB (messages mají FK na conversations).
    async fn seed_conversation(pool: &SqlitePool) -> ConversationId {
        let conversation = Conversation::new(ConversationTitle::new("Test").unwrap());
        let id = conversation.id.clone();
        SqliteConversationRepository::new(pool.clone())
            .save(&conversation)
            .await
            .unwrap();
        id
    }

    #[tokio::test]
    async fn delete_trailing_removes_only_messages_after_last_user() {
        let pool = test_pool().await;
        let conv = seed_conversation(&pool).await;
        let repo = SqliteMessageRepository::new(pool);

        // Historie: user → assistant → user → assistant (created_at vzestupně)
        let mut base = chrono::Utc::now();
        for (role_msg, text) in [
            (Message::user(conv.clone(), "první otázka"), "q1"),
            (
                Message::assistant(conv.clone(), "první odpověď", None),
                "a1",
            ),
            (Message::user(conv.clone(), "druhá otázka"), "q2"),
            (
                Message::assistant(
                    conv.clone(),
                    "druhá odpověď",
                    Some(GenerationStats::default()),
                ),
                "a2",
            ),
        ] {
            let _ = text;
            let mut m = role_msg;
            base += chrono::Duration::seconds(1);
            m.created_at = base;
            repo.save(&m).await.unwrap();
        }

        repo.delete_trailing_assistant_messages(&conv)
            .await
            .unwrap();

        let remaining = repo.list_by_conversation(&conv).await.unwrap();
        let contents: Vec<&str> = remaining.iter().map(|m| m.content.as_str()).collect();
        assert_eq!(
            contents,
            vec!["první otázka", "první odpověď", "druhá otázka"]
        );
    }

    #[tokio::test]
    async fn delete_trailing_is_noop_without_user_messages() {
        let pool = test_pool().await;
        let conv = seed_conversation(&pool).await;
        let repo = SqliteMessageRepository::new(pool);

        repo.save(&Message::assistant(conv.clone(), "osamocená odpověď", None))
            .await
            .unwrap();

        repo.delete_trailing_assistant_messages(&conv)
            .await
            .unwrap();

        assert_eq!(repo.list_by_conversation(&conv).await.unwrap().len(), 1);
    }
}
