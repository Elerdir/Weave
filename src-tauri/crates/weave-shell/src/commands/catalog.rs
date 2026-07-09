//! Vyhledávání modelů na HuggingFace Hub — read-only katalog. Stahování pak
//! jde přes existující `download_model` (model_manager + progress eventy).

use tauri::State;
use weave_application::ports::keychain_port::ApiService;
use weave_application::ports::model_catalog_port::{CatalogFile, CatalogModel};

use crate::state::AppState;

/// HF token z keychainu — jen best-effort: bez tokenu hledání funguje taky
/// (gated repa a vyšší rate limity jsou bonus), chyba keychainu nesmí
/// vyhledávání shodit.
async fn hf_token(state: &State<'_, AppState>) -> Option<String> {
    state
        .keychain
        .retrieve(&ApiService::HuggingFace)
        .await
        .ok()
        .flatten()
        .filter(|t| !t.trim().is_empty())
}

/// Fulltext hledání GGUF repozitářů na HuggingFace (řazeno podle stažení).
#[tauri::command]
pub async fn search_model_catalog(
    query: String,
    state: State<'_, AppState>,
) -> Result<Vec<CatalogModel>, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(vec![]);
    }
    let token = hf_token(&state).await;
    state
        .model_catalog
        .search(trimmed, token.as_deref(), 20)
        .await
        .map_err(|e| e.to_string())
}

/// GGUF soubory (kvantizace) daného repa, seřazené od nejmenšího.
#[tauri::command]
pub async fn list_catalog_gguf_files(
    repo_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<CatalogFile>, String> {
    let token = hf_token(&state).await;
    state
        .model_catalog
        .list_gguf_files(&repo_id, token.as_deref())
        .await
        .map_err(|e| e.to_string())
}
