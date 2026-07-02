use std::sync::Arc;

use tauri::{Emitter, State};
use tokio::sync::mpsc;
use weave_application::{
    ports::llm_port::StreamChunk, use_cases::send_message::SendMessageUseCase,
};
use weave_domain::{conversation::ConversationId, message::Message};

use crate::state::AppState;

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
pub async fn send_message(
    conversation_id: String,
    content: String,
    file_refs: Option<Vec<String>>,
    reference_images: Option<Vec<String>>,
    window: tauri::Window,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let conv_id = parse_conversation_id(&conversation_id)?;
    let uc = build_use_case(&state).await;
    let tx = spawn_stream_forwarder(window, &state);

    uc.execute(
        conv_id,
        content,
        file_refs.unwrap_or_default(),
        reference_images.unwrap_or_default(),
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
    let uc = build_use_case(&state).await;
    let tx = spawn_stream_forwarder(window, &state);

    uc.regenerate(conv_id, tx).await.map_err(|e| e.to_string())
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
    settings: weave_domain::generation_settings::GenerationSettings,
    state: State<'_, AppState>,
) -> Result<(), String> {
    use weave_application::ports::generation_settings_repository::GenerationSettingsRepository;
    use weave_infrastructure::db::generation_settings_repo::SqliteGenerationSettingsRepository;

    let conv_id = parse_conversation_id(&conversation_id)?;
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

/// Sestaví use case se všemi závislostmi (LLM backend dle aktuálního nastavení).
async fn build_use_case(state: &State<'_, AppState>) -> SendMessageUseCase {
    use weave_infrastructure::{
        db::{
            conversation_repo::SqliteConversationRepository,
            generation_settings_repo::SqliteGenerationSettingsRepository,
            message_repo::SqliteMessageRepository, persona_repo::SqlitePersonaRepository,
        },
        workspace::workspace_repo::SqliteWorkspaceRepository,
    };

    let llm = crate::commands::settings::resolve_llm(state).await;

    SendMessageUseCase::new(
        Arc::new(SqliteConversationRepository::new(state.pool.clone())),
        Arc::new(SqliteMessageRepository::new(state.pool.clone())),
        llm,
        state.image_gen.clone(),
        Arc::new(SqliteWorkspaceRepository::new(state.pool.clone())),
        Arc::new(SqlitePersonaRepository::new(state.pool.clone())),
        state.attachment_store.clone(),
        Arc::new(SqliteGenerationSettingsRepository::new(state.pool.clone())),
    )
}

/// Vytvoří kanál pro stream chunky, zaregistruje CancellationToken tohoto
/// generování (příkaz stop_generation ho zruší) a spustí přeposílací smyčku
/// do window eventů. Vrací odesílací konec pro use case.
fn spawn_stream_forwarder(
    window: tauri::Window,
    state: &State<'_, AppState>,
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
                        let _ = window.emit("stream-chunk", &chunk);
                    }
                    None => break,
                },
                _ = cancel.cancelled() => {
                    // Uzavřením rx přestanou klienti posílat tokeny a inference
                    // skončí. Frontend dostane Done, aby rozepsanou odpověď
                    // korektně uzavřel (částečný text zůstane zachovaný).
                    let _ = window.emit("stream-chunk", &StreamChunk::Done(Default::default()));
                    break;
                }
            }
        }
    });

    tx
}
