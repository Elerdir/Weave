use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::AppResult;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredImage {
    pub path: String,
    pub mime: String,
}

/// Ukládá uživatelem vybrané soubory (např. referenční obrázky pro generování)
/// do úložiště spravovaného appkou, aby zůstaly dostupné i po přesunutí/smazání originálu.
#[async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait AttachmentStorePort: Send + Sync {
    async fn store_reference_image(&self, source_path: &str) -> AppResult<StoredImage>;
}
