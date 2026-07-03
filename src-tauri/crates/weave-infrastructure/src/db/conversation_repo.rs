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
        use sqlx::Row;

        let pattern = format!("%{query}%");
        // Fulltext přes názvy I obsah zpráv. Runtime query (ne query! makro),
        // ať není potřeba obnovovat .sqlx cache.
        let rows = sqlx::query(
            "SELECT DISTINCT c.id, c.title, c.persona_id, c.pinned, c.created_at, c.updated_at
             FROM conversations c
             LEFT JOIN messages m ON m.conversation_id = c.id
             WHERE c.title LIKE ?1 OR m.content LIKE ?1
             ORDER BY c.updated_at DESC",
        )
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Repository(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| Conversation {
                id: ConversationId::from_uuid(Uuid::parse_str(&r.get::<String, _>("id")).unwrap()),
                title: ConversationTitle::new(r.get::<String, _>("title")).unwrap(),
                persona_id: r.get::<Option<String>, _>("persona_id"),
                pinned: r.get::<i64, _>("pinned") != 0,
                created_at: r.get::<String, _>("created_at").parse().unwrap(),
                updated_at: r.get::<String, _>("updated_at").parse().unwrap(),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::message_repo::SqliteMessageRepository;
    use weave_application::ports::conversation_repository::MessageRepository;
    use weave_domain::message::Message;

    async fn test_pool() -> sqlx::SqlitePool {
        let dir = std::env::temp_dir().join(format!("weave_conv_repo_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let url = format!("sqlite://{}", dir.join("test.db").to_string_lossy());
        crate::db::create_pool(&url).await.unwrap()
    }

    #[tokio::test]
    async fn search_matches_title_and_message_content() {
        let pool = test_pool().await;
        let repo = SqliteConversationRepository::new(pool.clone());
        let msg_repo = SqliteMessageRepository::new(pool);

        let plan = Conversation::new(ConversationTitle::new("Plán výletu").unwrap());
        let jina = Conversation::new(ConversationTitle::new("Jiné téma").unwrap());
        repo.save(&plan).await.unwrap();
        repo.save(&jina).await.unwrap();
        msg_repo
            .save(&Message::user(
                jina.id.clone(),
                "recept na svíčkovou omáčku",
            ))
            .await
            .unwrap();

        // Podle názvu
        let by_title = repo.search("výlet").await.unwrap();
        assert_eq!(by_title.len(), 1);
        assert_eq!(by_title[0].title.as_str(), "Plán výletu");

        // Podle obsahu zprávy (název nic neobsahuje)
        let by_content = repo.search("svíčkovou").await.unwrap();
        assert_eq!(by_content.len(), 1);
        assert_eq!(by_content[0].title.as_str(), "Jiné téma");

        // Bez shody
        assert!(repo.search("neexistuje").await.unwrap().is_empty());
    }
}
