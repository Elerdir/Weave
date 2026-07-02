use std::sync::Arc;
use tauri::{Emitter, State, Window};
use tokio::sync::mpsc;
use weave_application::ports::comfy_installer_port::{
    CheckpointInfo, ComfyInstallerPort, ComfyStatus, InstallProgress,
};

use crate::state::AppState;

fn make_installer(state: &AppState) -> Arc<dyn ComfyInstallerPort> {
    state.comfy_installer.clone()
}

#[tauri::command]
pub async fn get_comfyui_status(state: State<'_, AppState>) -> Result<ComfyStatus, String> {
    make_installer(&state)
        .status()
        .await
        .map_err(|e| e.to_string())
}

/// Spustí instalaci ComfyUI + PuLID na pozadí. Progress se posílá do okna
/// jako `comfyui-install-progress`. Trvá desítky minut (stahuje PyTorch,
/// git klonuje repozitáře, kompiluje závislosti).
#[tauri::command]
pub async fn install_comfyui(window: Window, state: State<'_, AppState>) -> Result<(), String> {
    let installer = make_installer(&state);
    let (tx, mut rx) = mpsc::channel::<InstallProgress>(256);

    let window_clone = window.clone();
    tokio::spawn(async move {
        while let Some(progress) = rx.recv().await {
            let event = match &progress {
                InstallProgress::Step { name } => {
                    serde_json::json!({ "type": "step", "name": name })
                }
                InstallProgress::Output(line) => {
                    serde_json::json!({ "type": "output", "line": line })
                }
                InstallProgress::Done => serde_json::json!({ "type": "done" }),
                InstallProgress::Error(e) => serde_json::json!({ "type": "error", "message": e }),
            };
            let _ = window_clone.emit("comfyui-install-progress", event);
        }
    });

    installer.install(tx).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn start_comfyui_server(state: State<'_, AppState>) -> Result<(), String> {
    make_installer(&state)
        .start_server()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stop_comfyui_server(state: State<'_, AppState>) -> Result<(), String> {
    make_installer(&state)
        .stop_server()
        .await
        .map_err(|e| e.to_string())
}

/// Vypíše stažené obrázkové checkpointy (models/checkpoints).
#[tauri::command]
pub async fn list_image_models(state: State<'_, AppState>) -> Result<Vec<CheckpointInfo>, String> {
    make_installer(&state)
        .list_checkpoints()
        .await
        .map_err(|e| e.to_string())
}

/// Smaže stažený obrázkový checkpoint podle názvu souboru.
#[tauri::command]
pub async fn delete_image_model(
    file_name: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    make_installer(&state)
        .delete_checkpoint(&file_name)
        .await
        .map_err(|e| e.to_string())
}

/// Zkopíruje soubor (typicky vygenerovaný obrázek) do cílové cesty vybrané
/// uživatelem v save dialogu.
#[tauri::command]
pub async fn save_file_copy(source: String, dest: String) -> Result<(), String> {
    tokio::fs::copy(&source, &dest)
        .await
        .map(|_| ())
        .map_err(|e| format!("Uložení souboru selhalo: {e}"))
}
