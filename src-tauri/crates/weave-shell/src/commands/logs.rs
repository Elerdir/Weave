use tauri::{Manager, State};
use weave_infrastructure::logs::{read_logs, LogEntry, LogFilter};

use crate::state::AppState;

/// Vrátí posledních `limit` záznamů aplikačního logu po aplikaci filtrů.
/// Čtení souborů běží mimo async runtime (spawn_blocking).
#[tauri::command]
pub async fn get_app_logs(
    min_level: Option<String>,
    target: Option<String>,
    search: Option<String>,
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<LogEntry>, String> {
    let dir = state.log_dir.clone();
    let filter = LogFilter {
        min_level,
        target,
        search,
        limit,
    };
    tokio::task::spawn_blocking(move || read_logs(&dir, &filter))
        .await
        .map_err(|e| format!("Čtení logů selhalo: {e}"))
}

/// Otevře log viewer v samostatném velkém okně (nebo zaostří už otevřené).
/// Okno načte stejný frontend s `?view=logs` — vykreslí se jen viewer.
#[tauri::command]
pub async fn open_log_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("logs") {
        win.show().map_err(|e| e.to_string())?;
        win.unminimize().map_err(|e| e.to_string())?;
        win.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    tauri::WebviewWindowBuilder::new(
        &app,
        "logs",
        tauri::WebviewUrl::App("index.html?view=logs".into()),
    )
    .title("Weave — Logy")
    .inner_size(1280.0, 860.0)
    .min_inner_size(800.0, 500.0)
    .build()
    .map_err(|e| format!("Otevření okna s logy selhalo: {e}"))?;
    Ok(())
}
