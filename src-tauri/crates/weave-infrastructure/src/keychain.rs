use async_trait::async_trait;
use keyring::Entry;
use weave_application::{
    error::{AppError, AppResult},
    ports::keychain_port::{ApiService, KeychainPort},
};

pub struct OsKeychain;

#[async_trait]
impl KeychainPort for OsKeychain {
    async fn store(&self, service: &ApiService, token: &str) -> AppResult<()> {
        let entry = Entry::new("weave", service.key_name())
            .map_err(|e| AppError::Keychain(e.to_string()))?;
        entry
            .set_password(token)
            .map_err(|e| AppError::Keychain(e.to_string()))
    }

    async fn retrieve(&self, service: &ApiService) -> AppResult<Option<String>> {
        let entry = Entry::new("weave", service.key_name())
            .map_err(|e| AppError::Keychain(e.to_string()))?;
        match entry.get_password() {
            Ok(pw) => Ok(Some(pw)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AppError::Keychain(e.to_string())),
        }
    }

    async fn delete(&self, service: &ApiService) -> AppResult<()> {
        let entry = Entry::new("weave", service.key_name())
            .map_err(|e| AppError::Keychain(e.to_string()))?;
        match entry.delete_credential() {
            Ok(_) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AppError::Keychain(e.to_string())),
        }
    }
}
