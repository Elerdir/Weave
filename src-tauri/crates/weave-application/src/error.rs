use thiserror::Error;
use weave_domain::error::DomainError;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Doménová chyba: {0}")]
    Domain(#[from] DomainError),

    #[error("Chyba úložiště: {0}")]
    Repository(String),

    #[error("Chyba LLM: {0}")]
    Llm(String),

    #[error("Chyba ComfyUI: {0}")]
    ComfyUi(String),

    #[error("Chyba keychain: {0}")]
    Keychain(String),

    #[error("Neočekávaná chyba: {0}")]
    Unexpected(#[from] anyhow::Error),
}

pub type AppResult<T> = Result<T, AppError>;
