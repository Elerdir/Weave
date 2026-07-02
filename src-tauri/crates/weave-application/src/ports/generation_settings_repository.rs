use async_trait::async_trait;
use weave_domain::{conversation::ConversationId, generation_settings::GenerationSettings};

use crate::error::AppResult;

/// Ukládá parametry generování per konverzace (kontext, teplota, max tokenů).
#[async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait GenerationSettingsRepository: Send + Sync {
    /// Vrátí uložené parametry; pro konverzaci bez záznamu výchozí (vše None).
    async fn get(&self, id: &ConversationId) -> AppResult<GenerationSettings>;
    async fn set(&self, id: &ConversationId, settings: &GenerationSettings) -> AppResult<()>;
}
