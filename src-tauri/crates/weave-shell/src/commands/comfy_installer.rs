use serde::Serialize;
use std::{path::Path, sync::Arc};
use tauri::{AppHandle, Emitter, Manager, State, Window};
use tokio::sync::mpsc;
use weave_application::ports::comfy_installer_port::{
    CheckpointInfo, ComfyInstallerPort, ComfyStatus, InstallProgress,
};

use crate::state::AppState;

fn make_installer(state: &AppState) -> Arc<dyn ComfyInstallerPort> {
    state.comfy_installer.clone()
}

#[derive(Debug, Serialize)]
pub struct ComfyDiagnostics {
    pub status: ComfyStatus,
    pub install_dir: String,
    pub main_py_exists: bool,
    pub requirements_exists: bool,
    pub venv_python_exists: bool,
    pub pulid_node_exists: bool,
    pub impact_pack_exists: bool,
    pub server_log_path: String,
    pub server_log_tail: String,
}

fn path_exists(path: &Path) -> bool {
    std::fs::metadata(path).is_ok()
}

fn read_tail(path: &Path, lines: usize) -> String {
    let Ok(text) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let mut tail = text.lines().rev().take(lines).collect::<Vec<_>>();
    tail.reverse();
    tail.join("\n")
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
pub async fn diagnose_comfyui(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ComfyDiagnostics, String> {
    let status = make_installer(&state)
        .status()
        .await
        .map_err(|e| e.to_string())?;
    let install_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("comfyui");
    let venv_python = if cfg!(windows) {
        install_dir.join("venv").join("Scripts").join("python.exe")
    } else {
        install_dir.join("venv").join("bin").join("python")
    };
    let server_log_path = install_dir.join("weave_logs").join("comfyui-server.log");

    Ok(ComfyDiagnostics {
        status,
        main_py_exists: path_exists(&install_dir.join("main.py")),
        requirements_exists: path_exists(&install_dir.join("requirements.txt")),
        venv_python_exists: path_exists(&venv_python),
        pulid_node_exists: path_exists(&install_dir.join("custom_nodes").join("PuLID_ComfyUI")),
        impact_pack_exists: path_exists(
            &install_dir.join("custom_nodes").join("ComfyUI-Impact-Pack"),
        ),
        server_log_tail: read_tail(&server_log_path, 80),
        server_log_path: server_log_path.to_string_lossy().into_owned(),
        install_dir: install_dir.to_string_lossy().into_owned(),
    })
}

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
pub async fn uninstall_comfyui(state: State<'_, AppState>) -> Result<(), String> {
    make_installer(&state)
        .uninstall()
        .await
        .map_err(|e| e.to_string())
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

/// Vypíše stažené LoRA soubory (models/loras) pro výběr v chatu.
#[tauri::command]
pub async fn list_lora_models(state: State<'_, AppState>) -> Result<Vec<CheckpointInfo>, String> {
    make_installer(&state)
        .list_loras()
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
