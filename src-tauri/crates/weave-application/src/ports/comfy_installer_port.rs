use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::error::AppResult;
use crate::ports::image_gen_port::StylePreset;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InstallProgress {
    /// Nový krok instalace začal (zobrazí se jako nadpis v UI).
    Step {
        name: String,
    },
    /// Živý řádek výstupu z git/pip/python — pro transparentnost dlouhotrvajících kroků.
    Output(String),
    Done,
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ComfyStatus {
    NotInstalled,
    Broken,
    Installed,
    Running,
}

/// Instaluje a spravuje lokální ComfyUI instanci (git clone + venv + pip +
/// PuLID custom node) a spouští ji jako podproces. Cíl: „jedno tlačítko",
/// vše lokálně, bez nutnosti ručního Python/pip zásahu od uživatele.
#[async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait ComfyInstallerPort: Send + Sync {
    async fn status(&self) -> AppResult<ComfyStatus>;
    async fn install(&self, tx: mpsc::Sender<InstallProgress>) -> AppResult<()>;
    /// ZastavĂ­ lokĂˇlnĂ­ server a odstranĂ­ spravovanou ComfyUI instalaci vÄŤetnÄ›
    /// venv, custom nodĹŻ a staĹľenĂ˝ch image modelĹŻ. Galerie obrĂˇzkĹŻ zĹŻstĂˇvĂˇ
    /// mimo tuto sloĹľku a nemaĹľe se.
    async fn uninstall(&self) -> AppResult<()>;
    async fn start_server(&self) -> AppResult<()>;
    async fn stop_server(&self) -> AppResult<()>;
    /// Zajistí, že je stažený checkpoint pro daný styl obrázku — když chybí,
    /// stáhne ho (s průběhem přes `tx`). Když už existuje, nedělá nic.
    async fn ensure_style_checkpoint(
        &self,
        style: StylePreset,
        tx: mpsc::Sender<InstallProgress>,
    ) -> AppResult<()>;
    /// Zajistí assety pro generování s referenčním obrázkem (PuLID custom
    /// node, PuLID váhy, InsightFace AntelopeV2) — instalace z dřívějších
    /// verzí appky je mít nemusí. Co existuje, přeskočí; co chybí, stáhne
    /// (s průběhem přes `tx`).
    async fn ensure_reference_assets(&self, tx: mpsc::Sender<InstallProgress>) -> AppResult<()>;
    /// Zajistí assety pro doladění obličeje FaceDetailerem (ComfyUI Impact
    /// Pack + Impact Subpack custom nodes, `ultralytics` a detekční model
    /// obličeje). Co existuje, přeskočí; co chybí, stáhne (průběh přes `tx`).
    async fn ensure_face_detailer_assets(&self, tx: mpsc::Sender<InstallProgress>)
        -> AppResult<()>;
    /// Zajistí LoRA soubor v `models/loras` — když chybí, stáhne ho
    /// (s průběhem přes `tx`). Když existuje, nedělá nic.
    async fn ensure_lora(
        &self,
        file_name: &str,
        download_url: &str,
        tx: mpsc::Sender<InstallProgress>,
    ) -> AppResult<()>;
    /// Zajistí checkpoint soubor v `models/checkpoints` — když chybí, stáhne
    /// ho (s průběhem přes `tx`). Když existuje, nedělá nic.
    async fn ensure_checkpoint(
        &self,
        file_name: &str,
        download_url: &str,
        tx: mpsc::Sender<InstallProgress>,
    ) -> AppResult<()>;
    /// Vypíše stažené obrázkové checkpointy (models/checkpoints).
    async fn list_checkpoints(&self) -> AppResult<Vec<CheckpointInfo>>;
    /// Smaže stažený checkpoint podle názvu souboru.
    async fn delete_checkpoint(&self, file_name: &str) -> AppResult<()>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointInfo {
    pub file_name: String,
    pub size_bytes: u64,
}
