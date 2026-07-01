use async_trait::async_trait;
use sqlx::SqlitePool;
use weave_application::{
    error::{AppError, AppResult},
    ports::persona_repository::PersonaRepository,
};
use weave_domain::persona::Persona;

pub struct SqlitePersonaRepository {
    pool: SqlitePool,
}

impl SqlitePersonaRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PersonaRepository for SqlitePersonaRepository {
    async fn save(&self, p: &Persona) -> AppResult<()> {
        let created_at = chrono::Utc::now().to_rfc3339();
        sqlx::query!(
            "INSERT INTO personas (id, name, icon, system_prompt, created_at)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                icon = excluded.icon,
                system_prompt = excluded.system_prompt",
            p.id,
            p.name,
            p.icon,
            p.system_prompt,
            created_at,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Repository(e.to_string()))?;
        Ok(())
    }

    async fn list_custom(&self) -> AppResult<Vec<Persona>> {
        let rows = sqlx::query!(
            "SELECT id, name, icon, system_prompt FROM personas ORDER BY created_at ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Repository(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| Persona {
                id: r.id,
                name: r.name,
                icon: r.icon,
                system_prompt: r.system_prompt,
                builtin: false,
            })
            .collect())
    }

    async fn find_by_id(&self, id: &str) -> AppResult<Option<Persona>> {
        let row = sqlx::query!(
            "SELECT id, name, icon, system_prompt FROM personas WHERE id = ?",
            id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Repository(e.to_string()))?;

        Ok(row.map(|r| Persona {
            id: r.id,
            name: r.name,
            icon: r.icon,
            system_prompt: r.system_prompt,
            builtin: false,
        }))
    }

    async fn delete(&self, id: &str) -> AppResult<()> {
        sqlx::query!("DELETE FROM personas WHERE id = ?", id)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Repository(e.to_string()))?;
        Ok(())
    }
}
