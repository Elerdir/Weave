use weave_domain::{conversation::Conversation, message::Message};

use crate::error::AppResult;

/// Vyrenderuje konverzaci do PDF (bytes). Implementace v infrastructure
/// (genpdf + bundled font s diakritikou).
pub trait PdfExporter: Send + Sync {
    fn render(&self, conversation: &Conversation, messages: &[Message]) -> AppResult<Vec<u8>>;
}
