use async_trait::async_trait;
use sqlx::SqlitePool;
use uuid::Uuid;
use weave_application::{
    error::{AppError, AppResult},
    ports::conversation_repository::ConversationRepository,
};
use weave_domain::conversation::{Conversation, ConversationId, ConversationTitle};

pub struct SqliteConversationRepository {
    pool: SqlitePool,
}

impl SqliteConversationRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ConversationRepository for SqliteConversationRepository {
    async fn save(&self, c: &Conversation) -> AppResult<()> {
        let id = c.id.as_uuid().to_string();
        let title = c.title.as_str();
        let persona_id = c.persona_id.clone();
        let created_at = c.created_at.to_rfc3339();
        let updated_at = c.updated_at.to_rfc3339();

        sqlx::query!(
            r#"
            INSERT INTO conversations (id, title, persona_id, pinned, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                persona_id = excluded.persona_id,
                pinned = excluded.pinned,
                updated_at = excluded.updated_at
            "#,
            id,
            title,
            persona_id,
            c.pinned,
            created_at,
            updated_at,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Repository(e.to_string()))?;
        Ok(())
    }

    async fn find_by_id(&self, id: &ConversationId) -> AppResult<Option<Conversation>> {
        let id_str = id.as_uuid().to_string();
        let row = sqlx::query!(
            "SELECT id, title, persona_id, pinned, created_at, updated_at FROM conversations WHERE id = ?",
            id_str
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Repository(e.to_string()))?;

        Ok(row.map(|r| Conversation {
            id: ConversationId::from_uuid(Uuid::parse_str(&r.id).unwrap()),
            title: ConversationTitle::new(r.title).unwrap(),
            persona_id: r.persona_id,
            pinned: r.pinned != 0,
            created_at: r.created_at.parse().unwrap(),
            updated_at: r.updated_at.parse().unwrap(),
        }))
    }

    async fn list_all(&self) -> AppResult<Vec<Conversation>> {
        let rows = sqlx::query!(
            "SELECT id, title, persona_id, pinned, created_at, updated_at FROM conversations ORDER BY pinned DESC, updated_at DESC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Repository(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| Conversation {
                id: ConversationId::from_uuid(Uuid::parse_str(&r.id).unwrap()),
                title: ConversationTitle::new(r.title).unwrap(),
                persona_id: r.persona_id,
                pinned: r.pinned != 0,
                created_at: r.created_at.parse().unwrap(),
                updated_at: r.updated_at.parse().unwrap(),
            })
            .collect())
    }

    async fn delete(&self, id: &ConversationId) -> AppResult<()> {
        let id_str = id.as_uuid().to_string();
        sqlx::query!("DELETE FROM conversations WHERE id = ?", id_str)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Repository(e.to_string()))?;
        Ok(())
    }

    async fn search(&self, query: &str) -> AppResult<Vec<Conversation>> {
        let pattern = format!("%{query}%");
        let rows = sqlx::query!(
            r#"
            SELECT DISTINCT c.id, c.title, c.persona_id, c.pinned, c.created_at, c.updated_at
            FROM conversations c
            WHERE c.title LIKE ?
            ORDER BY c.updated_at DESC
            "#,
            pattern
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Repository(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| Conversation {
                id: ConversationId::from_uuid(Uuid::parse_str(&r.id).unwrap()),
                title: ConversationTitle::new(r.title).unwrap(),
                persona_id: r.persona_id,
                pinned: r.pinned != 0,
                created_at: r.created_at.parse().unwrap(),
                updated_at: r.updated_at.parse().unwrap(),
            })
            .collect())
    }
}
