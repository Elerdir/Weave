use std::path::{Path, PathBuf};

use async_trait::async_trait;
use uuid::Uuid;
use weave_application::{
    error::{AppError, AppResult},
    ports::attachment_store_port::{AttachmentStorePort, StoredImage},
};

/// Kopíruje uživatelem vybrané referenční obrázky do vlastní složky appky,
/// aby zůstaly dostupné i po přesunutí/smazání originálního souboru.
pub struct LocalAttachmentStore {
    dir: PathBuf,
}

impl LocalAttachmentStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        let _ = std::fs::create_dir_all(&dir);
        Self { dir }
    }
}

fn mime_for_extension(ext: &str) -> Option<&'static str> {
    match ext.to_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        "bmp" => Some("image/bmp"),
        _ => None,
    }
}

#[async_trait]
impl AttachmentStorePort for LocalAttachmentStore {
    async fn store_reference_image(&self, source_path: &str) -> AppResult<StoredImage> {
        let source = Path::new(source_path);
        let ext = source.extension().and_then(|e| e.to_str()).unwrap_or("");
        let mime = mime_for_extension(ext).ok_or_else(|| {
            AppError::Attachment(format!("Nepodporovaný typ obrázku: .{ext}"))
        })?;

        let dest = self.dir.join(format!("{}.{}", Uuid::new_v4(), ext.to_lowercase()));

        tokio::fs::copy(source, &dest)
            .await
            .map_err(|e| AppError::Attachment(format!("Kopírování obrázku selhalo: {e}")))?;

        Ok(StoredImage {
            path: dest.to_string_lossy().into_owned(),
            mime: mime.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("weave_attach_test_{name}_{}", Uuid::new_v4()))
    }

    async fn write_source_file(name: &str, content: &[u8]) -> PathBuf {
        let dir = unique_temp_dir(&format!("src_{name}"));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        tokio::fs::write(&path, content).await.unwrap();
        path
    }

    #[tokio::test]
    async fn stores_png_with_correct_mime() {
        let source = write_source_file("photo.png", b"fake-png-bytes").await;
        let store = LocalAttachmentStore::new(unique_temp_dir("dest"));

        let stored = store
            .store_reference_image(source.to_str().unwrap())
            .await
            .unwrap();

        assert_eq!(stored.mime, "image/png");
        assert!(stored.path.ends_with(".png"));
        assert!(std::path::Path::new(&stored.path).exists());
    }

    #[tokio::test]
    async fn stores_jpeg_variants_with_correct_mime() {
        let source = write_source_file("photo.JPG", b"fake-jpg-bytes").await;
        let store = LocalAttachmentStore::new(unique_temp_dir("dest"));

        let stored = store
            .store_reference_image(source.to_str().unwrap())
            .await
            .unwrap();

        assert_eq!(stored.mime, "image/jpeg");
    }

    #[tokio::test]
    async fn rejects_unsupported_extension() {
        let source = write_source_file("document.pdf", b"fake-pdf-bytes").await;
        let store = LocalAttachmentStore::new(unique_temp_dir("dest"));

        let result = store.store_reference_image(source.to_str().unwrap()).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn each_stored_copy_gets_a_unique_path() {
        let source = write_source_file("photo.png", b"fake-png-bytes").await;
        let store = LocalAttachmentStore::new(unique_temp_dir("dest"));

        let first = store
            .store_reference_image(source.to_str().unwrap())
            .await
            .unwrap();
        let second = store
            .store_reference_image(source.to_str().unwrap())
            .await
            .unwrap();

        assert_ne!(first.path, second.path);
    }
}
