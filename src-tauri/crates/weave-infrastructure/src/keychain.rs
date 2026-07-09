use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use keyring::Entry;
use std::path::PathBuf;
use weave_application::{
    error::{AppError, AppResult},
    ports::keychain_port::{ApiService, KeychainPort},
};

pub struct OsKeychain {
    fallback_dir: PathBuf,
}

impl OsKeychain {
    pub fn new(fallback_dir: impl Into<PathBuf>) -> Self {
        Self {
            fallback_dir: fallback_dir.into(),
        }
    }

    fn entry(service: &ApiService) -> AppResult<Entry> {
        Entry::new("weave", service.key_name()).map_err(|e| AppError::Keychain(e.to_string()))
    }

    fn fallback_path(&self, service: &ApiService) -> PathBuf {
        let safe_name = service
            .key_name()
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                    ch
                } else {
                    '_'
                }
            })
            .collect::<String>();
        self.fallback_dir.join(format!("{safe_name}.dpapi"))
    }

    fn store_fallback(&self, service: &ApiService, token: &str) -> AppResult<()> {
        let protected = protect_token(token.as_bytes())?;
        std::fs::create_dir_all(&self.fallback_dir)
            .map_err(|e| AppError::Keychain(e.to_string()))?;
        std::fs::write(self.fallback_path(service), BASE64.encode(protected))
            .map_err(|e| AppError::Keychain(e.to_string()))
    }

    fn retrieve_fallback(&self, service: &ApiService) -> AppResult<Option<String>> {
        let path = self.fallback_path(service);
        if !path.exists() {
            return Ok(None);
        }
        let encoded =
            std::fs::read_to_string(&path).map_err(|e| AppError::Keychain(e.to_string()))?;
        let protected = BASE64
            .decode(encoded.trim())
            .map_err(|e| AppError::Keychain(format!("Neplatny fallback API klic: {e}")))?;
        let clear = unprotect_token(&protected)?;
        String::from_utf8(clear)
            .map(Some)
            .map_err(|e| AppError::Keychain(format!("Fallback API klic neni UTF-8: {e}")))
    }

    fn delete_fallback(&self, service: &ApiService) -> AppResult<()> {
        let path = self.fallback_path(service);
        match std::fs::remove_file(&path) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(AppError::Keychain(e.to_string())),
        }
    }
}

impl Default for OsKeychain {
    fn default() -> Self {
        let fallback_dir = dirs::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("Weave")
            .join("secure");
        Self::new(fallback_dir)
    }
}

#[async_trait]
impl KeychainPort for OsKeychain {
    async fn store(&self, service: &ApiService, token: &str) -> AppResult<()> {
        let entry = match Self::entry(service) {
            Ok(entry) => entry,
            Err(e) => {
                tracing::warn!(
                    service = service.key_name(),
                    error = %e,
                    "OS keychain entry nejde vytvorit; pouzivam DPAPI fallback"
                );
                return self.store_fallback(service, token);
            }
        };
        let keyring_result = entry
            .set_password(token)
            .map_err(|e| AppError::Keychain(e.to_string()));

        match keyring_result {
            Ok(()) => match entry.get_password() {
                Ok(stored) if stored == token => {
                    self.delete_fallback(service)?;
                    Ok(())
                }
                Ok(_) | Err(keyring::Error::NoEntry) => {
                    tracing::warn!(
                        service = service.key_name(),
                        "OS keychain zapis probehl, ale overeni ctenim selhalo; pouzivam DPAPI fallback"
                    );
                    self.store_fallback(service, token)
                }
                Err(e) => {
                    tracing::warn!(
                        service = service.key_name(),
                        error = %e,
                        "OS keychain nejde overit po zapisu; pouzivam DPAPI fallback"
                    );
                    self.store_fallback(service, token)
                }
            },
            Err(e) => {
                tracing::warn!(
                    service = service.key_name(),
                    error = %e,
                    "OS keychain zapis selhal; pouzivam DPAPI fallback"
                );
                self.store_fallback(service, token)
            }
        }
    }

    async fn retrieve(&self, service: &ApiService) -> AppResult<Option<String>> {
        let entry = match Self::entry(service) {
            Ok(entry) => entry,
            Err(e) => {
                tracing::warn!(
                    service = service.key_name(),
                    error = %e,
                    "OS keychain entry nejde vytvorit; zkousim DPAPI fallback"
                );
                return self.retrieve_fallback(service);
            }
        };
        match entry.get_password() {
            Ok(pw) => Ok(Some(pw)),
            Err(keyring::Error::NoEntry) => self.retrieve_fallback(service),
            Err(e) => {
                tracing::warn!(
                    service = service.key_name(),
                    error = %e,
                    "OS keychain cteni selhalo; zkousim DPAPI fallback"
                );
                self.retrieve_fallback(service)
            }
        }
    }

    async fn delete(&self, service: &ApiService) -> AppResult<()> {
        if let Ok(entry) = Self::entry(service) {
            match entry.delete_credential() {
                Ok(_) | Err(keyring::Error::NoEntry) => {}
                Err(e) => tracing::warn!(
                    service = service.key_name(),
                    error = %e,
                    "OS keychain delete selhal; pokracuji mazanim fallbacku"
                ),
            };
        }
        self.delete_fallback(service)
    }
}

#[cfg(target_os = "windows")]
fn protect_token(data: &[u8]) -> AppResult<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let ok = unsafe {
        CryptProtectData(
            &input,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err(AppError::Keychain(format!(
            "DPAPI sifrovani selhalo: {}",
            std::io::Error::last_os_error()
        )));
    }
    let bytes =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData as *mut core::ffi::c_void);
    }
    Ok(bytes)
}

#[cfg(target_os = "windows")]
fn unprotect_token(data: &[u8]) -> AppResult<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err(AppError::Keychain(format!(
            "DPAPI desifrovani selhalo: {}",
            std::io::Error::last_os_error()
        )));
    }
    let bytes =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData as *mut core::ffi::c_void);
    }
    Ok(bytes)
}

#[cfg(not(target_os = "windows"))]
fn protect_token(_data: &[u8]) -> AppResult<Vec<u8>> {
    Err(AppError::Keychain(
        "Fallback API klicu je dostupny jen na Windows".into(),
    ))
}

#[cfg(not(target_os = "windows"))]
fn unprotect_token(_data: &[u8]) -> AppResult<Vec<u8>> {
    Err(AppError::Keychain(
        "Fallback API klicu je dostupny jen na Windows".into(),
    ))
}
