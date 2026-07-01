use std::sync::Arc;
use tauri::State;
use weave_application::use_cases::personas::PersonaUseCase;
use weave_domain::persona::Persona;
use weave_infrastructure::db::persona_repo::SqlitePersonaRepository;

use crate::state::AppState;

fn make_uc(state: &AppState) -> PersonaUseCase {
    PersonaUseCase::new(Arc::new(SqlitePersonaRepository::new(state.pool.clone())))
}

#[tauri::command]
pub async fn list_personas(state: State<'_, AppState>) -> Result<Vec<Persona>, String> {
    make_uc(&state).list_all().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_persona(
    name: String,
    icon: String,
    system_prompt: String,
    state: State<'_, AppState>,
) -> Result<Persona, String> {
    make_uc(&state)
        .create(name, icon, system_prompt)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_persona(id: String, state: State<'_, AppState>) -> Result<(), String> {
    make_uc(&state).delete(&id).await.map_err(|e| e.to_string())
}

/// Nastaví (nebo zruší) personu konverzace.
#[tauri::command]
pub async fn set_conversation_persona(
    conversation_id: String,
    persona_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    use uuid::Uuid;
    use weave_application::ports::conversation_repository::ConversationRepository;
    use weave_domain::conversation::ConversationId;
    use weave_infrastructure::db::conversation_repo::SqliteConversationRepository;

    let uuid = Uuid::parse_str(&conversation_id).map_err(|e| e.to_string())?;
    let conv_id = ConversationId::from_uuid(uuid);
    let repo = SqliteConversationRepository::new(state.pool.clone());

    let mut conversation = repo
        .find_by_id(&conv_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Konverzace neexistuje".to_string())?;

    conversation.set_persona(persona_id);
    repo.save(&conversation).await.map_err(|e| e.to_string())
}
