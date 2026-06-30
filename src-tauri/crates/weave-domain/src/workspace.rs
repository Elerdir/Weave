use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::DomainError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkspacePath(String);

impl WorkspacePath {
    pub fn new(path: impl Into<String>) -> Result<Self, DomainError> {
        let path = path.into();
        if path.trim().is_empty() {
            return Err(DomainError::InvalidArgument("Cesta nesmí být prázdná".into()));
        }
        Ok(Self(path))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Položka ve workspace — soubor nebo složka.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceEntry {
    pub path: String,
    pub name: String,
    pub kind: EntryKind,
    pub size_bytes: Option<u64>,
    pub modified_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Directory,
}

/// Indexovaný soubor — metadata + extrahovaný text pro FTS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedFile {
    pub path: String,
    pub name: String,
    pub extension: Option<String>,
    pub size_bytes: u64,
    pub modified_at: DateTime<Utc>,
    pub indexed_at: DateTime<Utc>,
    /// Extrahovaný plaintext (pro FTS). Prázdný u binárních souborů.
    pub text_content: String,
}

impl IndexedFile {
    pub fn is_text_indexable(extension: &str) -> bool {
        matches!(
            extension.to_lowercase().as_str(),
            "txt" | "md" | "markdown" | "rs" | "ts" | "tsx" | "js" | "jsx"
                | "svelte" | "vue" | "html" | "css" | "scss" | "json" | "toml"
                | "yaml" | "yml" | "xml" | "csv" | "py" | "go" | "java" | "c"
                | "cpp" | "h" | "hpp" | "sh" | "bash" | "zsh" | "sql" | "graphql"
                | "env" | "gitignore" | "dockerfile" | "lock"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_path_is_rejected() {
        assert!(WorkspacePath::new("").is_err());
        assert!(WorkspacePath::new("   ").is_err());
    }

    #[test]
    fn valid_path_is_accepted() {
        let p = WorkspacePath::new("/home/user/projects").unwrap();
        assert_eq!(p.as_str(), "/home/user/projects");
    }

    #[test]
    fn text_indexable_extensions() {
        assert!(IndexedFile::is_text_indexable("rs"));
        assert!(IndexedFile::is_text_indexable("MD"));
        assert!(IndexedFile::is_text_indexable("svelte"));
        assert!(!IndexedFile::is_text_indexable("png"));
        assert!(!IndexedFile::is_text_indexable("exe"));
        assert!(!IndexedFile::is_text_indexable("mp4"));
    }
}
