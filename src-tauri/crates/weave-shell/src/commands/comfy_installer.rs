use std::sync::Arc;
use tauri::{Emitter, State, Window};
use tokio::sync::mpsc;
use weave_application::ports::comfy_installer_port::{
    ComfyInstallerPort, ComfyStatus, InstallProgress,
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
