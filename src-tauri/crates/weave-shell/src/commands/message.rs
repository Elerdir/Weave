use tauri::{Emitter, State};
use weave_domain::message::Message;

use crate::state::AppState;

#[tauri::command]
pub async fn list_messages(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<Message>, String> {
    use std::sync::Arc;
    use uuid::Uuid;
    use weave_application::ports::conversation_repository::MessageRepository;
    use weave_domain::conversation::ConversationId;
    use weave_infrastructure::db::message_repo::SqliteMessageRepository;

    let repo = Arc::new(SqliteMessageRepository::new(state.pool.clone()));
    let uuid = Uuid::parse_str(&conversation_id).map_err(|e| e.to_string())?;
    let conv_id = ConversationId::from_uuid(uuid);
    repo.list_by_conversation(&conv_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn send_message(
    conversation_id: String,
    content: String,
    window: tauri::Window,
    state: State<'_, AppState>,
) -> Result<(), String> {
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use uuid::Uuid;
    use weave_application::{
        ports::llm_port::StreamChunk, use_cases::send_message::SendMessageUseCase,
    };
    use weave_domain::conversation::ConversationId;
    use weave_infrastructure::db::{
        conversation_repo::SqliteConversationRepository, message_repo::SqliteMessageRepository,
    };

    let uuid = Uuid::parse_str(&conversation_id).map_err(|e| e.to_string())?;
    let conv_id = ConversationId::from_uuid(uuid);

    let conv_repo = Arc::new(SqliteConversationRepository::new(state.pool.clone()));
    let msg_repo = Arc::new(SqliteMessageRepository::new(state.pool.clone()));

    let (tx, mut rx) = mpsc::channel::<StreamChunk>(128);

    let uc = SendMessageUseCase::new(
        conv_repo,
        msg_repo,
        state.llm.clone(),
        state.image_gen.clone(),
    );

    let window_clone = window.clone();
    tokio::spawn(async move {
        while let Some(chunk) = rx.recv().await {
            let _ = window_clone.emit("stream-chunk", &chunk);
        }
    });

    uc.execute(conv_id, content, tx)
        .await
        .map_err(|e| e.to_string())
}
