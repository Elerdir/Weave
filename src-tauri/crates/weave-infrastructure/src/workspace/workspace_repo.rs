use async_trait::async_trait;
use sqlx::SqlitePool;
use weave_application::{
    error::{AppError, AppResult},
    ports::workspace_port::WorkspaceRepository,
};
use weave_domain::workspace::IndexedFile;

pub struct SqliteWorkspaceRepository {
    pool: SqlitePool,
}

impl SqliteWorkspaceRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl WorkspaceRepository for SqliteWorkspaceRepository {
    async fn upsert_file(&self, file: &IndexedFile) -> AppResult<()> {
        sqlx::query!(
            r#"
            INSERT INTO workspace_files (path, name, extension, size_bytes, modified_at, indexed_at, text_content)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(path) DO UPDATE SET
                name = excluded.name,
                extension = excluded.extension,
                size_bytes = excluded.size_bytes,
                modified_at = excluded.modified_at,
                indexed_at = excluded.indexed_at,
                text_content = excluded.text_content
            "#,
            file.path,
            file.name,
            file.extension,
            file.size_bytes as i64,
            file.modified_at.to_rfc3339(),
            file.indexed_at.to_rfc3339(),
            file.text_content,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Repository(e.to_string()))?;
        Ok(())
    }

    async fn remove_file(&self, path: &str) -> AppResult<()> {
        sqlx::query!("DELETE FROM workspace_files WHERE path = ?", path)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Repository(e.to_string()))?;
        Ok(())
    }

    async fn count(&self) -> AppResult<u64> {
        let row = sqlx::query!("SELECT COUNT(*) as count FROM workspace_files")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Repository(e.to_string()))?;
        Ok(row.count as u64)
    }

    async fn search(&self, query: &str, limit: u32) -> AppResult<Vec<IndexedFile>> {
        let rows = sqlx::query!(
            r#"
            SELECT wf.path, wf.name, wf.extension, wf.size_bytes, wf.modified_at,
                   wf.indexed_at, wf.text_content
            FROM workspace_files_fts fts
            JOIN workspace_files wf ON wf.rowid = fts.rowid
            WHERE workspace_files_fts MATCH ?
            ORDER BY rank
            LIMIT ?
            "#,
            query,
            limit,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Repository(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| IndexedFile {
                path: r.path,
                name: r.name,
                extension: r.extension,
                size_bytes: r.size_bytes as u64,
                modified_at: r.modified_at.parse().unwrap_or_default(),
                indexed_at: r.indexed_at.parse().unwrap_or_default(),
                text_content: r.text_content,
            })
            .collect())
    }

    async fn get_file(&self, path: &str) -> AppResult<Option<IndexedFile>> {
        let row = sqlx::query!(
            "SELECT path, name, extension, size_bytes, modified_at, indexed_at, text_content FROM workspace_files WHERE path = ?",
            path
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Repository(e.to_string()))?;

        Ok(row.map(|r| IndexedFile {
            path: r.path,
            name: r.name,
            extension: r.extension,
            size_bytes: r.size_bytes as u64,
            modified_at: r.modified_at.parse().unwrap_or_default(),
            indexed_at: r.indexed_at.parse().unwrap_or_default(),
            text_content: r.text_content,
        }))
    }

    async fn clear(&self) -> AppResult<()> {
        sqlx::query!("DELETE FROM workspace_files")
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Repository(e.to_string()))?;
        Ok(())
    }
}
