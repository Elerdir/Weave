use std::sync::Arc;

use serde::Serialize;
use tauri::{Emitter, State};
use tokio::sync::mpsc;
use weave_application::{
    ports::llm_port::StreamChunk, use_cases::send_message::SendMessageUseCase,
};
use weave_domain::{conversation::ConversationId, message::Message};

use crate::state::AppState;

#[derive(Debug, Serialize)]
struct StreamEvent {
    conversation_id: String,
    chunk: StreamChunk,
}

#[tauri::command]
pub async fn list_messages(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<Message>, String> {
    use weave_application::ports::conversation_repository::MessageRepository;
    use weave_infrastructure::db::message_repo::SqliteMessageRepository;

    let repo = Arc::new(SqliteMessageRepository::new(state.pool.clone()));
    let conv_id = parse_conversation_id(&conversation_id)?;
    repo.list_by_conversation(&conv_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn send_message(
    conversation_id: String,
    content: String,
    file_refs: Option<Vec<String>>,
    reference_images: Option<Vec<String>>,
    reference_preservation: Option<String>,
    translate_image_prompt: Option<bool>,
    window: tauri::Window,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let conv_id = parse_conversation_id(&conversation_id)?;
    let uc = build_use_case(&state, &conv_id).await;
    let tx = spawn_stream_forwarder(window, &state, conversation_id);

    uc.execute(
        conv_id,
        content,
        file_refs.unwrap_or_default(),
        reference_images.unwrap_or_default(),
        reference_preservation,
        translate_image_prompt.unwrap_or(true),
        tx,
    )
    .await
    .map_err(|e| e.to_string())
}

/// Znovu vygeneruje poslední odpověď asistenta v konverzaci.
#[tauri::command]
pub async fn regenerate_response(
    conversation_id: String,
    window: tauri::Window,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let conv_id = parse_conversation_id(&conversation_id)?;
    let uc = build_use_case(&state, &conv_id).await;
    let tx = spawn_stream_forwarder(window, &state, conversation_id);

    uc.regenerate(conv_id, tx).await.map_err(|e| e.to_string())
}

/// „Poslat znovu": smaže vše po dané zprávě uživatele a vygeneruje
/// čerstvou odpověď (stream jde jako u send_message).
#[tauri::command]
pub async fn resend_message(
    conversation_id: String,
    message_id: String,
    window: tauri::Window,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let conv_id = parse_conversation_id(&conversation_id)?;
    let msg_id = parse_message_id(&message_id)?;
    let uc = build_use_case(&state, &conv_id).await;
    let tx = spawn_stream_forwarder(window, &state, conversation_id);

    uc.resend(conv_id, msg_id, tx)
        .await
        .map_err(|e| e.to_string())
}

/// „Upravit a poslat": smaže původní zprávu a vše po ní — nová verze
/// dotazu se pak posílá běžným send_message.
#[tauri::command]
pub async fn truncate_conversation_from(
    conversation_id: String,
    message_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    use weave_application::ports::conversation_repository::MessageRepository;
    use weave_infrastructure::db::message_repo::SqliteMessageRepository;

    let conv_id = parse_conversation_id(&conversation_id)?;
    let msg_id = parse_message_id(&message_id)?;
    SqliteMessageRepository::new(state.pool.clone())
        .delete_messages_from(&conv_id, &msg_id)
        .await
        .map_err(|e| e.to_string())
}

/// Úprava vygenerovaného obrázku (img2img): instrukce + cesta k obrázku,
/// stream jde jako u send_message.
#[tauri::command]
pub async fn edit_image_message(
    conversation_id: String,
    content: String,
    init_image: String,
    window: tauri::Window,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let conv_id = parse_conversation_id(&conversation_id)?;
    let uc = build_use_case(&state, &conv_id).await;
    let tx = spawn_stream_forwarder(window, &state, conversation_id);

    uc.edit_image(conv_id, content, init_image, tx)
        .await
        .map_err(|e| e.to_string())
}

/// Vygeneruje krátký název konverzace z první výměny (LLM) a uloží ho.
/// Kdy se volá, rozhoduje frontend — jen u konverzací s výchozím názvem.
#[tauri::command]
pub async fn auto_title_conversation(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    use weave_application::use_cases::auto_title::AutoTitleUseCase;
    use weave_infrastructure::db::{
        conversation_repo::SqliteConversationRepository, message_repo::SqliteMessageRepository,
    };

    let conv_id = parse_conversation_id(&conversation_id)?;
    let llm = super::settings::resolve_llm(&state).await;

    AutoTitleUseCase::new(
        Arc::new(SqliteConversationRepository::new(state.pool.clone())),
        Arc::new(SqliteMessageRepository::new(state.pool.clone())),
        llm,
    )
    .execute(conv_id, "mistral-small-latest".into())
    .await
    .map_err(|e| e.to_string())
}

/// Zhustí konverzaci: LLM shrne historii a ta se nahradí souhrnem —
/// kontextové okno se uvolní, ale podstatná „paměť" zůstane. Vrací souhrn.
#[tauri::command]
pub async fn compact_conversation(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    use weave_application::use_cases::compact_conversation::CompactConversationUseCase;
    use weave_infrastructure::db::message_repo::SqliteMessageRepository;

    let conv_id = parse_conversation_id(&conversation_id)?;
    let msg_repo = Arc::new(SqliteMessageRepository::new(state.pool.clone()));
    let llm = super::settings::resolve_llm(&state).await;

    CompactConversationUseCase::new(msg_repo, llm)
        .execute(conv_id, "mistral-small-latest".into())
        .await
        .map_err(|e| e.to_string())
}

/// Vrátí per-konverzační parametry generování (posuvníky v chatu).
#[tauri::command]
pub async fn get_conversation_settings(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<weave_domain::generation_settings::GenerationSettings, String> {
    use weave_application::ports::generation_settings_repository::GenerationSettingsRepository;
    use weave_infrastructure::db::generation_settings_repo::SqliteGenerationSettingsRepository;

    let conv_id = parse_conversation_id(&conversation_id)?;
    SqliteGenerationSettingsRepository::new(state.pool.clone())
        .get(&conv_id)
        .await
        .map_err(|e| e.to_string())
}

/// Uloží per-konverzační parametry generování.
#[tauri::command]
pub async fn set_conversation_settings(
    conversation_id: String,
    mut settings: weave_domain::generation_settings::GenerationSettings,
    state: State<'_, AppState>,
) -> Result<(), String> {
    use weave_application::ports::generation_settings_repository::GenerationSettingsRepository;
    use weave_infrastructure::db::generation_settings_repo::SqliteGenerationSettingsRepository;

    let conv_id = parse_conversation_id(&conversation_id)?;
    if let Some(backend) = settings.runtime_backend.as_deref().map(str::trim) {
        match backend {
            "" => settings.runtime_backend = None,
            "default" | "mistral" | "local" | "embedded" | "openvino_npu" => {
                settings.runtime_backend = Some(backend.to_string());
            }
            other => return Err(format!("Neznamy runtime backend: {other}")),
        }
    }
    SqliteGenerationSettingsRepository::new(state.pool.clone())
        .set(&conv_id, &settings)
        .await
        .map_err(|e| e.to_string())
}

/// Zastaví právě běžící generování odpovědi (pokud nějaké běží).
#[tauri::command]
pub fn stop_generation(state: State<'_, AppState>) {
    if let Some(token) = state
        .active_generation
        .lock()
        .expect("active_generation mutex poisoned")
        .take()
    {
        token.cancel();
    }
}

fn parse_conversation_id(raw: &str) -> Result<ConversationId, String> {
    let uuid = uuid::Uuid::parse_str(raw).map_err(|e| e.to_string())?;
    Ok(ConversationId::from_uuid(uuid))
}

fn parse_message_id(raw: &str) -> Result<weave_domain::message::MessageId, String> {
    let uuid = uuid::Uuid::parse_str(raw).map_err(|e| e.to_string())?;
    Ok(weave_domain::message::MessageId::from_uuid(uuid))
}

/// Sestaví use case se všemi závislostmi (LLM backend dle aktuálního nastavení).
async fn build_use_case(
    state: &State<'_, AppState>,
    conversation_id: &ConversationId,
) -> SendMessageUseCase {
    use weave_application::ports::generation_settings_repository::GenerationSettingsRepository;
    use weave_infrastructure::{
        db::{
            conversation_repo::SqliteConversationRepository,
            generation_settings_repo::SqliteGenerationSettingsRepository,
            message_repo::SqliteMessageRepository, persona_repo::SqlitePersonaRepository,
        },
        workspace::workspace_repo::SqliteWorkspaceRepository,
    };

    let generation_settings_repo = SqliteGenerationSettingsRepository::new(state.pool.clone());
    let runtime_backend = generation_settings_repo
        .get(conversation_id)
        .await
        .ok()
        .and_then(|settings| settings.runtime_backend);
    let llm =
        crate::commands::settings::resolve_llm_with_backend(state, runtime_backend.as_deref())
            .await;

    // CivitAI token (volitelný) — bez něj funguje hledání LoRA, jen
    // stažení některých modelů může selhat.
    let civitai_token = state
        .keychain
        .retrieve(&weave_application::ports::keychain_port::ApiService::CivitAi)
        .await
        .ok()
        .flatten();

    SendMessageUseCase::new(
        Arc::new(SqliteConversationRepository::new(state.pool.clone())),
        Arc::new(SqliteMessageRepository::new(state.pool.clone())),
        llm,
        state.image_gen.clone(),
        Arc::new(SqliteWorkspaceRepository::new(state.pool.clone())),
        Arc::new(SqlitePersonaRepository::new(state.pool.clone())),
        state.attachment_store.clone(),
        Arc::new(generation_settings_repo),
        state.comfy_installer.clone(),
        Arc::new(weave_infrastructure::civitai::CivitAiClient::new(
            civitai_token,
        )),
    )
}

/// Vytvoří kanál pro stream chunky, zaregistruje CancellationToken tohoto
/// generování (příkaz stop_generation ho zruší) a spustí přeposílací smyčku
/// do window eventů. Vrací odesílací konec pro use case.
fn spawn_stream_forwarder(
    window: tauri::Window,
    state: &State<'_, AppState>,
    conversation_id: String,
) -> mpsc::Sender<StreamChunk> {
    let (tx, mut rx) = mpsc::channel::<StreamChunk>(128);

    // Případné předchozí generování zrušíme — běží vždy nejvýš jedno.
    let cancel = tokio_util::sync::CancellationToken::new();
    if let Some(old) = state
        .active_generation
        .lock()
        .expect("active_generation mutex poisoned")
        .replace(cancel.clone())
    {
        old.cancel();
    }

    tokio::spawn(async move {
        loop {
            tokio::select! {
                maybe_chunk = rx.recv() => match maybe_chunk {
                    Some(chunk) => {
                        let event = StreamEvent {
                            conversation_id: conversation_id.clone(),
                            chunk,
                        };
                        let _ = window.emit("stream-chunk", &event);
                    }
                    None => break,
                },
                _ = cancel.cancelled() => {
                    // Uzavřením rx přestanou klienti posílat tokeny a inference
                    // skončí. Frontend dostane Done, aby rozepsanou odpověď
                    // korektně uzavřel (částečný text zůstane zachovaný).
                    let event = StreamEvent {
                        conversation_id: conversation_id.clone(),
                        chunk: StreamChunk::Done(Default::default()),
                    };
                    let _ = window.emit("stream-chunk", &event);
                    break;
                }
            }
        }
    });

    tx
}
