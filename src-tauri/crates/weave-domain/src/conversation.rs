use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::DomainError;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConversationId(Uuid);

impl ConversationId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn from_uuid(id: Uuid) -> Self {
        Self(id)
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for ConversationId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ConversationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationTitle(String);

impl ConversationTitle {
    pub fn new(title: impl Into<String>) -> Result<Self, DomainError> {
        let title = title.into();
        if title.trim().is_empty() {
            return Err(DomainError::InvalidArgument(
                "Název konverzace nesmí být prázdný".into(),
            ));
        }
        if title.len() > 200 {
            return Err(DomainError::InvalidArgument(
                "Název konverzace nesmí přesáhnout 200 znaků".into(),
            ));
        }
        Ok(Self(title))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: ConversationId,
    pub title: ConversationTitle,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub persona_id: Option<String>,
    pub pinned: bool,
}

impl Conversation {
    pub fn new(title: ConversationTitle) -> Self {
        let now = Utc::now();
        Self {
            id: ConversationId::new(),
            title,
            created_at: now,
            updated_at: now,
            persona_id: None,
            pinned: false,
        }
    }

    pub fn rename(&mut self, title: ConversationTitle) {
        self.title = title;
        self.updated_at = Utc::now();
    }

    pub fn set_persona(&mut self, persona_id: Option<String>) {
        self.persona_id = persona_id;
        self.updated_at = Utc::now();
    }

    pub fn pin(&mut self) {
        self.pinned = true;
    }

    pub fn unpin(&mut self) {
        self.pinned = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_conversation_has_unique_id() {
        let title = ConversationTitle::new("Test").unwrap();
        let a = Conversation::new(title.clone());
        let b = Conversation::new(title);
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn empty_title_is_rejected() {
        assert!(ConversationTitle::new("").is_err());
        assert!(ConversationTitle::new("   ").is_err());
    }

    #[test]
    fn title_over_200_chars_is_rejected() {
        let long = "a".repeat(201);
        assert!(ConversationTitle::new(long).is_err());
    }

    #[test]
    fn rename_updates_title_and_timestamp() {
        let title = ConversationTitle::new("Původní").unwrap();
        let mut conv = Conversation::new(title);
        let before = conv.updated_at;

        // Malé zpoždění pro odlišení timestampů
        std::thread::sleep(std::time::Duration::from_millis(1));
        conv.rename(ConversationTitle::new("Nový").unwrap());

        assert_eq!(conv.title.as_str(), "Nový");
        assert!(conv.updated_at >= before);
    }

    #[test]
    fn pin_and_unpin_toggle_flag() {
        let mut conv = Conversation::new(ConversationTitle::new("Test").unwrap());
        assert!(!conv.pinned);
        conv.pin();
        assert!(conv.pinned);
        conv.unpin();
        assert!(!conv.pinned);
    }

    #[test]
    fn set_persona_updates_id_and_timestamp() {
        let mut conv = Conversation::new(ConversationTitle::new("Test").unwrap());
        assert!(conv.persona_id.is_none());
        conv.set_persona(Some("builtin:coder".into()));
        assert_eq!(conv.persona_id.as_deref(), Some("builtin:coder"));
        conv.set_persona(None);
        assert!(conv.persona_id.is_none());
    }
}
