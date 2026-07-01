mod process;

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::process::Child;
use tokio::sync::{mpsc, Mutex};
use weave_application::{
    error::{AppError, AppResult},
    ports::comfy_installer_port::{ComfyInstallerPort, ComfyStatus, InstallProgress},
};

use process::{find_system_python, has_nvidia_gpu, run_streamed, venv_python_path};

const COMFYUI_REPO: &str = "https://github.com/comfyanonymous/ComfyUI.git";
const PULID_REPO: &str = "https://github.com/cubiq/PuLID_ComfyUI.git";
pub const COMFYUI_DEFAULT_PORT: u16 = 8188;

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
}
