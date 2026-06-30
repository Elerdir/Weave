use async_trait::async_trait;
use weave_domain::workspace::{IndexedFile, WorkspaceEntry};

use crate::error::AppResult;

#[async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait WorkspaceRepository: Send + Sync {
    /// Uloží/aktualizuje indexovaný soubor.
    async fn upsert_file(&self, file: &IndexedFile) -> AppResult<()>;
    /// Smaže soubor z indexu (byl smazán na disku).
    async fn remove_file(&self, path: &str) -> AppResult<()>;
    /// Vrátí počet indexovaných souborů.
    async fn count(&self) -> AppResult<u64>;
    /// Full-text hledání v indexu.
    async fn search(&self, query: &str, limit: u32) -> AppResult<Vec<IndexedFile>>;
    /// Vrátí obsah souboru z indexu (nebo None pokud není).
    async fn get_file(&self, path: &str) -> AppResult<Option<IndexedFile>>;
    /// Smaže celý index.
    async fn clear(&self) -> AppResult<()>;
}

#[async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait FileSystemPort: Send + Sync {
    /// Rekurzivně vrátí všechny položky ve složce (soubory + podsložky).
    async fn list_recursive(&self, root: &str) -> AppResult<Vec<WorkspaceEntry>>;
    /// Přečte textový obsah souboru.
    async fn read_text(&self, path: &str) -> AppResult<String>;
    /// Zapíše textový obsah do souboru (vytvoří pokud neexistuje).
    async fn write_text(&self, path: &str, content: &str) -> AppResult<()>;
    /// Vytvoří soubor nebo složku.
    async fn create(&self, path: &str, is_dir: bool) -> AppResult<()>;
    /// Smaže soubor nebo složku.
    async fn delete(&self, path: &str) -> AppResult<()>;
    /// Přejmenuje/přesune.
    async fn rename(&self, from: &str, to: &str) -> AppResult<()>;
    /// Vrátí přímé potomky složky (nerekurzivně) — pro lazy tree loading.
    async fn list_children(&self, path: &str) -> AppResult<Vec<WorkspaceEntry>>;
}
