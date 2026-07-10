use async_trait::async_trait;
use sqlx::{Row, SqlitePool};
use weave_application::{
    error::{AppError, AppResult},
    ports::generation_settings_repository::GenerationSettingsRepository,
};
use weave_domain::{conversation::ConversationId, generation_settings::GenerationSettings};

/// SQLite úložiště per-konverzačních parametrů generování.
/// Záměrně runtime dotazy (ne query! makra) — není potřeba obnovovat
/// sqlx offline cache při každé změně schématu.
pub struct SqliteGenerationSettingsRepository {
    pool: SqlitePool,
}

impl SqliteGenerationSettingsRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl GenerationSettingsRepository for SqliteGenerationSettingsRepository {
    async fn get(&self, id: &ConversationId) -> AppResult<GenerationSettings> {
        let id_str = id.as_uuid().to_string();
        let row = sqlx::query(
            "SELECT context_length, temperature, max_tokens, pulid_weight, face_detailer,
                    runtime_backend, image_checkpoint, image_lora
             FROM conversation_settings WHERE conversation_id = ?",
        )
        .bind(&id_str)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Repository(e.to_string()))?;

        let Some(row) = row else {
            return Ok(GenerationSettings::default());
        };

        Ok(GenerationSettings {
            context_length: row
                .try_get::<Option<i64>, _>("context_length")
                .map_err(|e| AppError::Repository(e.to_string()))?
                .map(|v| v as u32),
            temperature: row
                .try_get::<Option<f64>, _>("temperature")
                .map_err(|e| AppError::Repository(e.to_string()))?
                .map(|v| v as f32),
            max_tokens: row
                .try_get::<Option<i64>, _>("max_tokens")
                .map_err(|e| AppError::Repository(e.to_string()))?
                .map(|v| v as u32),
            pulid_weight: row
                .try_get::<Option<f64>, _>("pulid_weight")
                .map_err(|e| AppError::Repository(e.to_string()))?
                .map(|v| v as f32),
            face_detailer: row
                .try_get::<Option<i64>, _>("face_detailer")
                .map_err(|e| AppError::Repository(e.to_string()))?
                .map(|v| v != 0),
            runtime_backend: row
                .try_get::<Option<String>, _>("runtime_backend")
                .map_err(|e| AppError::Repository(e.to_string()))?,
            image_checkpoint: row
                .try_get::<Option<String>, _>("image_checkpoint")
                .map_err(|e| AppError::Repository(e.to_string()))?,
            image_lora: row
                .try_get::<Option<String>, _>("image_lora")
                .map_err(|e| AppError::Repository(e.to_string()))?,
        })
    }

    async fn set(&self, id: &ConversationId, settings: &GenerationSettings) -> AppResult<()> {
        let id_str = id.as_uuid().to_string();
        sqlx::query(
            "INSERT INTO conversation_settings
                 (conversation_id, context_length, temperature, max_tokens, pulid_weight,
                  face_detailer, runtime_backend, image_checkpoint, image_lora)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(conversation_id) DO UPDATE SET
                 context_length = excluded.context_length,
                 temperature = excluded.temperature,
                 max_tokens = excluded.max_tokens,
                 pulid_weight = excluded.pulid_weight,
                 face_detailer = excluded.face_detailer,
                 runtime_backend = excluded.runtime_backend,
                 image_checkpoint = excluded.image_checkpoint,
                 image_lora = excluded.image_lora",
        )
        .bind(&id_str)
        .bind(settings.context_length.map(|v| v as i64))
        .bind(settings.temperature.map(|v| v as f64))
        .bind(settings.max_tokens.map(|v| v as i64))
        .bind(settings.pulid_weight.map(|v| v as f64))
        .bind(settings.face_detailer.map(i64::from))
        .bind(settings.runtime_backend.as_deref())
        .bind(settings.image_checkpoint.as_deref())
        .bind(settings.image_lora.as_deref())
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

    async fn seeded() -> (SqliteGenerationSettingsRepository, ConversationId) {
        let dir = std::env::temp_dir().join(format!("weave_gen_settings_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let url = format!("sqlite://{}", dir.join("test.db").to_string_lossy());
        let pool = crate::db::create_pool(&url).await.unwrap();

        let conversation = Conversation::new(ConversationTitle::new("Test").unwrap());
        let id = conversation.id.clone();
        SqliteConversationRepository::new(pool.clone())
            .save(&conversation)
            .await
            .unwrap();

        (SqliteGenerationSettingsRepository::new(pool), id)
    }

    #[tokio::test]
    async fn get_returns_defaults_when_no_record_exists() {
        let (repo, conv) = seeded().await;
        assert_eq!(
            repo.get(&conv).await.unwrap(),
            GenerationSettings::default()
        );
    }

    #[tokio::test]
    async fn set_then_get_roundtrips_and_upserts() {
        let (repo, conv) = seeded().await;

        let first = GenerationSettings {
            context_length: Some(16384),
            temperature: Some(1.2),
            max_tokens: Some(2048),
            pulid_weight: Some(0.8),
            face_detailer: Some(true),
            runtime_backend: Some("openvino_npu".to_string()),
            image_checkpoint: Some("realvis_ultra.safetensors".to_string()),
            image_lora: Some("nikol_v1.safetensors".to_string()),
        };
        repo.set(&conv, &first).await.unwrap();
        assert_eq!(repo.get(&conv).await.unwrap(), first);

        // Upsert — druhé uložení přepíše, včetně shození hodnoty na None
        let second = GenerationSettings {
            context_length: Some(8192),
            temperature: None,
            max_tokens: None,
            pulid_weight: None,
            face_detailer: Some(false),
            runtime_backend: Some("default".to_string()),
            image_checkpoint: None,
            image_lora: None,
        };
        repo.set(&conv, &second).await.unwrap();
        assert_eq!(repo.get(&conv).await.unwrap(), second);
    }
}
