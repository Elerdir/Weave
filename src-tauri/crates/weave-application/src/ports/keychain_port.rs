use async_trait::async_trait;

use crate::error::AppResult;

#[derive(Debug, Clone, PartialEq)]
pub enum ApiService {
    Mistral,
    CivitAi,
    HuggingFace,
}

impl ApiService {
    pub fn key_name(&self) -> &'static str {
        match self {
            ApiService::Mistral => "weave.mistral.api_key",
            ApiService::CivitAi => "weave.civitai.api_key",
            ApiService::HuggingFace => "weave.huggingface.api_key",
        }
    }
}

#[async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait KeychainPort: Send + Sync {
    async fn store(&self, service: &ApiService, token: &str) -> AppResult<()>;
    async fn retrieve(&self, service: &ApiService) -> AppResult<Option<String>>;
    async fn delete(&self, service: &ApiService) -> AppResult<()>;
}
