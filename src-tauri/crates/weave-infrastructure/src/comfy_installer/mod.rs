mod process;

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::process::Child;
use tokio::sync::{mpsc, Mutex};
use weave_application::{
    error::{AppError, AppResult},
    ports::comfy_installer_port::{
        CheckpointInfo, ComfyInstallerPort, ComfyStatus, InstallProgress,
    },
    ports::image_gen_port::StylePreset,
};

use process::{
    download_file, extract_zip, find_system_python, has_nvidia_gpu, run_streamed, venv_python_path,
};

const COMFYUI_REPO: &str = "https://github.com/comfyanonymous/ComfyUI.git";
const PULID_REPO: &str = "https://github.com/cubiq/PuLID_ComfyUI.git";
/// Weave spouští vlastní ComfyUI na dedikovaném portu, ne na standardním
/// 8188 — na 8188 běžně poslouchají jiné ComfyUI instalace (např. z AI
/// Studia). Kdyby Weave použil 8188, health-check by se navázal na cizí
/// instanci s jinými modely a workflow by padal na „checkpoint not in list".
pub const COMFYUI_DEFAULT_PORT: u16 = 8199;

/// Základní SDXL checkpoint — používá ho text-to-image i PuLID větev workflow.
/// `cubiq/PuLID_ComfyUI` patchuje SDXL cross-attention, ne FLUX/DiT. SDXL base
/// je navíc na HuggingFace veřejný bez přihlášení, což FLUX.1-dev/schnell
/// nejsou (obě vrací 401 bez auth tokenu) — jednoklikový no-auth download
/// stylem `recommended_models.rs` by u FLUX nešel udělat.
pub const SDXL_CHECKPOINT_FILENAME: &str = "sd_xl_base_1.0.safetensors";
const SDXL_CHECKPOINT_URL: &str = "https://huggingface.co/stabilityai/stable-diffusion-xl-base-1.0/resolve/main/sd_xl_base_1.0.safetensors";

/// Realistický checkpoint — RealVisXL V5.0, jeden z nejlepších SDXL modelů
/// na fotorealismus (obličeje, kůže, světlo). SDXL architektura → PuLID OK.
pub const REALVIS_CHECKPOINT_FILENAME: &str = "RealVisXL_V5.0_fp16.safetensors";
const REALVIS_CHECKPOINT_URL: &str =
    "https://huggingface.co/SG161222/RealVisXL_V5.0/resolve/main/RealVisXL_V5.0_fp16.safetensors";

/// Semi-real/anime checkpoint — Pony Diffusion V6 XL (SDXL architektura).
/// Vyžaduje score tagy v promptu a clip skip -2 — obojí řeší workflow builder.
/// Oficiální distribuce je na CivitAI za přihlášením; tohle je veřejné
/// HF zrcadlo (ověřeno HEAD 200, 6 938 041 050 B).
pub const PONY_CHECKPOINT_FILENAME: &str = "ponyDiffusionV6XL_v6StartWithThisOne.safetensors";
const PONY_CHECKPOINT_URL: &str = "https://huggingface.co/LyliaEngine/Pony_Diffusion_V6_XL/resolve/main/ponyDiffusionV6XL_v6StartWithThisOne.safetensors";

/// Který checkpoint použít pro daný styl: Realistic → RealVisXL,
/// SemiRealistic/Anime → Pony V6, Artistic/ThreeD → SDXL base (styl se
/// řídí promptem). Všechny jsou SDXL architektura → fungují s PuLID.
pub fn checkpoint_filename_for_style(style: StylePreset) -> &'static str {
    match style {
        StylePreset::Realistic => REALVIS_CHECKPOINT_FILENAME,
        StylePreset::SemiRealistic | StylePreset::Anime => PONY_CHECKPOINT_FILENAME,
        StylePreset::Artistic | StylePreset::ThreeD => SDXL_CHECKPOINT_FILENAME,
    }
}

fn checkpoint_url_for_style(style: StylePreset) -> &'static str {
    match style {
        StylePreset::Realistic => REALVIS_CHECKPOINT_URL,
        StylePreset::SemiRealistic | StylePreset::Anime => PONY_CHECKPOINT_URL,
        StylePreset::Artistic | StylePreset::ThreeD => SDXL_CHECKPOINT_URL,
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

/// ComfyUI Impact Pack (FaceDetailer uzel) a Impact Subpack
/// (UltralyticsDetectorProvider) — volitelné doladění obličeje/očí.
const IMPACT_PACK_REPO: &str = "https://github.com/ltdrdata/ComfyUI-Impact-Pack.git";
const IMPACT_SUBPACK_REPO: &str = "https://github.com/ltdrdata/ComfyUI-Impact-Subpack.git";

/// YOLO detektor obličeje pro `UltralyticsDetectorProvider`. Ukládá se do
/// `models/ultralytics/bbox/`, uzel na něj odkazuje s prefixem podsložky
/// (`bbox/…`) — proto ta hodnota v [`FACE_DETECTOR_MODEL_NAME`].
pub const FACE_DETECTOR_MODEL_NAME: &str = "bbox/face_yolov8m.pt";
const FACE_DETECTOR_FILENAME: &str = "face_yolov8m.pt";
const FACE_DETECTOR_URL: &str =
    "https://huggingface.co/Bingsu/adetailer/resolve/main/face_yolov8m.pt";

// set_readonly(false) je tu záměr: git objekty mají na Windows readonly flag,
// který blokuje remove_dir_all při odinstalaci. Volá se těsně před smazáním
// složky, takže „world writable" na Unixu je bez praktického dopadu.
#[allow(clippy::permissions_set_readonly_false)]
fn clear_readonly_flags(path: &Path) -> AppResult<()> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        for entry in std::fs::read_dir(path).map_err(|e| AppError::ComfyUi(e.to_string()))? {
            let entry = entry.map_err(|e| AppError::ComfyUi(e.to_string()))?;
            clear_readonly_flags(&entry.path())?;
        }
    }
    let metadata = std::fs::metadata(path).map_err(|e| AppError::ComfyUi(e.to_string()))?;
    let mut permissions = metadata.permissions();
    if permissions.readonly() {
        permissions.set_readonly(false);
        std::fs::set_permissions(path, permissions)
            .map_err(|e| AppError::ComfyUi(e.to_string()))?;
    }
    Ok(())
}

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

    fn main_py_path(&self) -> PathBuf {
        self.install_dir.join("main.py")
    }

    fn has_valid_checkout(&self) -> bool {
        self.main_py_path().exists() && self.install_dir.join("requirements.txt").exists()
    }

    fn remove_install_dir(&self) -> AppResult<()> {
        if !self.install_dir.exists() {
            return Ok(());
        }
        clear_readonly_flags(&self.install_dir)?;
        std::fs::remove_dir_all(&self.install_dir)
            .map_err(|e| AppError::ComfyUi(format!("Odinstalace ComfyUI selhala: {e}")))
    }

    fn custom_nodes_dir(&self) -> PathBuf {
        self.install_dir.join("custom_nodes")
    }

    fn pulid_dir(&self) -> PathBuf {
        self.custom_nodes_dir().join("PuLID_ComfyUI")
    }

    fn impact_pack_dir(&self) -> PathBuf {
        self.custom_nodes_dir().join("ComfyUI-Impact-Pack")
    }

    fn impact_subpack_dir(&self) -> PathBuf {
        self.custom_nodes_dir().join("ComfyUI-Impact-Subpack")
    }

    /// Složka detektorů obličeje pro `UltralyticsDetectorProvider`.
    fn ultralytics_bbox_dir(&self) -> PathBuf {
        self.install_dir
            .join("models")
            .join("ultralytics")
            .join("bbox")
    }

    fn checkpoints_dir(&self) -> PathBuf {
        self.install_dir.join("models").join("checkpoints")
    }

    fn logs_dir(&self) -> PathBuf {
        self.install_dir.join("weave_logs")
    }

    fn server_log_path(&self) -> PathBuf {
        self.logs_dir().join("comfyui-server.log")
    }

    fn read_log_tail(path: &Path) -> String {
        let Ok(text) = std::fs::read_to_string(path) else {
            return "Log ComfyUI zatim neni dostupny.".into();
        };
        let mut lines = text.lines().rev().take(80).collect::<Vec<_>>();
        lines.reverse();
        let tail = lines.join("\n");
        if tail.trim().is_empty() {
            "Log ComfyUI je prazdny.".into()
        } else {
            tail
        }
    }

    fn start_error(message: impl Into<String>, log_path: &Path) -> AppError {
        AppError::ComfyUi(format!(
            "{}\n\nPosledni radky logu ({}):\n{}",
            message.into(),
            log_path.display(),
            Self::read_log_tail(log_path)
        ))
    }

    fn loras_dir(&self) -> PathBuf {
        self.install_dir.join("models").join("loras")
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

    /// Naklonuje PuLID custom node, pokud chybí. Vrací true, když klonoval
    /// (nová instalace uzlu → je potřeba doinstalovat i jeho pip závislosti).
    async fn ensure_pulid_custom_node(
        &self,
        tx: &mpsc::Sender<InstallProgress>,
    ) -> AppResult<bool> {
        let pulid_dir = self.pulid_dir();
        if pulid_dir.exists() {
            return Ok(false);
        }
        Self::step(tx, "Stahuji PuLID custom node").await;
        std::fs::create_dir_all(self.custom_nodes_dir())
            .map_err(|e| AppError::ComfyUi(e.to_string()))?;
        run_streamed(
            "git",
            &["clone", PULID_REPO, &pulid_dir.to_string_lossy()],
            None,
            tx,
        )
        .await?;
        Ok(true)
    }

    /// Pip závislosti PuLID (insightface aj.) — na Windows může vyžadovat
    /// Visual C++ Build Tools; selhání hlásíme srozumitelně místo zahození
    /// celé instalace ComfyUI samotného.
    async fn install_pulid_python_deps(&self, tx: &mpsc::Sender<InstallProgress>) {
        let pulid_requirements = self.pulid_dir().join("requirements.txt");
        if !pulid_requirements.exists() {
            return;
        }
        let venv_python = venv_python_path(&self.venv_dir());
        if !venv_python.exists() {
            return; // venv ještě není — závislosti doinstaluje plná instalace
        }
        Self::step(tx, "Instaluji závislosti PuLID (insightface aj.)").await;
        if let Err(e) = run_streamed(
            &venv_python.to_string_lossy(),
            &[
                "-m",
                "pip",
                "install",
                "-r",
                &pulid_requirements.to_string_lossy(),
            ],
            None,
            tx,
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

    /// Stáhne PuLID váhy pro `PulidModelLoader`, pokud chybí.
    async fn ensure_pulid_weights(&self, tx: &mpsc::Sender<InstallProgress>) -> AppResult<()> {
        let pulid_weights_path = self.pulid_weights_dir().join(PULID_WEIGHTS_FILENAME);
        if pulid_weights_path.exists() {
            return Ok(());
        }
        Self::step(tx, "Stahuji PuLID váhy (~750 MB)").await;
        download_file(
            &self.http,
            PULID_WEIGHTS_URL,
            &pulid_weights_path,
            "PuLID váhy",
            tx,
        )
        .await
    }

    /// Stáhne a rozbalí InsightFace AntelopeV2 pro `PulidInsightFaceLoader`,
    /// pokud chybí.
    async fn ensure_antelopev2(&self, tx: &mpsc::Sender<InstallProgress>) -> AppResult<()> {
        let insightface_dir = self.insightface_models_dir();
        if insightface_dir.join("antelopev2").exists() {
            return Ok(());
        }
        Self::step(tx, "Stahuji InsightFace AntelopeV2 (~340 MB)").await;
        let zip_path = insightface_dir.join("antelopev2.zip");
        download_file(&self.http, ANTELOPEV2_ZIP_URL, &zip_path, "AntelopeV2", tx).await?;
        extract_zip(&zip_path, &insightface_dir)?;
        std::fs::remove_file(&zip_path).ok();
        Ok(())
    }

    /// Naklonuje custom node repozitář do `custom_nodes/`, pokud ještě chybí.
    /// Vrátí `true`, když se klonovalo teď (volající pak doinstaluje pip deps).
    async fn ensure_custom_node(
        &self,
        repo: &str,
        dir: &std::path::Path,
        label: &str,
        tx: &mpsc::Sender<InstallProgress>,
    ) -> AppResult<bool> {
        if dir.exists() {
            return Ok(false);
        }
        Self::step(tx, &format!("Stahuji {label} custom node")).await;
        std::fs::create_dir_all(self.custom_nodes_dir())
            .map_err(|e| AppError::ComfyUi(e.to_string()))?;
        run_streamed("git", &["clone", repo, &dir.to_string_lossy()], None, tx).await?;
        Ok(true)
    }

    /// Doinstaluje pip závislosti (`requirements.txt`) daného custom node uzlu
    /// do ComfyUI venv. Selhání jen zaloguje (uzel může fungovat i bez všech
    /// extras) — neshodí instalaci ComfyUI samotného.
    async fn install_node_python_deps(
        &self,
        dir: &std::path::Path,
        label: &str,
        tx: &mpsc::Sender<InstallProgress>,
    ) {
        let requirements = dir.join("requirements.txt");
        if !requirements.exists() {
            return;
        }
        let venv_python = venv_python_path(&self.venv_dir());
        if !venv_python.exists() {
            return;
        }
        Self::step(tx, &format!("Instaluji závislosti {label}")).await;
        if let Err(e) = run_streamed(
            &venv_python.to_string_lossy(),
            &[
                "-m",
                "pip",
                "install",
                "-r",
                &requirements.to_string_lossy(),
            ],
            None,
            tx,
        )
        .await
        {
            let _ = tx
                .send(InstallProgress::Output(format!(
                    "Varování: instalace závislostí {label} selhala ({e}). Doladění obličeje \
                     (FaceDetailer) nemusí fungovat; ostatní generování je v pořádku."
                )))
                .await;
        }
    }

    /// Stáhne YOLO detektor obličeje pro `UltralyticsDetectorProvider`, pokud chybí.
    async fn ensure_face_detector_model(
        &self,
        tx: &mpsc::Sender<InstallProgress>,
    ) -> AppResult<()> {
        let path = self.ultralytics_bbox_dir().join(FACE_DETECTOR_FILENAME);
        if path.exists() {
            return Ok(());
        }
        Self::step(tx, "Stahuji detektor obličeje (face_yolov8m ~52 MB)").await;
        download_file(
            &self.http,
            FACE_DETECTOR_URL,
            &path,
            "detektor obličeje",
            tx,
        )
        .await
    }
}

#[async_trait]
impl ComfyInstallerPort for LocalComfyInstaller {
    async fn status(&self) -> AppResult<ComfyStatus> {
        if self.server.lock().await.is_some() {
            return Ok(ComfyStatus::Running);
        }
        if self.has_valid_checkout() && venv_python_path(&self.venv_dir()).exists() {
            Ok(ComfyStatus::Installed)
        } else if self.install_dir.exists() {
            Ok(ComfyStatus::Broken)
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
        if self.install_dir.exists() && !self.has_valid_checkout() {
            Self::step(&tx, "Opravuji rozbitou instalaci ComfyUI").await;
            self.remove_install_dir()?;
        }

        if !self.has_valid_checkout() {
            Self::step(&tx, "Stahuji ComfyUI (git clone)").await;
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

        // 4. PyTorch — CUDA build pokud je NVIDIA GPU, jinak CPU/výchozí.
        // Index se čas od času posouvá dál (PyTorch přestává pro starší CUDA
        // verze stavět wheely pro nové verze Pythonu) — cu124 přestal mít
        // wheely pro Python 3.14, cu126 v době psaní funguje pro 3.9-3.14.
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
                    "https://download.pytorch.org/whl/cu126",
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

        // 6.+7. PuLID custom node + jeho pip závislosti (i pro existující
        // klon — dřívější pokus o pip mohl selhat a re-instalace ho zopakuje)
        self.ensure_pulid_custom_node(&tx).await?;
        self.install_pulid_python_deps(&tx).await;

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

        // 9. PuLID váhy + 10. InsightFace AntelopeV2
        self.ensure_pulid_weights(&tx).await?;
        self.ensure_antelopev2(&tx).await?;

        let _ = tx.send(InstallProgress::Done).await;
        Ok(())
    }

    async fn uninstall(&self) -> AppResult<()> {
        self.stop_server().await?;
        self.remove_install_dir()
    }

    async fn ensure_lora(
        &self,
        file_name: &str,
        download_url: &str,
        tx: mpsc::Sender<InstallProgress>,
    ) -> AppResult<()> {
        // Ochrana proti path traversal — jméno souboru nesmí obsahovat cestu.
        if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
            return Err(AppError::ComfyUi(format!(
                "Neplatný název LoRA souboru: {file_name}"
            )));
        }
        let path = self.loras_dir().join(file_name);
        if path.exists() {
            return Ok(());
        }
        Self::step(&tx, &format!("Stahuji LoRA: {file_name}")).await;
        download_file(&self.http, download_url, &path, file_name, &tx).await
    }

    async fn ensure_checkpoint(
        &self,
        file_name: &str,
        download_url: &str,
        tx: mpsc::Sender<InstallProgress>,
    ) -> AppResult<()> {
        // Ochrana proti path traversal — jméno souboru nesmí obsahovat cestu.
        if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
            return Err(AppError::ComfyUi(format!(
                "Neplatný název checkpoint souboru: {file_name}"
            )));
        }
        let path = self.checkpoints_dir().join(file_name);
        if path.exists() {
            return Ok(());
        }
        Self::step(&tx, &format!("Stahuji checkpoint: {file_name}")).await;
        download_file(&self.http, download_url, &path, file_name, &tx).await
    }

    async fn ensure_reference_assets(&self, tx: mpsc::Sender<InstallProgress>) -> AppResult<()> {
        let cloned_now = self.ensure_pulid_custom_node(&tx).await?;
        if cloned_now {
            self.install_pulid_python_deps(&tx).await;
        }
        self.ensure_pulid_weights(&tx).await?;
        self.ensure_antelopev2(&tx).await
    }

    async fn ensure_face_detailer_assets(
        &self,
        tx: mpsc::Sender<InstallProgress>,
    ) -> AppResult<()> {
        // Impact Pack = FaceDetailer uzel; Impact Subpack = UltralyticsDetectorProvider.
        if self
            .ensure_custom_node(
                IMPACT_PACK_REPO,
                &self.impact_pack_dir(),
                "Impact Pack",
                &tx,
            )
            .await?
        {
            self.install_node_python_deps(&self.impact_pack_dir(), "Impact Pack", &tx)
                .await;
        }
        if self
            .ensure_custom_node(
                IMPACT_SUBPACK_REPO,
                &self.impact_subpack_dir(),
                "Impact Subpack",
                &tx,
            )
            .await?
        {
            self.install_node_python_deps(&self.impact_subpack_dir(), "Impact Subpack", &tx)
                .await;
        }
        self.ensure_face_detector_model(&tx).await?;
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

        if !self.has_valid_checkout() {
            return Err(AppError::ComfyUi(format!(
                "ComfyUI instalace je nekompletni: chybi {}. V nastaveni pouzij Opravit instalaci.",
                self.main_py_path().display()
            )));
        }

        let log_path = self.server_log_path();
        std::fs::create_dir_all(self.logs_dir())
            .map_err(|e| AppError::ComfyUi(format!("Vytvoreni ComfyUI log slozky selhalo: {e}")))?;
        let stdout = std::fs::File::create(&log_path)
            .map_err(|e| AppError::ComfyUi(format!("Vytvoreni ComfyUI logu selhalo: {e}")))?;
        let stderr = stdout
            .try_clone()
            .map_err(|e| AppError::ComfyUi(format!("Priprava ComfyUI logu selhala: {e}")))?;

        let mut cmd = tokio::process::Command::new(&venv_python);
        cmd.arg("main.py")
            .arg("--port")
            .arg(COMFYUI_DEFAULT_PORT.to_string())
            .current_dir(&self.install_dir)
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        crate::spawn::hide_console(&mut cmd);
        let child = cmd
            .spawn()
            .map_err(|e| AppError::ComfyUi(format!("Spuštění ComfyUI selhalo: {e}")))?;

        *guard = Some(child);
        drop(guard);

        // Health check — počkej, až server reálně odpovídá (max ~60s).
        let url = format!("http://localhost:{COMFYUI_DEFAULT_PORT}/system_stats");
        for _ in 0..60 {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

            {
                let mut guard = self.server.lock().await;
                if let Some(child) = guard.as_mut() {
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            *guard = None;
                            return Err(Self::start_error(
                                format!("ComfyUI proces skoncil pred startem serveru ({status})."),
                                &log_path,
                            ));
                        }
                        Ok(None) => {}
                        Err(e) => {
                            return Err(Self::start_error(
                                format!("Kontrola ComfyUI procesu selhala: {e}"),
                                &log_path,
                            ));
                        }
                    }
                } else {
                    return Err(Self::start_error(
                        "ComfyUI proces uz neni evidovany jako bezici.",
                        &log_path,
                    ));
                }
            }

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
        let _ = self.stop_server().await;
        Err(Self::start_error(
            "ComfyUI server se nespustil do 60 sekund.",
            &log_path,
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

    async fn list_checkpoints(&self) -> AppResult<Vec<CheckpointInfo>> {
        let dir = self.checkpoints_dir();
        let mut result = Vec::new();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Ok(result); // složka ještě neexistuje = žádné modely
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_model = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                e.eq_ignore_ascii_case("safetensors") || e.eq_ignore_ascii_case("ckpt")
            });
            if !is_model {
                continue;
            }
            if let (Some(name), Ok(meta)) = (path.file_name(), entry.metadata()) {
                result.push(CheckpointInfo {
                    file_name: name.to_string_lossy().into_owned(),
                    size_bytes: meta.len(),
                });
            }
        }
        result.sort_by(|a, b| a.file_name.cmp(&b.file_name));
        Ok(result)
    }

    async fn delete_checkpoint(&self, file_name: &str) -> AppResult<()> {
        // Ochrana proti path traversal — jméno souboru nesmí obsahovat cestu.
        if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
            return Err(AppError::ComfyUi(format!(
                "Neplatný název souboru: {file_name}"
            )));
        }
        let path = self.checkpoints_dir().join(file_name);
        std::fs::remove_file(&path)
            .map_err(|e| AppError::ComfyUi(format!("Smazání modelu selhalo: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_installer() -> LocalComfyInstaller {
        let dir = std::env::temp_dir().join(format!("weave_ckpt_test_{}", uuid::Uuid::new_v4()));
        LocalComfyInstaller::new(dir)
    }

    #[tokio::test]
    async fn list_checkpoints_empty_without_directory() {
        let installer = temp_installer();
        assert!(installer.list_checkpoints().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn status_reports_broken_when_directory_is_incomplete() {
        let installer = temp_installer();
        std::fs::create_dir_all(&installer.install_dir).unwrap();
        assert_eq!(installer.status().await.unwrap(), ComfyStatus::Broken);
    }

    #[tokio::test]
    async fn list_and_delete_checkpoints_roundtrip() {
        let installer = temp_installer();
        let dir = installer.checkpoints_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("model-a.safetensors"), b"aa").unwrap();
        std::fs::write(dir.join("model-b.CKPT"), b"bbb").unwrap();
        std::fs::write(dir.join("poznamky.txt"), b"ne-model").unwrap();

        let listed = installer.list_checkpoints().await.unwrap();
        let names: Vec<&str> = listed.iter().map(|c| c.file_name.as_str()).collect();
        assert_eq!(names, vec!["model-a.safetensors", "model-b.CKPT"]);
        assert_eq!(listed[0].size_bytes, 2);

        installer
            .delete_checkpoint("model-a.safetensors")
            .await
            .unwrap();
        assert_eq!(installer.list_checkpoints().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn ensure_reference_assets_is_noop_when_everything_exists() {
        let installer = temp_installer();
        // Připrav všechny PuLID assety „na disku" — nic se nesmí stahovat
        // (žádný network krok by tady stejně neprošel).
        std::fs::create_dir_all(installer.pulid_dir()).unwrap();
        std::fs::create_dir_all(installer.pulid_weights_dir()).unwrap();
        std::fs::write(
            installer.pulid_weights_dir().join(PULID_WEIGHTS_FILENAME),
            b"fake",
        )
        .unwrap();
        std::fs::create_dir_all(installer.insightface_models_dir().join("antelopev2")).unwrap();

        let (tx, mut rx) = mpsc::channel(8);
        installer.ensure_reference_assets(tx).await.unwrap();

        // Žádné kroky se nehlásily — vše už existovalo.
        assert!(rx.recv().await.is_none());
    }

    #[tokio::test]
    async fn delete_checkpoint_rejects_path_traversal() {
        let installer = temp_installer();
        assert!(installer
            .delete_checkpoint("../venv/pyvenv.cfg")
            .await
            .is_err());
        assert!(installer
            .delete_checkpoint("sub/model.safetensors")
            .await
            .is_err());
    }
}
