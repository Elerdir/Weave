mod process;

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::process::Child;
use tokio::sync::{mpsc, Mutex};
use weave_application::{
    error::{AppError, AppResult},
    ports::comfy_installer_port::{ComfyInstallerPort, ComfyStatus, InstallProgress},
    ports::image_gen_port::StylePreset,
};

use process::{
    download_file, extract_zip, find_system_python, has_nvidia_gpu, run_streamed, venv_python_path,
};

const COMFYUI_REPO: &str = "https://github.com/comfyanonymous/ComfyUI.git";
const PULID_REPO: &str = "https://github.com/cubiq/PuLID_ComfyUI.git";
pub const COMFYUI_DEFAULT_PORT: u16 = 8188;

/// Základní SDXL checkpoint — používá ho text-to-image i PuLID větev workflow.
/// `cubiq/PuLID_ComfyUI` patchuje SDXL cross-attention, ne FLUX/DiT. SDXL base
/// je navíc na HuggingFace veřejný bez přihlášení, což FLUX.1-dev/schnell
/// nejsou (obě vrací 401 bez auth tokenu) — jednoklikový no-auth download
/// stylem `recommended_models.rs` by u FLUX nešel udělat.
pub const SDXL_CHECKPOINT_FILENAME: &str = "sd_xl_base_1.0.safetensors";
const SDXL_CHECKPOINT_URL: &str = "https://huggingface.co/stabilityai/stable-diffusion-xl-base-1.0/resolve/main/sd_xl_base_1.0.safetensors";

/// Anime checkpoint (SDXL architektura → funguje i s PuLID). Stahuje se až
/// na vyžádání — když klasifikace promptu vyhodnotí anime styl.
pub const ANIME_CHECKPOINT_FILENAME: &str = "animagine-xl-3.1.safetensors";
const ANIME_CHECKPOINT_URL: &str = "https://huggingface.co/cagliostrolab/animagine-xl-3.1/resolve/main/animagine-xl-3.1.safetensors";

/// Který checkpoint použít pro daný styl. Realistic/Artistic/ThreeD jede na
/// SDXL base (styl se řídí promptem), Anime má vlastní doladěný checkpoint.
pub fn checkpoint_filename_for_style(style: StylePreset) -> &'static str {
    match style {
        StylePreset::Anime => ANIME_CHECKPOINT_FILENAME,
        _ => SDXL_CHECKPOINT_FILENAME,
    }
}

fn checkpoint_url_for_style(style: StylePreset) -> &'static str {
    match style {
        StylePreset::Anime => ANIME_CHECKPOINT_URL,
        _ => SDXL_CHECKPOINT_URL,
    }
}

/// PuLID váhy pro `PulidModelLoader` — konverze do IPAdapter formátu od
/// huchenlei, přesně ta, na kterou odkazuje README `cubiq/PuLID_ComfyUI`.
pub const PULID_WEIGHTS_FILENAME: &str = "ip-adapter_pulid_sdxl_fp16.safetensors";
const PULID_WEIGHTS_URL: &str = "https://huggingface.co/huchenlei/ipadapter_pulid/resolve/main/ip-adapter_pulid_sdxl_fp16.safetensors";

/// InsightFace AntelopeV2 pro `PulidInsightFaceLoader` — bez něj PuLID nemá
/// jak analyzovat obličej na referenčním obrázku. Archiv už obsahuje kořenovou
/// složku `antelopev2/`, takže se rozbaluje přímo do `models/insightface/models/`.
const ANTELOPEV2_ZIP_URL: &str =
    "https://huggingface.co/MonsterMMORPG/tools/resolve/main/antelopev2.zip";

pub struct LocalComfyInstaller {
    install_dir: PathBuf,
    server: Arc<Mutex<Option<Child>>>,
    http: reqwest::Client,
}

impl LocalComfyInstaller {
    pub fn new(install_dir: impl Into<PathBuf>) -> Self {
        Self {
            install_dir: install_dir.into(),
            server: Arc::new(Mutex::new(None)),
            http: reqwest::Client::new(),
        }
    }

    fn venv_dir(&self) -> PathBuf {
        self.install_dir.join("venv")
    }

    fn custom_nodes_dir(&self) -> PathBuf {
        self.install_dir.join("custom_nodes")
    }

    fn pulid_dir(&self) -> PathBuf {
        self.custom_nodes_dir().join("PuLID_ComfyUI")
    }

    fn checkpoints_dir(&self) -> PathBuf {
        self.install_dir.join("models").join("checkpoints")
    }

    /// Složka pro PuLID *váhy* (`models/pulid/`) — nezaměňovat s [`Self::pulid_dir`],
    /// což je adresář se zdrojovým kódem custom node uzlu.
    fn pulid_weights_dir(&self) -> PathBuf {
        self.install_dir.join("models").join("pulid")
    }

    /// Rodič pro `antelopev2/` — samotná složka modelu vznikne rozbalením zipu.
    fn insightface_models_dir(&self) -> PathBuf {
        self.install_dir
            .join("models")
            .join("insightface")
            .join("models")
    }

    async fn step(tx: &mpsc::Sender<InstallProgress>, name: &str) {
        let _ = tx
            .send(InstallProgress::Step {
                name: name.to_string(),
            })
            .await;
    }
}

#[async_trait]
impl ComfyInstallerPort for LocalComfyInstaller {
    async fn status(&self) -> AppResult<ComfyStatus> {
        if self.server.lock().await.is_some() {
            return Ok(ComfyStatus::Running);
        }
        let main_py = self.install_dir.join("main.py");
        if main_py.exists() && venv_python_path(&self.venv_dir()).exists() {
            Ok(ComfyStatus::Installed)
        } else {
            Ok(ComfyStatus::NotInstalled)
        }
    }

    async fn install(&self, tx: mpsc::Sender<InstallProgress>) -> AppResult<()> {
        // 1. Python
        Self::step(&tx, "Hledám Python interpret").await;
        let python = find_system_python().ok_or_else(|| {
            AppError::ComfyUi(
                "Python 3 nenalezen. Nainstaluj Python 3.10+ z python.org a spusť instalaci znovu."
                    .into(),
            )
        })?;

        // 2. Git clone ComfyUI (pokud ještě neexistuje)
        if !self.install_dir.join(".git").exists() {
            Self::step(&tx, "Stahuji ComfyUI (git clone)").await;
            std::fs::create_dir_all(&self.install_dir)
                .map_err(|e| AppError::ComfyUi(e.to_string()))?;
            run_streamed(
                "git",
                &[
                    "clone",
                    "--depth",
                    "1",
                    COMFYUI_REPO,
                    &self.install_dir.to_string_lossy(),
                ],
                None,
                &tx,
            )
            .await?;
        }

        // 3. Virtuální prostředí
        let venv_dir = self.venv_dir();
        if !venv_python_path(&venv_dir).exists() {
            Self::step(&tx, "Vytvářím Python virtuální prostředí").await;
            run_streamed(
                &python,
                &["-m", "venv", &venv_dir.to_string_lossy()],
                None,
                &tx,
            )
            .await?;
        }
        let venv_python = venv_python_path(&venv_dir).to_string_lossy().into_owned();

        // 4. PyTorch — CUDA build pokud je NVIDIA GPU, jinak CPU/výchozí
        Self::step(&tx, "Instaluji PyTorch (může trvat několik minut)").await;
        if has_nvidia_gpu() {
            run_streamed(
                &venv_python,
                &[
                    "-m",
                    "pip",
                    "install",
                    "torch",
                    "torchvision",
                    "torchaudio",
                    "--index-url",
                    "https://download.pytorch.org/whl/cu124",
                ],
                None,
                &tx,
            )
            .await?;
        } else {
            run_streamed(
                &venv_python,
                &["-m", "pip", "install", "torch", "torchvision", "torchaudio"],
                None,
                &tx,
            )
            .await?;
        }

        // 5. Zbytek závislostí ComfyUI
        Self::step(&tx, "Instaluji závislosti ComfyUI").await;
        let requirements = self.install_dir.join("requirements.txt");
        run_streamed(
            &venv_python,
            &[
                "-m",
                "pip",
                "install",
                "-r",
                &requirements.to_string_lossy(),
            ],
            None,
            &tx,
        )
        .await?;

        // 6. PuLID custom node
        let pulid_dir = self.pulid_dir();
        if !pulid_dir.exists() {
            Self::step(&tx, "Stahuji PuLID custom node").await;
            std::fs::create_dir_all(self.custom_nodes_dir())
                .map_err(|e| AppError::ComfyUi(e.to_string()))?;
            run_streamed(
                "git",
                &["clone", PULID_REPO, &pulid_dir.to_string_lossy()],
                None,
                &tx,
            )
            .await?;
        }

        // 7. PuLID závislosti (insightface aj.) — na Windows může vyžadovat
        // Visual C++ Build Tools; pokud selže, hlásíme srozumitelnou chybu
        // místo zahození celé instalace ComfyUI samotného.
        let pulid_requirements = pulid_dir.join("requirements.txt");
        if pulid_requirements.exists() {
            Self::step(&tx, "Instaluji závislosti PuLID (insightface aj.)").await;
            if let Err(e) = run_streamed(
                &venv_python,
                &[
                    "-m",
                    "pip",
                    "install",
                    "-r",
                    &pulid_requirements.to_string_lossy(),
                ],
                None,
                &tx,
            )
            .await
            {
                let _ = tx
                    .send(InstallProgress::Output(format!(
                        "Varování: instalace PuLID závislostí selhala ({e}). ComfyUI samotné je funkční, \
                         ale PuLID reference obrázky nemusí fungovat. Na Windows bývá potřeba nainstalovat \
                         'Visual C++ Build Tools' kvůli kompilaci insightface, pak zkus instalaci znovu."
                    )))
                    .await;
            }
        }

        // 8. SDXL checkpoint pro PuLID větev (viz komentář u konstanty výše)
        let checkpoint_path = self.checkpoints_dir().join(SDXL_CHECKPOINT_FILENAME);
        if !checkpoint_path.exists() {
            Self::step(&tx, "Stahuji SDXL checkpoint pro PuLID (~6,5 GB)").await;
            download_file(
                &self.http,
                SDXL_CHECKPOINT_URL,
                &checkpoint_path,
                "SDXL checkpoint",
                &tx,
            )
            .await?;
        }

        // 9. PuLID váhy
        let pulid_weights_path = self.pulid_weights_dir().join(PULID_WEIGHTS_FILENAME);
        if !pulid_weights_path.exists() {
            Self::step(&tx, "Stahuji PuLID váhy (~750 MB)").await;
            download_file(
                &self.http,
                PULID_WEIGHTS_URL,
                &pulid_weights_path,
                "PuLID váhy",
                &tx,
            )
            .await?;
        }

        // 10. InsightFace AntelopeV2 — detekce obličeje pro PulidInsightFaceLoader
        let insightface_dir = self.insightface_models_dir();
        if !insightface_dir.join("antelopev2").exists() {
            Self::step(&tx, "Stahuji InsightFace AntelopeV2 (~340 MB)").await;
            let zip_path = insightface_dir.join("antelopev2.zip");
            download_file(&self.http, ANTELOPEV2_ZIP_URL, &zip_path, "AntelopeV2", &tx).await?;
            extract_zip(&zip_path, &insightface_dir)?;
            std::fs::remove_file(&zip_path).ok();
        }

        let _ = tx.send(InstallProgress::Done).await;
        Ok(())
    }

    async fn start_server(&self) -> AppResult<()> {
        let mut guard = self.server.lock().await;
        if guard.is_some() {
            return Ok(()); // už běží
        }

        let venv_python = venv_python_path(&self.venv_dir());
        if !venv_python.exists() {
            return Err(AppError::ComfyUi("ComfyUI není nainstalováno".into()));
        }

        let child = tokio::process::Command::new(&venv_python)
            .arg("main.py")
            .arg("--port")
            .arg(COMFYUI_DEFAULT_PORT.to_string())
            .current_dir(&self.install_dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| AppError::ComfyUi(format!("Spuštění ComfyUI selhalo: {e}")))?;

        *guard = Some(child);
        drop(guard);

        // Health check — počkej, až server reálně odpovídá (max ~60s).
        let url = format!("http://localhost:{COMFYUI_DEFAULT_PORT}/system_stats");
        for _ in 0..60 {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            if self
                .http
                .get(&url)
                .send()
                .await
                .map(|r| r.status().is_success())
                .unwrap_or(false)
            {
                return Ok(());
            }
        }
        Err(AppError::ComfyUi(
            "ComfyUI server se nespustil do 60 sekund".into(),
        ))
    }

    async fn stop_server(&self) -> AppResult<()> {
        let mut guard = self.server.lock().await;
        if let Some(mut child) = guard.take() {
            let _ = child.kill().await;
        }
        Ok(())
    }

    async fn ensure_style_checkpoint(
        &self,
        style: StylePreset,
        tx: mpsc::Sender<InstallProgress>,
    ) -> AppResult<()> {
        let filename = checkpoint_filename_for_style(style);
        let path = self.checkpoints_dir().join(filename);
        if path.exists() {
            return Ok(());
        }
        Self::step(&tx, &format!("Stahuji model pro zvolený styl: {filename}")).await;
        download_file(
            &self.http,
            checkpoint_url_for_style(style),
            &path,
            filename,
            &tx,
        )
        .await
    }
}
