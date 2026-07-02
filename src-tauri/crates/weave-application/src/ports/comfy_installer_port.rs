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
    async fn start_server(&self) -> AppResult<()>;
    async fn stop_server(&self) -> AppResult<()>;
    /// Zajistí, že je stažený checkpoint pro daný styl obrázku — když chybí,
    /// stáhne ho (s průběhem přes `tx`). Když už existuje, nedělá nic.
    async fn ensure_style_checkpoint(
        &self,
        style: StylePreset,
        tx: mpsc::Sender<InstallProgress>,
    ) -> AppResult<()>;
}
