pub mod app_config;
pub mod conversation_repo;
pub mod message_repo;
pub mod persona_repo;
pub mod workspace_config;

use anyhow::Result;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    SqlitePool,
};
use std::str::FromStr;

pub async fn create_pool(database_url: &str) -> Result<SqlitePool> {
    // create_if_missing = při prvním spuštění se soubor DB vytvoří.
    // Bez toho sqlx padá s "unable to open database file" (code 14).
    let options = SqliteConnectOptions::from_str(database_url)?.create_if_missing(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_pool_creates_missing_db_and_runs_migrations() {
        // Regrese: na čerstvém stroji DB soubor neexistuje — musí se vytvořit.
        let path = std::env::temp_dir().join(format!("weave_test_{}.db", uuid::Uuid::new_v4()));
        let _ = std::fs::remove_file(&path);
        assert!(!path.exists());

        let p = path.to_string_lossy().replace('\\', "/");
        let url = if p.starts_with('/') {
            format!("sqlite://{p}")
        } else {
            format!("sqlite:///{p}")
        };

        let pool = create_pool(&url).await.expect("pool se má vytvořit");

        // Migrace proběhly → tabulka conversations existuje a je prázdná.
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM conversations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);

        drop(pool);
        let _ = std::fs::remove_file(&path);
    }
}
