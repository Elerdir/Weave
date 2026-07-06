use async_trait::async_trait;
use tokio::sync::mpsc;
use weave_application::{
    error::{AppError, AppResult},
    ports::model_manager_port::{
        DownloadProgress, GpuBackend, GpuInfo, LocalModel, ModelManagerPort,
    },
};

pub struct LocalModelManager {
    models_dir: std::sync::RwLock<std::path::PathBuf>,
    http: reqwest::Client,
}

impl LocalModelManager {
    pub fn new(models_dir: impl Into<std::path::PathBuf>) -> Self {
        Self {
            models_dir: std::sync::RwLock::new(models_dir.into()),
            http: reqwest::Client::new(),
        }
    }

    fn dir(&self) -> std::path::PathBuf {
        self.models_dir
            .read()
            .expect("models_dir lock poisoned")
            .clone()
    }

    fn manifest_path(&self) -> std::path::PathBuf {
        self.dir().join("manifest.json")
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
        let dir = self.dir();
        std::fs::create_dir_all(&dir).map_err(|e| AppError::Repository(e.to_string()))?;
        let data = serde_json::to_string_pretty(models)
            .map_err(|e| AppError::Repository(e.to_string()))?;
        std::fs::write(dir.join("manifest.json"), data)
            .map_err(|e| AppError::Repository(e.to_string()))
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
        // Odhad velikosti jen pro "Started" event — samotné stahování (níže) si
        // Content-Length/Accept-Ranges zjišťuje znovu, ať zvolí sekvenční nebo
        // paralelní (segmentovaný) režim.
        let total_hint = self
            .http
            .head(source_url)
            .send()
            .await
            .ok()
            .and_then(|r| r.content_length())
            .unwrap_or(0);
        let _ = tx
            .send(DownloadProgress::Started {
                model_id: model_id.to_string(),
                total_bytes: total_hint,
            })
            .await;

        let dir = self.dir();
        let dest = dir.join(format!("{model_id}.gguf"));
        std::fs::create_dir_all(&dir).map_err(|e| AppError::Repository(e.to_string()))?;

        // Jedno TCP spojení bývá na CDN throughput-limitované výrazně pod reálnou
        // šířku pásma — u velkých souborů s podporou HTTP Range se proto stahuje
        // paralelně přes víc segmentů (viz `parallel_download`). Progress se
        // reportuje throttlovaně (~5x/s); `try_send` místo `send().await`, aby šlo
        // volat i ze sync callbacku volaného souběžnými segmenty.
        let progress_tx = tx.clone();
        let downloaded = crate::parallel_download::download(
            &self.http,
            source_url,
            &dest,
            move |downloaded, total| {
                let _ = progress_tx.try_send(DownloadProgress::Progress { downloaded, total });
            },
        )
        .await
        .map_err(AppError::Repository)?;

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
        let path = self.dir().join(format!("{model_id}.gguf"));
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
            // Unified memory — volnou VRAM přes nvidia-smi zjistit nejde a
            // nemáme (zatím) jinou metodu, 0 = "neznámá" (viz recommend_gpu_layers).
            free_vram_mb: 0,
            backend: GpuBackend::Metal,
        }));

        #[cfg(not(target_os = "macos"))]
        {
            // Zkusíme CUDA přes nvidia-smi
            if let Ok(output) = std::process::Command::new("nvidia-smi")
                .args([
                    "--query-gpu=name,memory.total,memory.free",
                    "--format=csv,noheader",
                ])
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
                    let free_vram = parts
                        .get(2)
                        .and_then(|s| s.split_whitespace().next())
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);
                    return Ok(Some(GpuInfo {
                        name,
                        vram_mb: vram,
                        free_vram_mb: free_vram,
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

    async fn models_dir(&self) -> AppResult<std::path::PathBuf> {
        Ok(self.dir())
    }

    async fn set_models_dir(&self, new_dir: std::path::PathBuf) -> AppResult<()> {
        let old_dir = self.dir();
        if old_dir == new_dir {
            return Ok(());
        }

        std::fs::create_dir_all(&new_dir).map_err(|e| AppError::Repository(e.to_string()))?;

        // Existující modely (a manifest) přesuneme, ať uživatel po přepnutí
        // složky nemusí nic stahovat znovu. `rename` funguje jen v rámci
        // stejného disku — přes disky (C: → D:) je potřeba kopie + smazání.
        if old_dir.exists() {
            for entry in
                std::fs::read_dir(&old_dir).map_err(|e| AppError::Repository(e.to_string()))?
            {
                let entry = entry.map_err(|e| AppError::Repository(e.to_string()))?;
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let dest = new_dir.join(entry.file_name());
                if std::fs::rename(&path, &dest).is_err() {
                    std::fs::copy(&path, &dest).map_err(|e| AppError::Repository(e.to_string()))?;
                    std::fs::remove_file(&path).map_err(|e| AppError::Repository(e.to_string()))?;
                }
            }
        }

        *self.models_dir.write().expect("models_dir lock poisoned") = new_dir;
        Ok(())
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
    async fn set_models_dir_moves_existing_models_and_manifest() {
        let (mgr, old_dir) = tmp_manager();
        let new_dir =
            std::env::temp_dir().join(format!("weave_model_mgr_new_{}", uuid::Uuid::new_v4()));

        let model = LocalModel {
            id: "movable".into(),
            name: "movable".into(),
            version: "latest".into(),
            size_bytes: 5,
            path: old_dir.join("movable.gguf").to_string_lossy().into_owned(),
            checksum: String::new(),
        };
        mgr.write_manifest(std::slice::from_ref(&model)).unwrap();
        std::fs::write(old_dir.join("movable.gguf"), b"hello").unwrap();

        mgr.set_models_dir(new_dir.clone()).await.unwrap();

        assert_eq!(mgr.models_dir().await.unwrap(), new_dir);
        assert!(new_dir.join("movable.gguf").exists());
        assert!(new_dir.join("manifest.json").exists());
        assert!(!old_dir.join("movable.gguf").exists());

        let loaded = mgr.list_local().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "movable");

        let _ = std::fs::remove_dir_all(&old_dir);
        let _ = std::fs::remove_dir_all(&new_dir);
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
