use tauri::{Manager, State};
use weave_domain::subject::{Subject, SubjectImage};
use weave_infrastructure::db::subject_repo::SqliteSubjectRepository;

use crate::state::AppState;

fn repo(state: &State<'_, AppState>) -> SqliteSubjectRepository {
    SqliteSubjectRepository::new(state.pool.clone())
}

#[tauri::command]
pub async fn list_subjects(state: State<'_, AppState>) -> Result<Vec<Subject>, String> {
    repo(&state).list().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_subject(name: String, state: State<'_, AppState>) -> Result<Subject, String> {
    repo(&state)
        .create(name.trim())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn rename_subject(
    id: String,
    name: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    repo(&state)
        .rename(&id, name.trim())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_subject_notes(
    id: String,
    notes: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    repo(&state)
        .set_notes(&id, &notes)
        .await
        .map_err(|e| e.to_string())
}

/// Smaže postavu i s fotkami (záznamy kaskádou, soubory best-effort z disku).
#[tauri::command]
pub async fn delete_subject(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let paths = repo(&state).delete(&id).await.map_err(|e| e.to_string())?;
    for path in paths {
        let _ = std::fs::remove_file(&path);
    }
    Ok(())
}

/// Přidá k postavě fotku: zdrojový soubor se zkopíruje do spravovaného úložiště
/// (přežije přesun/smazání originálu) a zaeviduje.
#[tauri::command]
pub async fn add_subject_image(
    subject_id: String,
    source_path: String,
    state: State<'_, AppState>,
) -> Result<SubjectImage, String> {
    let stored = state
        .attachment_store
        .store_reference_image(&source_path)
        .await
        .map_err(|e| e.to_string())?;
    repo(&state)
        .add_image(&subject_id, &stored.path, &stored.mime)
        .await
        .map_err(|e| e.to_string())
}

/// Odebere fotku z postavy a smaže její soubor z disku.
#[tauri::command]
pub async fn remove_subject_image(
    image_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if let Some(path) = repo(&state)
        .remove_image(&image_id)
        .await
        .map_err(|e| e.to_string())?
    {
        let _ = std::fs::remove_file(&path);
    }
    Ok(())
}

/// Otevře správu postav v samostatném okně (nebo zaostří už otevřené).
#[tauri::command]
pub async fn open_subjects_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("subjects") {
        win.show().map_err(|e| e.to_string())?;
        win.unminimize().map_err(|e| e.to_string())?;
        win.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    tauri::WebviewWindowBuilder::new(
        &app,
        "subjects",
        tauri::WebviewUrl::App("index.html?view=subjects".into()),
    )
    .title("Weave — Postavy")
    .inner_size(900.0, 720.0)
    .min_inner_size(560.0, 420.0)
    .build()
    .map_err(|e| format!("Otevření správy postav selhalo: {e}"))?;
    Ok(())
}
