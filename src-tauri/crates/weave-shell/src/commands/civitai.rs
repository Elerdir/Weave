//! Prohlížeč CivitAI — hledání checkpointů/LoRA s náhledy a stažení do
//! ComfyUI models složek. Stahování posílá průběh do okna jako
//! `civitai-download-progress` (stejný vzor jako comfyui-install-progress).

use tauri::Emitter;
use tauri::{State, Window};
use tokio::sync::mpsc;
use weave_application::ports::comfy_installer_port::InstallProgress;
use weave_application::ports::keychain_port::ApiService;
use weave_application::ports::lora_catalog_port::{
    CatalogBrowseItem, ImageModelKind, LoraCatalogPort,
};

use crate::state::AppState;

/// CivitAI klient s tokenem z keychainu (best-effort — bez tokenu funguje
/// hledání i stažení většiny modelů).
async fn civitai_client(
    state: &State<'_, AppState>,
) -> weave_infrastructure::civitai::CivitAiClient {
    let token = state
        .keychain
        .retrieve(&ApiService::CivitAi)
        .await
        .ok()
        .flatten()
        .filter(|t| !t.trim().is_empty());
    weave_infrastructure::civitai::CivitAiClient::new(token)
}

/// Fulltext procházení CivitAI katalogu (checkpointy/LoRA), řazeno podle stažení.
#[tauri::command]
pub async fn browse_civitai(
    query: String,
    kind: ImageModelKind,
    base_model: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<CatalogBrowseItem>, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(vec![]);
    }
    civitai_client(&state)
        .await
        .browse(trimmed, kind, base_model.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// Stáhne checkpoint/LoRA do příslušné ComfyUI models složky.
/// Idempotentní — existující soubor se nestahuje znovu.
#[tauri::command]
pub async fn download_civitai_model(
    kind: ImageModelKind,
    file_name: String,
    download_url: String,
    window: Window,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let (tx, mut rx) = mpsc::channel::<InstallProgress>(64);

    let window_clone = window.clone();
    let file_for_events = file_name.clone();
    let forward = tokio::spawn(async move {
        while let Some(progress) = rx.recv().await {
            let event = match progress {
                InstallProgress::Step { name } => {
                    serde_json::json!({ "type": "step", "file": file_for_events, "name": name })
                }
                InstallProgress::Output(line) => {
                    serde_json::json!({ "type": "output", "file": file_for_events, "line": line })
                }
                InstallProgress::Done => {
                    serde_json::json!({ "type": "done", "file": file_for_events })
                }
                InstallProgress::Error(e) => {
                    serde_json::json!({ "type": "error", "file": file_for_events, "message": e })
                }
            };
            let _ = window_clone.emit("civitai-download-progress", event);
        }
    });

    let result = match kind {
        ImageModelKind::Checkpoint => {
            state
                .comfy_installer
                .ensure_checkpoint(&file_name, &download_url, tx)
                .await
        }
        ImageModelKind::Lora => {
            state
                .comfy_installer
                .ensure_lora(&file_name, &download_url, tx)
                .await
        }
    };
    let _ = forward.await;

    result.map_err(|e| e.to_string())?;
    let _ = window.emit(
        "civitai-download-progress",
        serde_json::json!({ "type": "done", "file": file_name }),
    );
    Ok(())
}
