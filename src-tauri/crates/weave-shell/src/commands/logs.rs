use tauri::State;
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
