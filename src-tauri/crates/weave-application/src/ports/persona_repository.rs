use async_trait::async_trait;
use weave_domain::persona::Persona;

use crate::error::AppResult;

/// Úložiště vlastních (uživatelských) person. Vestavěné jsou v doméně.
#[async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait PersonaRepository: Send + Sync {
    async fn save(&self, persona: &Persona) -> AppResult<()>;
    async fn list_custom(&self) -> AppResult<Vec<Persona>>;
    async fn find_by_id(&self, id: &str) -> AppResult<Option<Persona>>;
    async fn delete(&self, id: &str) -> AppResult<()>;
}
