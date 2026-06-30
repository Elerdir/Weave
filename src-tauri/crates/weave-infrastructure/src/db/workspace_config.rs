use sqlx::SqlitePool;
use weave_application::error::{AppError, AppResult};

pub async fn set_workspace(pool: &SqlitePool, path: &str) -> AppResult<()> {
    sqlx::query!(
        "INSERT INTO workspace_config (key, value) VALUES ('workspace_path', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        path
    )
    .execute(pool)
    .await
    .map_err(|e| AppError::Repository(e.to_string()))?;
    Ok(())
}

pub async fn get_workspace(pool: &SqlitePool) -> AppResult<Option<String>> {
    let row = sqlx::query!(
        "SELECT value FROM workspace_config WHERE key = 'workspace_path'"
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Repository(e.to_string()))?;
    Ok(row.map(|r| r.value))
}
