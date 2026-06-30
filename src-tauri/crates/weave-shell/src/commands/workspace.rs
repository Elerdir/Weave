use std::sync::Arc;
use tauri::{State, Window};
use tokio::sync::mpsc;
use weave_application::use_cases::workspace::{IndexProgress, WorkspaceUseCase};
use weave_domain::workspace::WorkspaceEntry;
use weave_infrastructure::{
    db::workspace_config,
    workspace::{fs_adapter::NativeFileSystem, workspace_repo::SqliteWorkspaceRepository},
};

use crate::state::AppState;

fn make_uc(state: &AppState) -> WorkspaceUseCase {
    WorkspaceUseCase::new(
        Arc::new(NativeFileSystem),
        Arc::new(SqliteWorkspaceRepository::new(state.pool.clone())),
    )
}

/// Otevře systémový dialog pro výběr složky a vrátí cestu.
#[tauri::command]
pub async fn pick_workspace_folder() -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    // Volat z Tauri window contextu — dialog vrátí Option<PathBuf>
    // Implementace přes tauri::async_runtime protože dialog je sync
    // Zde vrátíme None jako placeholder — frontend volá tauri-plugin-dialog přímo
    Ok(None)
}

/// Uloží cestu workspace do konfigurace.
#[tauri::command]
pub async fn set_workspace(path: String, state: State<'_, AppState>) -> Result<(), String> {
    workspace_config::set_workspace(&state.pool, &path)
        .await
        .map_err(|e| e.to_string())
}

/// Vrátí aktuální workspace cestu (nebo None).
#[tauri::command]
pub async fn get_workspace(state: State<'_, AppState>) -> Result<Option<String>, String> {
    workspace_config::get_workspace(&state.pool)
        .await
        .map_err(|e| e.to_string())
}

/// Spustí indexování workspace na pozadí — posílá progress eventy do okna.
#[tauri::command]
pub async fn index_workspace(
    path: String,
    window: Window,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let uc = make_uc(&state);
    let (tx, mut rx) = mpsc::channel::<IndexProgress>(64);

    let window_clone = window.clone();
    tokio::spawn(async move {
        while let Some(progress) = rx.recv().await {
            let event = match &progress {
                IndexProgress::Started { total } => {
                    serde_json::json!({ "type": "started", "total": total })
                }
                IndexProgress::File { path, indexed, total } => {
                    serde_json::json!({ "type": "file", "path": path, "indexed": indexed, "total": total })
                }
                IndexProgress::Done { indexed, skipped } => {
                    serde_json::json!({ "type": "done", "indexed": indexed, "skipped": skipped })
                }
                IndexProgress::Error(e) => {
                    serde_json::json!({ "type": "error", "message": e })
                }
            };
            let _ = window_clone.emit("workspace-index-progress", event);
        }
    });

    uc.index(&path, tx).await.map_err(|e| e.to_string())
}

/// Vrátí přímé potomky složky (lazy tree loading).
#[tauri::command]
pub async fn list_workspace_children(
    path: String,
    state: State<'_, AppState>,
) -> Result<Vec<WorkspaceEntry>, String> {
    make_uc(&state)
        .list_children(&path)
        .await
        .map_err(|e| e.to_string())
}

/// Přečte obsah souboru.
#[tauri::command]
pub async fn read_workspace_file(
    path: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    make_uc(&state)
        .read_file(&path)
        .await
        .map_err(|e| e.to_string())
}

/// Zapíše obsah souboru.
#[tauri::command]
pub async fn write_workspace_file(
    path: String,
    content: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    make_uc(&state)
        .write_file(&path, &content)
        .await
        .map_err(|e| e.to_string())
}

/// Vytvoří soubor nebo složku.
#[tauri::command]
pub async fn create_workspace_entry(
    path: String,
    is_dir: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    make_uc(&state)
        .create(&path, is_dir)
        .await
        .map_err(|e| e.to_string())
}

/// Smaže soubor nebo složku.
#[tauri::command]
pub async fn delete_workspace_entry(
    path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    make_uc(&state)
        .delete(&path)
        .await
        .map_err(|e| e.to_string())
}

/// Přejmenuje soubor nebo složku.
#[tauri::command]
pub async fn rename_workspace_entry(
    from: String,
    to: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    make_uc(&state)
        .rename(&from, &to)
        .await
        .map_err(|e| e.to_string())
}

/// Full-text hledání ve workspace indexu.
#[tauri::command]
pub async fn search_workspace(
    query: String,
    limit: Option<u32>,
    state: State<'_, AppState>,
) -> Result<Vec<weave_domain::workspace::IndexedFile>, String> {
    make_uc(&state)
        .search(&query, limit.unwrap_or(20))
        .await
        .map_err(|e| e.to_string())
}
