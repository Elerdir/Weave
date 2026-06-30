use std::sync::Arc;
use tauri::State;
use weave_application::ports::conversation_repository::ConversationRepository;
use weave_application::use_cases::create_conversation::CreateConversationUseCase;
use weave_domain::conversation::Conversation;
use weave_infrastructure::db::conversation_repo::SqliteConversationRepository;

use crate::state::AppState;

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
    use uuid::Uuid;
    use weave_domain::conversation::ConversationId;

    let repo = Arc::new(SqliteConversationRepository::new(state.pool.clone()));
    let uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let conv_id = ConversationId::from_uuid(uuid);
    repo.delete(&conv_id).await.map_err(|e| e.to_string())
}
