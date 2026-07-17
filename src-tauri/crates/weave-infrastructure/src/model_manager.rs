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
        expected_sha256: Option<String>,
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
        //
        // Stahuje se do `.part` a až po úspěchu přejmenuje — segmentované stahování
        // soubor předalokuje na plnou velikost, takže napůl stažený soubor by jinak
        // vypadal jako hotový model (a llama.cpp by na něm spadl).
        let tmp_dest = dir.join(format!("{model_id}.gguf.part"));
        let progress_tx = tx.clone();
        let downloaded = crate::parallel_download::download(
            &self.http,
            source_url,
            &tmp_dest,
            move |downloaded, total| {
                let _ = progress_tx.try_send(DownloadProgress::Progress { downloaded, total });
            },
        )
        .await
        .map_err(AppError::Repository)?;

        // Ověření SHA256 (je-li znám, např. z HF `lfs.oid`) — JEŠTĚ na `.part`
        // souboru, aby vadný obsah nikdy nedostal finální jméno modelu.
        let _ = tx.send(DownloadProgress::Verifying).await;
        let mut checksum = String::new();
        if let Some(expected) = expected_sha256
            .as_deref()
            .map(|s| s.trim().trim_start_matches("sha256:").to_ascii_lowercase())
            .filter(|s| !s.is_empty())
        {
            let actual = sha256_of_file(&tmp_dest).await?;
            if actual != expected {
                let _ = std::fs::remove_file(&tmp_dest);
                return Err(AppError::Repository(format!(
                    "Stažený soubor je poškozený: SHA256 nesouhlasí \
                     (očekáváno {expected}, staženo {actual}). Soubor byl smazán — zkus to znovu."
                )));
            }
            checksum = actual;
        }
        std::fs::rename(&tmp_dest, &dest).map_err(|e| AppError::Repository(e.to_string()))?;

        let model = LocalModel {
            id: model_id.to_string(),
            name: model_id.to_string(),
            version: "latest".to_string(),
            size_bytes: downloaded,
            path: dest.to_string_lossy().into_owned(),
            checksum,
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
        #[cfg(target_os = "macos")]
        {
            Ok(Some(detect_apple_gpu()))
        }

        #[cfg(not(target_os = "macos"))]
        {
            if let Some(info) = detect_nvidia_gpu() {
                return Ok(Some(info));
            }
            // Ne-NVIDIA GPU (AMD/Intel) — aspoň jméno adaptéru, aby wizard
            // neukázal "GPU nenalezena"; akcelerace pak jde přes llm-vulkan.
            #[cfg(windows)]
            if let Some(info) = detect_windows_gpu_fallback() {
                return Ok(Some(info));
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

/// SHA256 souboru (hex, lowercase). Hashuje se streamovaně po 1 MB blocích
/// ve `spawn_blocking` — modely mají jednotky až desítky GB a nesmí blokovat
/// async runtime.
async fn sha256_of_file(path: &std::path::Path) -> AppResult<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut file =
            std::fs::File::open(&path).map_err(|e| AppError::Repository(e.to_string()))?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 1024 * 1024];
        loop {
            let n = file
                .read(&mut buf)
                .map_err(|e| AppError::Repository(e.to_string()))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Ok(hex::encode(hasher.finalize()))
    })
    .await
    .map_err(|e| AppError::Repository(e.to_string()))?
}

/// macOS: jméno čipu a velikost unified memory přes `sysctl`. Metal sdílí
/// paměť s CPU, takže jako "VRAM" hlásíme celou unified memory; kolik z ní
/// je volné, systém jednoduše zjistit nedá (0 = neznámá, viz
/// `recommend_gpu_layers` — při neznámé volné VRAM se offloadují všechny vrstvy).
#[cfg(target_os = "macos")]
fn detect_apple_gpu() -> GpuInfo {
    let brand = sysctl_value("machdep.cpu.brand_string");
    let memsize = sysctl_value("hw.memsize").and_then(|s| s.parse::<u64>().ok());
    apple_gpu_info(brand.as_deref(), memsize)
}

#[cfg(target_os = "macos")]
fn sysctl_value(key: &str) -> Option<String> {
    let mut cmd = std::process::Command::new("sysctl");
    cmd.args(["-n", key]);
    crate::spawn::hide_console_std(&mut cmd);
    cmd.output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Čistá část Apple detekce — oddělená kvůli testům (běží i mimo macOS).
pub fn apple_gpu_info(brand: Option<&str>, memsize_bytes: Option<u64>) -> GpuInfo {
    GpuInfo {
        name: brand
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("Apple Silicon GPU")
            .to_string(),
        vram_mb: memsize_bytes.unwrap_or(0) / (1024 * 1024),
        free_vram_mb: 0,
        backend: GpuBackend::Metal,
    }
}

/// NVIDIA GPU přes `nvidia-smi` (Windows/Linux).
#[cfg(not(target_os = "macos"))]
fn detect_nvidia_gpu() -> Option<GpuInfo> {
    let mut cmd = std::process::Command::new("nvidia-smi");
    cmd.args([
        "--query-gpu=name,memory.total,memory.free",
        "--format=csv,noheader",
    ]);
    crate::spawn::hide_console_std(&mut cmd);
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    // Multi-GPU stroj: bereme první (primární) kartu.
    parse_nvidia_smi_line(text.lines().next()?)
}

/// Parsuje řádek `nvidia-smi --query-gpu=name,memory.total,memory.free
/// --format=csv,noheader`, např. `NVIDIA GeForce RTX 3090, 24576 MiB, 20143 MiB`.
pub fn parse_nvidia_smi_line(line: &str) -> Option<GpuInfo> {
    let parts: Vec<&str> = line.trim().split(',').collect();
    let name = parts
        .first()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())?
        .to_string();
    let mb = |i: usize| {
        parts
            .get(i)
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0)
    };
    Some(GpuInfo {
        name,
        vram_mb: mb(1),
        free_vram_mb: mb(2),
        backend: GpuBackend::Cuda,
    })
}

/// Windows fallback pro ne-NVIDIA GPU (AMD/Intel): jméno adaptéru z WMI.
/// VRAM přes WMI spolehlivě nejde (AdapterRAM je 32bit, ořezává na 4 GB),
/// takže hlásíme jen jméno; backend odpovídá `llm-vulkan` akceleraci.
#[cfg(windows)]
fn detect_windows_gpu_fallback() -> Option<GpuInfo> {
    let mut cmd = std::process::Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-Command",
        "(Get-CimInstance Win32_VideoController).Name",
    ]);
    crate::spawn::hide_console_std(&mut cmd);
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    fallback_gpu_from_names(&String::from_utf8_lossy(&output.stdout))
}

/// Vybere první skutečný adaptér (přeskočí virtuální/softwarové) a určí
/// backend podle výrobce — NVIDIA bez funkčního nvidia-smi je pořád CUDA.
pub fn fallback_gpu_from_names(output: &str) -> Option<GpuInfo> {
    let name = output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .find(|l| {
            let low = l.to_lowercase();
            !low.contains("microsoft basic") && !low.contains("remote display")
        })?
        .to_string();
    let backend = if name.to_lowercase().contains("nvidia") {
        GpuBackend::Cuda
    } else {
        GpuBackend::Vulkan
    };
    Some(GpuInfo {
        name,
        vram_mb: 0,
        free_vram_mb: 0,
        backend,
    })
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
    async fn download_verifies_sha256_and_fills_manifest_checksum() {
        use sha2::{Digest, Sha256};
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let body = b"gguf-fake-content".to_vec();
        let expected = hex::encode(Sha256::digest(&body));
        Mock::given(method("HEAD"))
            .respond_with(
                ResponseTemplate::new(200).insert_header("content-length", body.len().to_string()),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
            .mount(&server)
            .await;

        let (mgr, dir) = tmp_manager();
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });

        // Prefix "sha256:" musí projít taky (formát z HF lfs.oid)
        mgr.download(
            "checked",
            &server.uri(),
            Some(format!("sha256:{expected}")),
            tx,
        )
        .await
        .unwrap();
        let _ = drain.await;

        assert!(dir.join("checked.gguf").exists());
        let manifest = mgr.list_local().await.unwrap();
        assert_eq!(
            manifest[0].checksum, expected,
            "checksum patří do manifestu"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn download_rejects_sha256_mismatch_and_removes_file() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("HEAD"))
            .respond_with(ResponseTemplate::new(200).insert_header("content-length", "9"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"tampered!".to_vec()))
            .mount(&server)
            .await;

        let (mgr, dir) = tmp_manager();
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });

        let err = mgr
            .download("bad", &server.uri(), Some("0".repeat(64)), tx)
            .await
            .unwrap_err()
            .to_string();
        let _ = drain.await;

        assert!(err.contains("SHA256"), "chyba má zmínit checksum: {err}");
        // Vadný soubor nesmí zůstat pod finálním ani .part jménem a nesmí
        // se dostat do manifestu.
        assert!(!dir.join("bad.gguf").exists());
        assert!(!dir.join("bad.gguf.part").exists());
        assert!(mgr.list_local().await.unwrap().is_empty());
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

    #[test]
    fn parse_nvidia_smi_line_extracts_name_and_memory() {
        let info = parse_nvidia_smi_line("NVIDIA GeForce RTX 3090, 24576 MiB, 20143 MiB").unwrap();
        assert_eq!(info.name, "NVIDIA GeForce RTX 3090");
        assert_eq!(info.vram_mb, 24576);
        assert_eq!(info.free_vram_mb, 20143);
        assert!(matches!(info.backend, GpuBackend::Cuda));
    }

    #[test]
    fn parse_nvidia_smi_line_tolerates_missing_memory_columns() {
        let info = parse_nvidia_smi_line("NVIDIA T4").unwrap();
        assert_eq!(info.name, "NVIDIA T4");
        assert_eq!(info.vram_mb, 0);
        assert_eq!(info.free_vram_mb, 0);
    }

    #[test]
    fn parse_nvidia_smi_line_rejects_empty_input() {
        assert!(parse_nvidia_smi_line("").is_none());
        assert!(parse_nvidia_smi_line("   ").is_none());
    }

    #[test]
    fn apple_gpu_info_converts_memsize_and_uses_brand() {
        let info = apple_gpu_info(Some("Apple M2 Max"), Some(32 * 1024 * 1024 * 1024));
        assert_eq!(info.name, "Apple M2 Max");
        assert_eq!(info.vram_mb, 32 * 1024);
        assert_eq!(info.free_vram_mb, 0);
        assert!(matches!(info.backend, GpuBackend::Metal));
    }

    #[test]
    fn apple_gpu_info_falls_back_when_sysctl_unavailable() {
        let info = apple_gpu_info(None, None);
        assert_eq!(info.name, "Apple Silicon GPU");
        assert_eq!(info.vram_mb, 0);
    }

    #[test]
    fn fallback_gpu_skips_virtual_adapters_and_detects_vendor() {
        let amd = fallback_gpu_from_names("Microsoft Basic Display Adapter\nAMD Radeon RX 7800 XT")
            .unwrap();
        assert_eq!(amd.name, "AMD Radeon RX 7800 XT");
        assert!(matches!(amd.backend, GpuBackend::Vulkan));

        let nv = fallback_gpu_from_names("NVIDIA GeForce GTX 1660\n").unwrap();
        assert!(matches!(nv.backend, GpuBackend::Cuda));

        assert!(fallback_gpu_from_names("Microsoft Basic Display Adapter\n").is_none());
        assert!(fallback_gpu_from_names("").is_none());
    }
}
