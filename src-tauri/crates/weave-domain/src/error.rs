use thiserror::Error;

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("Neplatný argument: {0}")]
    InvalidArgument(String),

    #[error("Entita nenalezena: {0}")]
    NotFound(String),

    #[error("Operace není povolena: {0}")]
    Forbidden(String),
}
