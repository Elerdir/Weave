use async_trait::async_trait;
use weave_domain::conversation::{Conversation, ConversationId};
use weave_domain::message::{Message, MessageId};

use crate::error::AppResult;

#[async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait ConversationRepository: Send + Sync {
    async fn save(&self, conversation: &Conversation) -> AppResult<()>;
    async fn find_by_id(&self, id: &ConversationId) -> AppResult<Option<Conversation>>;
    async fn list_all(&self) -> AppResult<Vec<Conversation>>;
    async fn delete(&self, id: &ConversationId) -> AppResult<()>;
    async fn search(&self, query: &str) -> AppResult<Vec<Conversation>>;
}

#[async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait MessageRepository: Send + Sync {
    async fn save(&self, message: &Message) -> AppResult<()>;
    async fn list_by_conversation(&self, id: &ConversationId) -> AppResult<Vec<Message>>;
    async fn delete_by_conversation(&self, id: &ConversationId) -> AppResult<()>;
    /// Smaže zprávy následující po poslední zprávě uživatele (typicky poslední
    /// odpověď asistenta) — základ pro „znovu vygenerovat“.
    async fn delete_trailing_assistant_messages(&self, id: &ConversationId) -> AppResult<()>;
    /// Smaže všechny zprávy PO dané zprávě (danou zprávu nechá) —
    /// „poslat znovu": konverzace se vrátí do stavu těsně po tomto dotazu.
    async fn delete_messages_after(
        &self,
        conversation_id: &ConversationId,
        message_id: &MessageId,
    ) -> AppResult<()>;
    /// Smaže danou zprávu a všechny po ní — „upravit a poslat":
    /// původní dotaz nahradí nová verze.
    async fn delete_messages_from(
        &self,
        conversation_id: &ConversationId,
        message_id: &MessageId,
    ) -> AppResult<()>;
}
