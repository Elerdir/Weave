use std::sync::Arc;
use tauri::State;
use weave_application::ports::conversation_repository::ConversationRepository;
use weave_application::use_cases::create_conversation::CreateConversationUseCase;
use weave_application::use_cases::export_conversation::{ExportConversationUseCase, ExportFormat};
use weave_domain::conversation::Conversation;
use weave_infrastructure::db::{
    conversation_repo::SqliteConversationRepository, message_repo::SqliteMessageRepository,
};

use crate::state::AppState;

fn parse_conv_id(id: &str) -> Result<weave_domain::conversation::ConversationId, String> {
    use uuid::Uuid;
    use weave_domain::conversation::ConversationId;
    let uuid = Uuid::parse_str(id).map_err(|e| e.to_string())?;
    Ok(ConversationId::from_uuid(uuid))
}

fn make_export_uc(state: &AppState) -> ExportConversationUseCase {
    ExportConversationUseCase::new(
        Arc::new(SqliteConversationRepository::new(state.pool.clone())),
        Arc::new(SqliteMessageRepository::new(state.pool.clone())),
    )
}

#[tauri::command]
pub async fn list_conversations(state: State<'_, AppState>) -> Result<Vec<Conversation>, String> {
    let repo = Arc::new(SqliteConversationRepository::new(state.pool.clone()));
    repo.list_all().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_conversation(
    title: String,
    state: State<'_, AppState>,
) -> Result<Conversation, String> {
    let repo = Arc::new(SqliteConversationRepository::new(state.pool.clone()));
    let uc = CreateConversationUseCase::new(repo);
    uc.execute(title).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_conversation(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let repo = Arc::new(SqliteConversationRepository::new(state.pool.clone()));
    let conv_id = parse_conv_id(&id)?;
    repo.delete(&conv_id).await.map_err(|e| e.to_string())
}

/// Navrhne název souboru pro export (bezpečný, s příponou).
#[tauri::command]
pub async fn suggest_export_filename(
    conversation_id: String,
    format: ExportFormat,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let conv_id = parse_conv_id(&conversation_id)?;
    make_export_uc(&state)
        .suggested_filename(&conv_id, format)
        .await
        .map_err(|e| e.to_string())
}

/// Vyexportuje konverzaci do souboru na dané cestě.
#[tauri::command]
pub async fn export_conversation(
    conversation_id: String,
    format: ExportFormat,
    output_path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let conv_id = parse_conv_id(&conversation_id)?;
    let content = make_export_uc(&state)
        .render(&conv_id, format)
        .await
        .map_err(|e| e.to_string())?;
    std::fs::write(&output_path, content).map_err(|e| e.to_string())
}
