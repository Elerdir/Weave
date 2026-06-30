use sqlx::SqlitePool;
use weave_application::error::{AppError, AppResult};

/// Vrátí hodnotu nastavení podle klíče (nebo None).
pub async fn get(pool: &SqlitePool, key: &str) -> AppResult<Option<String>> {
    let row = sqlx::query!("SELECT value FROM app_config WHERE key = ?", key)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::Repository(e.to_string()))?;
    Ok(row.map(|r| r.value))
}

/// Uloží (upsert) hodnotu nastavení.
pub async fn set(pool: &SqlitePool, key: &str, value: &str) -> AppResult<()> {
    sqlx::query!(
        "INSERT INTO app_config (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        key,
        value
    )
    .execute(pool)
    .await
    .map_err(|e| AppError::Repository(e.to_string()))?;
    Ok(())
}
