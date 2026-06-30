use tauri::{Emitter, State, Window};
use tokio::sync::mpsc;
use weave_application::ports::model_manager_port::{DownloadProgress, GpuInfo, LocalModel};

use crate::state::AppState;

#[tauri::command]
pub async fn list_local_models(state: State<'_, AppState>) -> Result<Vec<LocalModel>, String> {
    state
        .model_manager
        .list_local()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn detect_gpu(state: State<'_, AppState>) -> Result<Option<GpuInfo>, String> {
    state
        .model_manager
        .detect_gpu()
        .await
        .map_err(|e| e.to_string())
}

/// Stáhne model na pozadí. Progress se posílá do okna jako `model-download-progress`.
#[tauri::command]
pub async fn download_model(
    model_id: String,
    source_url: String,
    window: Window,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let (tx, mut rx) = mpsc::channel::<DownloadProgress>(64);

    let window_clone = window.clone();
    let model_for_events = model_id.clone();
    tokio::spawn(async move {
        while let Some(progress) = rx.recv().await {
            let event = match &progress {
                DownloadProgress::Started {
                    model_id,
                    total_bytes,
                } => {
                    serde_json::json!({ "type": "started", "modelId": model_id, "total": total_bytes })
                }
                DownloadProgress::Progress { downloaded, total } => {
                    serde_json::json!({ "type": "progress", "modelId": model_for_events, "downloaded": downloaded, "total": total })
                }
                DownloadProgress::Verifying => {
                    serde_json::json!({ "type": "verifying", "modelId": model_for_events })
                }
                DownloadProgress::Done { model } => {
                    serde_json::json!({ "type": "done", "modelId": model.id, "model": model })
                }
                DownloadProgress::Error(e) => {
                    serde_json::json!({ "type": "error", "modelId": model_for_events, "message": e })
                }
            };
            let _ = window_clone.emit("model-download-progress", event);
        }
    });

    state
        .model_manager
        .download(&model_id, &source_url, tx)
        .await
        .map_err(|e| e.to_string())
}

/// Smaže lokální model.
#[tauri::command]
pub async fn delete_model(model_id: String, state: State<'_, AppState>) -> Result<(), String> {
    state
        .model_manager
        .delete(&model_id)
        .await
        .map_err(|e| e.to_string())
}
