use std::sync::Arc;

use crate::{
    error::AppResult,
    ports::keychain_port::{ApiService, KeychainPort},
};

pub struct ManageApiKeysUseCase {
    keychain: Arc<dyn KeychainPort>,
}

impl ManageApiKeysUseCase {
    pub fn new(keychain: Arc<dyn KeychainPort>) -> Self {
        Self { keychain }
    }

    pub async fn store_token(&self, service: ApiService, token: &str) -> AppResult<()> {
        self.keychain.store(&service, token).await?;
        tracing::info!(service = service.key_name(), "API token uložen do keychain");
        Ok(())
    }

    pub async fn has_token(&self, service: &ApiService) -> AppResult<bool> {
        Ok(self.keychain.retrieve(service).await?.is_some())
    }

    pub async fn delete_token(&self, service: ApiService) -> AppResult<()> {
        self.keychain.delete(&service).await
    }

    /// Vrátí maskovaný token (jen pro zobrazení v UI — nikdy plaintext).
    pub async fn masked_token(&self, service: &ApiService) -> AppResult<Option<String>> {
        Ok(self.keychain.retrieve(service).await?.map(|t| {
            let visible = t.chars().take(4).collect::<String>();
            format!("{visible}••••••••••••")
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::keychain_port::MockKeychainPort;

    #[tokio::test]
    async fn stores_and_masks_token() {
        let mut mock = MockKeychainPort::new();
        mock.expect_store()
            .times(1)
            .returning(|_, _| Box::pin(async { Ok(()) }));
        mock.expect_retrieve()
            .times(1)
            .returning(|_| Box::pin(async { Ok(Some("sk-abc123xyz".into())) }));

        let uc = ManageApiKeysUseCase::new(Arc::new(mock));
        uc.store_token(ApiService::Mistral, "sk-abc123xyz")
            .await
            .unwrap();

        let masked = uc.masked_token(&ApiService::Mistral).await.unwrap();
        assert!(masked.is_some());
        let m = masked.unwrap();
        assert!(m.starts_with("sk-a"));
        assert!(m.contains('•'));
        assert!(!m.contains("abc123xyz"));
    }
}
