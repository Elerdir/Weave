use tauri::State;
use weave_application::{
    ports::keychain_port::ApiService, use_cases::manage_api_keys::ManageApiKeysUseCase,
};

use crate::state::AppState;

fn parse_service(service: &str) -> Result<ApiService, String> {
    match service {
        "mistral" => Ok(ApiService::Mistral),
        "civitai" => Ok(ApiService::CivitAi),
        "huggingface" => Ok(ApiService::HuggingFace),
        _ => Err(format!("Neznámá služba: {service}")),
    }
}

#[tauri::command]
pub async fn get_api_key_status(
    service: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let svc = parse_service(&service)?;
    let uc = ManageApiKeysUseCase::new(state.keychain.clone());
    uc.has_token(&svc).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn store_api_key(
    service: String,
    token: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let svc = parse_service(&service)?;
    let uc = ManageApiKeysUseCase::new(state.keychain.clone());
    uc.store_token(svc, &token).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_api_key(service: String, state: State<'_, AppState>) -> Result<(), String> {
    let svc = parse_service(&service)?;
    let uc = ManageApiKeysUseCase::new(state.keychain.clone());
    uc.delete_token(svc).await.map_err(|e| e.to_string())
}

/// Vrátí maskovaný token (jen prvních pár znaků + tečky) — pro zobrazení v UI.
#[tauri::command]
pub async fn get_masked_api_key(
    service: String,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let svc = parse_service(&service)?;
    let uc = ManageApiKeysUseCase::new(state.keychain.clone());
    uc.masked_token(&svc).await.map_err(|e| e.to_string())
}

/// Přečte hodnotu obecného nastavení.
#[tauri::command]
pub async fn get_app_setting(
    key: String,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    weave_infrastructure::db::app_config::get(&state.pool, &key)
        .await
        .map_err(|e| e.to_string())
}

/// Uloží hodnotu obecného nastavení.
#[tauri::command]
pub async fn set_app_setting(
    key: String,
    value: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    weave_infrastructure::db::app_config::set(&state.pool, &key, &value)
        .await
        .map_err(|e| e.to_string())
}

/// Ověří dostupnost ComfyUI serveru na dané URL.
#[tauri::command]
pub async fn test_comfyui_connection(url: String) -> Result<bool, String> {
    use weave_application::ports::image_gen_port::ImageGenPort;
    use weave_infrastructure::comfyui::ComfyUiClient;

    let client = ComfyUiClient::new(url);
    Ok(client.is_available().await)
}

/// Ověří dostupnost lokálního OpenAI-kompatibilního LLM serveru (llama.cpp).
#[tauri::command]
pub async fn test_local_llm_connection(url: String) -> Result<bool, String> {
    use weave_infrastructure::llm::local_client::LocalLlmClient;

    let client = LocalLlmClient::new(url);
    Ok(client.is_available().await)
}

pub const LLM_BACKEND_KEY: &str = "llm.backend";
pub const LLM_LOCAL_URL_KEY: &str = "llm.local_url";
pub const DEFAULT_LOCAL_URL: &str = "http://localhost:8080";
pub const LLM_MODEL_PATH_KEY: &str = "llm.model_path";
pub const LLM_GPU_LAYERS_KEY: &str = "llm.gpu_layers";
pub const LLM_CTX_KEY: &str = "llm.context_length";
/// Výchozí kontextové okno vestavěné inference. Vyvažuje délku konverzace
/// proti VRAM (KV cache roste s kontextem lineárně); v Nastavení jde změnit.
pub const DEFAULT_LLM_CTX: u32 = 8192;

/// Sestaví aktivní LLM klienta podle uloženého nastavení
/// (Mistral API / lokální llama.cpp server / vestavěná GPU inference).
pub async fn resolve_llm(
    state: &AppState,
) -> std::sync::Arc<dyn weave_application::ports::llm_port::LlmPort> {
    use std::sync::Arc;
    use weave_application::ports::keychain_port::ApiService;
    use weave_infrastructure::{
        db::app_config, llm::local_client::LocalLlmClient, llm::mistral_client::MistralClient,
    };

    let backend = app_config::get(&state.pool, LLM_BACKEND_KEY)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "mistral".to_string());

    // Vestavěná GPU inference (jen když je zkompilovaná feature llm-embedded).
    #[cfg(feature = "llm-embedded")]
    if backend == "embedded" {
        use weave_infrastructure::llm::embedded::EmbeddedLlamaClient;

        let path = app_config::get(&state.pool, LLM_MODEL_PATH_KEY)
            .await
            .ok()
            .flatten();
        if let Some(path) = path.filter(|p| !p.is_empty()) {
            let layers = app_config::get(&state.pool, LLM_GPU_LAYERS_KEY)
                .await
                .ok()
                .flatten()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(999);
            let n_ctx = app_config::get(&state.pool, LLM_CTX_KEY)
                .await
                .ok()
                .flatten()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(DEFAULT_LLM_CTX);
            tracing::info!(%path, layers, n_ctx, "Aktivuji vestavěnou GPU inferenci");
            return Arc::new(EmbeddedLlamaClient::new(path.into(), layers, n_ctx));
        }
        tracing::warn!("backend=embedded, ale není nastavena cesta k modelu → fallback Mistral");
    }

    if backend == "local" {
        let url = app_config::get(&state.pool, LLM_LOCAL_URL_KEY)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| DEFAULT_LOCAL_URL.to_string());
        return Arc::new(LocalLlmClient::new(url));
    }

    // Výchozí: Mistral API (klíč z keychain)
    let key = state
        .keychain
        .retrieve(&ApiService::Mistral)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    Arc::new(MistralClient::new(key))
}
