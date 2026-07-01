use async_trait::async_trait;
use tokio::sync::mpsc;
use weave_application::{
    error::{AppError, AppResult},
    ports::model_manager_port::{
        DownloadProgress, GpuBackend, GpuInfo, LocalModel, ModelManagerPort,
    },
};

pub struct LocalModelManager {
    models_dir: std::path::PathBuf,
    http: reqwest::Client,
}

impl LocalModelManager {
    pub fn new(models_dir: impl Into<std::path::PathBuf>) -> Self {
        Self {
            models_dir: models_dir.into(),
            http: reqwest::Client::new(),
        }
    }

    fn manifest_path(&self) -> std::path::PathBuf {
        self.models_dir.join("manifest.json")
    }

    fn read_manifest(&self) -> AppResult<Vec<LocalModel>> {
        let path = self.manifest_path();
        if !path.exists() {
            return Ok(vec![]);
        }
        let data =
            std::fs::read_to_string(&path).map_err(|e| AppError::Repository(e.to_string()))?;
        serde_json::from_str(&data).map_err(|e| AppError::Repository(e.to_string()))
    }

    fn write_manifest(&self, models: &[LocalModel]) -> AppResult<()> {
        std::fs::create_dir_all(&self.models_dir)
            .map_err(|e| AppError::Repository(e.to_string()))?;
        let data = serde_json::to_string_pretty(models)
            .map_err(|e| AppError::Repository(e.to_string()))?;
        std::fs::write(self.manifest_path(), data).map_err(|e| AppError::Repository(e.to_string()))
    }
}

#[async_trait]
impl ModelManagerPort for LocalModelManager {
    async fn list_local(&self) -> AppResult<Vec<LocalModel>> {
        self.read_manifest()
    }

    async fn download(
        &self,
        model_id: &str,
        source_url: &str,
        tx: mpsc::Sender<DownloadProgress>,
    ) -> AppResult<()> {
        use futures_util::StreamExt;
        use std::io::Write;

        let response = self
            .http
            .get(source_url)
            .send()
            .await
            .map_err(|e| AppError::Repository(e.to_string()))?;

        let total = response.content_length().unwrap_or(0);
        let _ = tx
            .send(DownloadProgress::Started {
                model_id: model_id.to_string(),
                total_bytes: total,
            })
            .await;

        let dest = self.models_dir.join(format!("{model_id}.gguf"));
        std::fs::create_dir_all(&self.models_dir)
            .map_err(|e| AppError::Repository(e.to_string()))?;

        let mut file =
            std::fs::File::create(&dest).map_err(|e| AppError::Repository(e.to_string()))?;
        let mut downloaded = 0u64;
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|e| AppError::Repository(e.to_string()))?;
            file.write_all(&bytes)
                .map_err(|e| AppError::Repository(e.to_string()))?;
            downloaded += bytes.len() as u64;
            let _ = tx
                .send(DownloadProgress::Progress { downloaded, total })
                .await;
        }

        let _ = tx.send(DownloadProgress::Verifying).await;
        // TODO: SHA256 checksum ověření

        let model = LocalModel {
            id: model_id.to_string(),
            name: model_id.to_string(),
            version: "latest".to_string(),
            size_bytes: downloaded,
            path: dest.to_string_lossy().into_owned(),
            checksum: String::new(),
        };

        // Zapsat do manifestu, jinak by list_local model po restartu neviděl
        // (bug: download stahoval soubor, ale manifest.json se nikdy neaktualizoval).
        let mut models = self.read_manifest().unwrap_or_default();
        models.retain(|m| m.id != model.id);
        models.push(model.clone());
        self.write_manifest(&models)?;

        let _ = tx.send(DownloadProgress::Done { model }).await;
        Ok(())
    }

    async fn delete(&self, model_id: &str) -> AppResult<()> {
        let path = self.models_dir.join(format!("{model_id}.gguf"));
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| AppError::Repository(e.to_string()))?;
        }

        let mut models = self.read_manifest().unwrap_or_default();
        models.retain(|m| m.id != model_id);
        self.write_manifest(&models)?;
        Ok(())
    }

    async fn detect_gpu(&self) -> AppResult<Option<GpuInfo>> {
        // Detekce dle platformy — produkčně rozšíříme o nvml / metal query
        #[cfg(target_os = "macos")]
        return Ok(Some(GpuInfo {
            name: "Apple Silicon GPU".to_string(),
            vram_mb: 0,
            backend: GpuBackend::Metal,
        }));

        #[cfg(not(target_os = "macos"))]
        {
            // Zkusíme CUDA přes nvidia-smi
            if let Ok(output) = std::process::Command::new("nvidia-smi")
                .args(["--query-gpu=name,memory.total", "--format=csv,noheader"])
                .output()
            {
                if output.status.success() {
                    let info = String::from_utf8_lossy(&output.stdout);
                    let parts: Vec<&str> = info.trim().split(',').collect();
                    let name = parts
                        .first()
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default();
                    let vram = parts
                        .get(1)
                        .and_then(|s| s.split_whitespace().next())
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);
                    return Ok(Some(GpuInfo {
                        name,
                        vram_mb: vram,
                        backend: GpuBackend::Cuda,
                    }));
                }
            }
            Ok(None)
        }
    }

    async fn check_for_updates(&self) -> AppResult<Vec<String>> {
        // Placeholder — produkčně dotaz na HuggingFace API
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_manager() -> (LocalModelManager, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("weave_model_mgr_{}", uuid::Uuid::new_v4()));
        (LocalModelManager::new(dir.clone()), dir)
    }

    #[tokio::test]
    async fn list_local_is_empty_without_manifest() {
        let (mgr, dir) = tmp_manager();
        assert!(mgr.list_local().await.unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn write_then_read_manifest_roundtrips() {
        let (mgr, dir) = tmp_manager();

        // Regrese: download() dřív nikdy nezapisoval manifest.json, takže
        // list_local() po restartu appky staženého modelu neviděl.
        let model = LocalModel {
            id: "test-model".into(),
            name: "test-model".into(),
            version: "latest".into(),
            size_bytes: 1234,
            path: dir.join("test-model.gguf").to_string_lossy().into_owned(),
            checksum: String::new(),
        };
        mgr.write_manifest(std::slice::from_ref(&model)).unwrap();

        let loaded = mgr.list_local().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "test-model");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn delete_removes_from_manifest() {
        let (mgr, dir) = tmp_manager();
        let model = LocalModel {
            id: "to-delete".into(),
            name: "to-delete".into(),
            version: "latest".into(),
            size_bytes: 1,
            path: dir.join("to-delete.gguf").to_string_lossy().into_owned(),
            checksum: String::new(),
        };
        mgr.write_manifest(&[model]).unwrap();
        assert_eq!(mgr.list_local().await.unwrap().len(), 1);

        mgr.delete("to-delete").await.unwrap();
        assert!(mgr.list_local().await.unwrap().is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
