use tauri::State;
use weave_application::ports::model_manager_port::{GpuInfo, LocalModel};

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
