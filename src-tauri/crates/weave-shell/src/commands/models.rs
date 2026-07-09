use tauri::{Emitter, State, Window};
use tokio::sync::mpsc;
use weave_application::ports::model_manager_port::{DownloadProgress, GpuInfo, LocalModel};
use weave_domain::recommended_models::{
    recommend_gpu_layers, recommended_models, RecommendedModel,
};

use crate::state::AppState;

/// Klíč v `app_config`, pod kterým se pamatuje uživatelem zvolená složka pro
/// stahování modelů (aby se použila i po restartu appky).
pub const MODELS_DIR_KEY: &str = "models.directory";
pub const DOWNLOAD_SEGMENTS_KEY: &str = "models.download_segments";

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
pub async fn get_download_segments() -> Result<u64, String> {
    Ok(weave_infrastructure::parallel_download::current_max_segments())
}

#[tauri::command]
pub async fn set_download_segments(
    segments: u64,
    state: State<'_, AppState>,
) -> Result<u64, String> {
    use weave_infrastructure::db::app_config;

    let segments = segments.clamp(1, 32);
    weave_infrastructure::parallel_download::set_max_segments_override(segments);
    app_config::set(&state.pool, DOWNLOAD_SEGMENTS_KEY, &segments.to_string())
        .await
        .map_err(|e| e.to_string())?;
    Ok(segments)
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
/// `sha256` (je-li znám, např. z HF katalogu) se po stažení ověří.
#[tauri::command]
pub async fn download_model(
    model_id: String,
    source_url: String,
    sha256: Option<String>,
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
        .download(&model_id, &source_url, sha256, tx)
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

/// Vrátí aktuální složku pro stahování modelů.
#[tauri::command]
pub async fn get_models_dir(state: State<'_, AppState>) -> Result<String, String> {
    state
        .model_manager
        .models_dir()
        .await
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
}

/// Změní složku pro stahování modelů — existující modely se do ní přesunou,
/// takže je uživatel po přepnutí nemusí stahovat znovu. Uloží se i do
/// `app_config`, ať se použije i po restartu appky.
#[tauri::command]
pub async fn set_models_dir(dir: String, state: State<'_, AppState>) -> Result<(), String> {
    use weave_infrastructure::db::app_config;

    state
        .model_manager
        .set_models_dir(std::path::PathBuf::from(&dir))
        .await
        .map_err(|e| e.to_string())?;

    app_config::set(&state.pool, MODELS_DIR_KEY, &dir)
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

    // Doporučené modely checksum v katalogu nemají → bez ověření.
    state
        .model_manager
        .download(&recommended.id, &recommended.download_url, None, tx)
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

    // Kolik vrstev na GPU podle skutečně volné VRAM — ne slepě "všechny",
    // ať model, co se nevejde, neskončí OOM/nepředvídatelně pomalým částečným
    // offloadem, ale rovnou celý v RAM.
    let free_vram_mb = state
        .model_manager
        .detect_gpu()
        .await
        .map_err(|e| e.to_string())?
        .map(|gpu| gpu.free_vram_mb)
        .unwrap_or(0);
    let gpu_layers = recommend_gpu_layers(recommended.size_bytes, free_vram_mb);

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
        &gpu_layers.to_string(),
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// Doporučí `gpu_layers` pro libovolný `.gguf` soubor podle jeho velikosti na
/// disku a aktuálně volné VRAM — používá se při ručním přepnutí na jiný
/// stažený model nebo výběru vlastního souboru, ať mají stejnou VRAM-aware
/// logiku jako jednoklikové stažení doporučeného modelu.
#[tauri::command]
pub async fn recommend_gpu_layers_for_path(
    path: String,
    state: State<'_, AppState>,
) -> Result<u32, String> {
    let size_bytes = tokio::fs::metadata(&path)
        .await
        .map_err(|e| format!("Nelze přečíst velikost souboru {path}: {e}"))?
        .len();
    let free_vram_mb = state
        .model_manager
        .detect_gpu()
        .await
        .map_err(|e| e.to_string())?
        .map(|gpu| gpu.free_vram_mb)
        .unwrap_or(0);
    Ok(recommend_gpu_layers(size_bytes, free_vram_mb))
}
