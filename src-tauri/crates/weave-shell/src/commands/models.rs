use tauri::{Emitter, State, Window};
use tokio::sync::mpsc;
use weave_application::ports::model_manager_port::{DownloadProgress, GpuInfo, LocalModel};
use weave_domain::recommended_models::{recommended_models, RecommendedModel};

use crate::state::AppState;

#[tauri::command]
pub async fn list_local_models(state: State<'_, AppState>) -> Result<Vec<LocalModel>, String> {
    state
        .model_manager
        .list_local()
        .await
        .map_err(|e| e.to_string())
}

/// Vrátí seznam doporučených modelů k jednoklikovému stažení.
#[tauri::command]
pub async fn list_recommended_models() -> Result<Vec<RecommendedModel>, String> {
    Ok(recommended_models())
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

/// Stáhne doporučený model podle ID a po dokončení automaticky přepne
/// appku na vestavěnou GPU inferenci (backend=embedded, model_path, gpu_layers)
/// — cíl je "jedno tlačítko" bez nutnosti cokoliv ručně nastavovat.
#[tauri::command]
pub async fn download_recommended_model(
    model_id: String,
    window: Window,
    state: State<'_, AppState>,
) -> Result<(), String> {
    use weave_infrastructure::db::app_config;

    let recommended = recommended_models()
        .into_iter()
        .find(|m| m.id == model_id)
        .ok_or_else(|| format!("Neznámý doporučený model: {model_id}"))?;

    let (tx, mut rx) = mpsc::channel::<DownloadProgress>(64);
    let window_clone = window.clone();
    let model_for_events = recommended.id.clone();

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
        .download(&recommended.id, &recommended.download_url, tx)
        .await
        .map_err(|e| e.to_string())?;

    // Model stažen → nastavíme appku, aby ho rovnou používala.
    let local_model = state
        .model_manager
        .list_local()
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|m| m.id == recommended.id)
        .ok_or_else(|| "Model se stáhl, ale nenašel se v manifestu".to_string())?;

    app_config::set(&state.pool, super::settings::LLM_BACKEND_KEY, "embedded")
        .await
        .map_err(|e| e.to_string())?;
    app_config::set(
        &state.pool,
        super::settings::LLM_MODEL_PATH_KEY,
        &local_model.path,
    )
    .await
    .map_err(|e| e.to_string())?;
    app_config::set(
        &state.pool,
        super::settings::LLM_GPU_LAYERS_KEY,
        &recommended.recommended_gpu_layers.to_string(),
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}
