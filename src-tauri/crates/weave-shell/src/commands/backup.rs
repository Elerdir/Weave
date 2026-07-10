//! Záloha a obnova uživatelských dat (Nastavení → Záloha).

use tauri::{AppHandle, Manager, State};

use crate::state::AppState;

fn data_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("Nepodařilo se zjistit datovou složku: {e}"))
}

/// Vytvoří ZIP zálohu (DB + referenční fotky) do zvoleného souboru.
/// Vrací velikost zálohy v bajtech.
#[tauri::command]
pub async fn export_backup(
    dest: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<u64, String> {
    let data_dir = data_dir(&app)?;
    weave_infrastructure::backup::create_backup(&state.pool, &data_dir, std::path::Path::new(&dest))
        .await
        .map_err(|e| e.to_string())
}

/// Připraví obnovu ze ZIP zálohy — data se vymění při příštím startu
/// aplikace (otevřenou databázi nelze přepsat za běhu). Frontend po
/// úspěchu nabídne restart.
#[tauri::command]
pub async fn import_backup(src: String, app: AppHandle) -> Result<(), String> {
    let data_dir = data_dir(&app)?;
    weave_infrastructure::backup::stage_restore(&data_dir, std::path::Path::new(&src))
        .map_err(|e| e.to_string())
}
