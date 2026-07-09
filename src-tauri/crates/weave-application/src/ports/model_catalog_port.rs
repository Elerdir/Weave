use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::AppResult;

/// Model nalezený ve vzdáleném katalogu (HuggingFace Hub).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogModel {
    /// Plné repo id, např. `bartowski/Meta-Llama-3.1-8B-Instruct-GGUF`.
    pub repo_id: String,
    /// Autor/organizace (část před lomítkem).
    pub author: String,
    /// Název repa (část za lomítkem).
    pub name: String,
    pub downloads: u64,
    pub likes: u64,
    /// Gated repo (Llama apod.) — stažení vyžaduje HF token se schváleným přístupem.
    pub gated: bool,
}

/// Jeden GGUF soubor uvnitř repa — typicky jedna kvantizace modelu.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogFile {
    /// Cesta souboru v repu, např. `Meta-Llama-3.1-8B-Instruct-Q4_K_M.gguf`.
    pub file_name: String,
    pub size_bytes: u64,
    /// Kvantizace vyčtená z názvu (`Q4_K_M`, `IQ2_XS`, `F16`…), pokud šla poznat.
    pub quant: Option<String>,
    /// Přímá URL ke stažení (`…/resolve/main/<path>`).
    pub download_url: String,
    /// SHA256 souboru z HF LFS metadat (`lfs.oid`) — ověří se po stažení.
    pub sha256: Option<String>,
}

/// Vyhledávání modelů ve vzdáleném katalogu (HuggingFace) — jen čtení,
/// stahování řeší `ModelManagerPort`.
#[async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait ModelCatalogPort: Send + Sync {
    /// Fulltext hledání GGUF repozitářů, seřazeno podle počtu stažení.
    /// `token` = HF token (gated repa a vyšší rate limity); bez něj funguje taky.
    async fn search(
        &self,
        query: &str,
        token: Option<&str>,
        limit: u32,
    ) -> AppResult<Vec<CatalogModel>>;

    /// Vypíše GGUF soubory (kvantizace) v repu, seřazené podle velikosti.
    async fn list_gguf_files(
        &self,
        repo_id: &str,
        token: Option<&str>,
    ) -> AppResult<Vec<CatalogFile>>;
}
