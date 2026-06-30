use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::conversation::ConversationId;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(Uuid);

impl MessageId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for MessageId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for MessageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Attachment {
    Image {
        path: String,
        mime: String,
    },
    Document {
        path: String,
        name: String,
        mime: String,
    },
}

/// Statistiky generování — tokeny/s atd.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GenerationStats {
    pub tokens_per_second: f64,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub model_id: String,
    pub backend: ModelBackend,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelBackend {
    #[default]
    MistralApi,
    LocalCuda,
    LocalMetal,
    LocalVulkan,
    LocalCpu,
    ComfyUi,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub conversation_id: ConversationId,
    pub role: Role,
    pub content: String,
    pub attachments: Vec<Attachment>,
    pub stats: Option<GenerationStats>,
    pub created_at: DateTime<Utc>,
}

impl Message {
    pub fn user(conversation_id: ConversationId, content: impl Into<String>) -> Self {
        Self {
            id: MessageId::new(),
            conversation_id,
            role: Role::User,
            content: content.into(),
            attachments: vec![],
            stats: None,
            created_at: Utc::now(),
        }
    }

    pub fn assistant(
        conversation_id: ConversationId,
        content: impl Into<String>,
        stats: Option<GenerationStats>,
    ) -> Self {
        Self {
            id: MessageId::new(),
            conversation_id,
            role: Role::Assistant,
            content: content.into(),
            attachments: vec![],
            stats,
            created_at: Utc::now(),
        }
    }

    pub fn with_attachments(mut self, attachments: Vec<Attachment>) -> Self {
        self.attachments = attachments;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_conv_id() -> ConversationId {
        ConversationId::new()
    }

    #[test]
    fn user_message_has_correct_role() {
        let msg = Message::user(dummy_conv_id(), "Ahoj");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content, "Ahoj");
        assert!(msg.attachments.is_empty());
        assert!(msg.stats.is_none());
    }

    #[test]
    fn assistant_message_stores_stats() {
        let stats = GenerationStats {
            tokens_per_second: 42.5,
            prompt_tokens: 10,
            completion_tokens: 50,
            model_id: "mistral-small".into(),
            backend: ModelBackend::MistralApi,
        };
        let msg = Message::assistant(dummy_conv_id(), "Odpověď", Some(stats.clone()));
        assert_eq!(msg.role, Role::Assistant);
        assert!(msg.stats.is_some());
        assert_eq!(msg.stats.unwrap().tokens_per_second, 42.5);
    }

    #[test]
    fn with_attachments_appends_correctly() {
        let msg = Message::user(dummy_conv_id(), "Viz příloha").with_attachments(vec![
            Attachment::Image {
                path: "/tmp/img.png".into(),
                mime: "image/png".into(),
            },
        ]);
        assert_eq!(msg.attachments.len(), 1);
    }
}
