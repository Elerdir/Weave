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
            .map(|s| serde_json::to_string(s))
            .transpose()
            .map_err(|e| AppError::Repository(e.to_string()))?;

        sqlx::query!(
            r#"
            INSERT INTO messages (id, conversation_id, role, content, attachments, stats, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
            m.id.to_string(),
            m.conversation_id.as_uuid().to_string(),
            role,
            m.content,
            attachments,
            stats,
            m.created_at.to_rfc3339(),
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Repository(e.to_string()))?;
        Ok(())
    }

    async fn list_by_conversation(&self, id: &ConversationId) -> AppResult<Vec<Message>> {
        let rows = sqlx::query!(
            "SELECT id, conversation_id, role, content, attachments, stats, created_at FROM messages WHERE conversation_id = ? ORDER BY created_at ASC",
            id.as_uuid().to_string()
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
        sqlx::query!(
            "DELETE FROM messages WHERE conversation_id = ?",
            id.as_uuid().to_string()
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Repository(e.to_string()))?;
        Ok(())
    }
}
