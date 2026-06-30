use std::sync::Arc;

use tokio::sync::mpsc;
use weave_domain::workspace::{IndexedFile, WorkspaceEntry, WorkspacePath};

use crate::{
    error::{AppError, AppResult},
    ports::workspace_port::{FileSystemPort, WorkspaceRepository},
};

#[derive(Debug, Clone)]
pub enum IndexProgress {
    Started { total: u64 },
    File { path: String, indexed: u64, total: u64 },
    Done { indexed: u64, skipped: u64 },
    Error(String),
}

pub struct WorkspaceUseCase {
    fs: Arc<dyn FileSystemPort>,
    repo: Arc<dyn WorkspaceRepository>,
}

impl WorkspaceUseCase {
    pub fn new(fs: Arc<dyn FileSystemPort>, repo: Arc<dyn WorkspaceRepository>) -> Self {
        Self { fs, repo }
    }

    /// Zindexuje celou workspace složku. Posílá progress přes channel.
    pub async fn index(
        &self,
        root: &str,
        tx: mpsc::Sender<IndexProgress>,
    ) -> AppResult<()> {
        WorkspacePath::new(root)?;

        let entries = self.fs.list_recursive(root).await?;
        let files: Vec<_> = entries
            .iter()
            .filter(|e| matches!(e.kind, weave_domain::workspace::EntryKind::File))
            .collect();

        let total = files.len() as u64;
        let _ = tx.send(IndexProgress::Started { total }).await;

        let mut indexed = 0u64;
        let mut skipped = 0u64;

        self.repo.clear().await?;

        for entry in &files {
            let ext = std::path::Path::new(&entry.path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_string();

            let text_content = if IndexedFile::is_text_indexable(&ext) {
                match self.fs.read_text(&entry.path).await {
                    Ok(text) => {
                        // Limit na 512 KB aby SQLite nezahlcoval
                        if text.len() > 512 * 1024 {
                            text[..512 * 1024].to_string()
                        } else {
                            text
                        }
                    }
                    Err(_) => {
                        skipped += 1;
                        continue;
                    }
                }
            } else {
                skipped += 1;
                String::new()
            };

            let indexed_file = IndexedFile {
                path: entry.path.clone(),
                name: entry.name.clone(),
                extension: if ext.is_empty() { None } else { Some(ext) },
                size_bytes: entry.size_bytes.unwrap_or(0),
                modified_at: entry.modified_at.unwrap_or_else(chrono::Utc::now),
                indexed_at: chrono::Utc::now(),
                text_content,
            };

            self.repo.upsert_file(&indexed_file).await?;
            indexed += 1;

            let _ = tx
                .send(IndexProgress::File {
                    path: entry.path.clone(),
                    indexed,
                    total,
                })
                .await;
        }

        let _ = tx.send(IndexProgress::Done { indexed, skipped }).await;
        tracing::info!(%indexed, %skipped, "Workspace indexování dokončeno");
        Ok(())
    }

    /// Vrátí přímé potomky složky — pro lazy tree loading v UI.
    pub async fn list_children(&self, path: &str) -> AppResult<Vec<WorkspaceEntry>> {
        self.fs.list_children(path).await
    }

    /// Přečte obsah souboru.
    pub async fn read_file(&self, path: &str) -> AppResult<String> {
        self.fs.read_text(path).await
    }

    /// Zapíše obsah souboru a aktualizuje index.
    pub async fn write_file(&self, path: &str, content: &str) -> AppResult<()> {
        self.fs.write_text(path, content).await?;

        // Aktualizuj index
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string();

        let name = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        if IndexedFile::is_text_indexable(&ext) {
            let file = IndexedFile {
                path: path.to_string(),
                name,
                extension: if ext.is_empty() { None } else { Some(ext) },
                size_bytes: content.len() as u64,
                modified_at: chrono::Utc::now(),
                indexed_at: chrono::Utc::now(),
                text_content: content.to_string(),
            };
            self.repo.upsert_file(&file).await?;
        }

        Ok(())
    }

    /// Vytvoří soubor nebo složku.
    pub async fn create(&self, path: &str, is_dir: bool) -> AppResult<()> {
        self.fs.create(path, is_dir).await
    }

    /// Smaže soubor nebo složku a odebere z indexu.
    pub async fn delete(&self, path: &str) -> AppResult<()> {
        self.fs.delete(path).await?;
        self.repo.remove_file(path).await
    }

    /// Přejmenuje a aktualizuje index.
    pub async fn rename(&self, from: &str, to: &str) -> AppResult<()> {
        self.fs.rename(from, to).await?;
        self.repo.remove_file(from).await?;
        // Nový soubor bude indexován při dalším full-scan nebo lazy přístupu
        Ok(())
    }

    /// Full-text hledání ve workspace.
    pub async fn search(&self, query: &str, limit: u32) -> AppResult<Vec<IndexedFile>> {
        if query.trim().is_empty() {
            return Err(AppError::Domain(
                weave_domain::error::DomainError::InvalidArgument(
                    "Hledaný výraz nesmí být prázdný".into(),
                ),
            ));
        }
        self.repo.search(query, limit).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::workspace_port::{MockFileSystemPort, MockWorkspaceRepository};
    use weave_domain::workspace::{EntryKind, WorkspaceEntry};

    fn make_entry(path: &str, name: &str, kind: EntryKind) -> WorkspaceEntry {
        WorkspaceEntry {
            path: path.to_string(),
            name: name.to_string(),
            kind,
            size_bytes: Some(100),
            modified_at: Some(chrono::Utc::now()),
        }
    }

    #[tokio::test]
    async fn index_skips_binary_files() {
        let mut fs = MockFileSystemPort::new();
        let mut repo = MockWorkspaceRepository::new();

        fs.expect_list_recursive().returning(|_| {
            Ok(vec![
                make_entry("/ws/readme.md", "readme.md", EntryKind::File),
                make_entry("/ws/photo.png", "photo.png", EntryKind::File),
            ])
        });
        fs.expect_read_text()
            .times(1) // jen readme.md
            .returning(|_| Ok("# Hello".to_string()));

        repo.expect_clear().returning(|| Ok(()));
        repo.expect_upsert_file().times(1).returning(|_| Ok(()));

        let uc = WorkspaceUseCase::new(Arc::new(fs), Arc::new(repo));
        let (tx, _rx) = mpsc::channel(32);
        uc.index("/ws", tx).await.unwrap();
    }

    #[tokio::test]
    async fn search_rejects_empty_query() {
        let fs = MockFileSystemPort::new();
        let repo = MockWorkspaceRepository::new();
        let uc = WorkspaceUseCase::new(Arc::new(fs), Arc::new(repo));
        assert!(uc.search("", 10).await.is_err());
    }

    #[tokio::test]
    async fn write_file_updates_index_for_text() {
        let mut fs = MockFileSystemPort::new();
        let mut repo = MockWorkspaceRepository::new();

        fs.expect_write_text().times(1).returning(|_, _| Ok(()));
        repo.expect_upsert_file().times(1).returning(|_| Ok(()));

        let uc = WorkspaceUseCase::new(Arc::new(fs), Arc::new(repo));
        uc.write_file("/ws/notes.md", "# Obsah").await.unwrap();
    }
}
