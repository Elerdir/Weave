use async_trait::async_trait;
use std::path::Path;
use weave_application::{
    error::{AppError, AppResult},
    ports::workspace_port::FileSystemPort,
};
use weave_domain::workspace::{EntryKind, WorkspaceEntry};

pub struct NativeFileSystem;

#[async_trait]
impl FileSystemPort for NativeFileSystem {
    async fn list_recursive(&self, root: &str) -> AppResult<Vec<WorkspaceEntry>> {
        let root_path = Path::new(root);
        if !root_path.exists() {
            return Err(AppError::Repository(format!("Složka neexistuje: {root}")));
        }

        let mut entries = Vec::new();
        collect_recursive(root_path, &mut entries).map_err(|e| AppError::Repository(e.to_string()))?;
        Ok(entries)
    }

    async fn list_children(&self, path: &str) -> AppResult<Vec<WorkspaceEntry>> {
        let dir = Path::new(path);
        if !dir.is_dir() {
            return Err(AppError::Repository(format!("Není složka: {path}")));
        }

        let mut entries = Vec::new();
        let read_dir = std::fs::read_dir(dir).map_err(|e| AppError::Repository(e.to_string()))?;

        for entry in read_dir.flatten() {
            if let Some(e) = fs_entry_to_domain(&entry) {
                entries.push(e);
            }
        }

        // Složky první, pak soubory, abecedně
        entries.sort_by(|a, b| {
            match (&a.kind, &b.kind) {
                (EntryKind::Directory, EntryKind::File) => std::cmp::Ordering::Less,
                (EntryKind::File, EntryKind::Directory) => std::cmp::Ordering::Greater,
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            }
        });

        Ok(entries)
    }

    async fn read_text(&self, path: &str) -> AppResult<String> {
        std::fs::read_to_string(path).map_err(|e| AppError::Repository(e.to_string()))
    }

    async fn write_text(&self, path: &str, content: &str) -> AppResult<()> {
        // Vytvoř parent složky pokud neexistují
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::Repository(e.to_string()))?;
        }
        std::fs::write(path, content).map_err(|e| AppError::Repository(e.to_string()))
    }

    async fn create(&self, path: &str, is_dir: bool) -> AppResult<()> {
        if is_dir {
            std::fs::create_dir_all(path).map_err(|e| AppError::Repository(e.to_string()))
        } else {
            if let Some(parent) = Path::new(path).parent() {
                std::fs::create_dir_all(parent).map_err(|e| AppError::Repository(e.to_string()))?;
            }
            std::fs::File::create(path)
                .map(|_| ())
                .map_err(|e| AppError::Repository(e.to_string()))
        }
    }

    async fn delete(&self, path: &str) -> AppResult<()> {
        let p = Path::new(path);
        if p.is_dir() {
            std::fs::remove_dir_all(path).map_err(|e| AppError::Repository(e.to_string()))
        } else {
            std::fs::remove_file(path).map_err(|e| AppError::Repository(e.to_string()))
        }
    }

    async fn rename(&self, from: &str, to: &str) -> AppResult<()> {
        std::fs::rename(from, to).map_err(|e| AppError::Repository(e.to_string()))
    }
}

fn collect_recursive(dir: &Path, out: &mut Vec<WorkspaceEntry>) -> std::io::Result<()> {
    // Přeskočíme skryté složky a node_modules/target/.git
    let skip_dirs = [".git", "node_modules", "target", ".pnpm-store", "__pycache__", ".venv"];

    for entry in std::fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() && skip_dirs.contains(&name.as_str()) {
            continue;
        }

        if let Some(ws_entry) = fs_entry_to_domain(&entry) {
            out.push(ws_entry);
        }

        if path.is_dir() {
            collect_recursive(&path, out)?;
        }
    }
    Ok(())
}

fn fs_entry_to_domain(entry: &std::fs::DirEntry) -> Option<WorkspaceEntry> {
    let path = entry.path();
    let name = entry.file_name().to_string_lossy().to_string();
    let meta = entry.metadata().ok()?;

    let modified_at = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| {
            chrono::DateTime::from_timestamp(d.as_secs() as i64, 0).unwrap_or_default()
        });

    Some(WorkspaceEntry {
        path: path.to_string_lossy().into_owned(),
        name,
        kind: if meta.is_dir() { EntryKind::Directory } else { EntryKind::File },
        size_bytes: if meta.is_file() { Some(meta.len()) } else { None },
        modified_at,
    })
}
