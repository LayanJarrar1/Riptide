//! API-key storage. Keys never go into the config file or the database: on Windows they live in
//! the Credential Manager (per user, DPAPI-protected); elsewhere, and in tests, in memory only.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::error::{AiError, Result};
use crate::key::ApiKey;
use crate::types::ProviderKind;

/// Per-provider key storage.
pub trait SecretStore: Send + Sync + std::fmt::Debug {
    /// The stored key, if any.
    fn get(&self, provider: ProviderKind) -> Result<Option<ApiKey>>;
    /// Stores or replaces the key.
    fn set(&self, provider: ProviderKind, key: &ApiKey) -> Result<()>;
    /// Removes the key; removing a missing key is not an error.
    fn delete(&self, provider: ProviderKind) -> Result<()>;
    /// A secret that is not a provider key (a Brave Search token, a Jamendo client id), stored
    /// under its own name next to the provider keys. `name` is a short ascii id such as
    /// `brave`.
    fn get_named(&self, name: &str) -> Result<Option<ApiKey>>;
    /// Stores or replaces a named secret.
    fn set_named(&self, name: &str, key: &ApiKey) -> Result<()>;
    /// Removes a named secret; removing a missing one is not an error.
    fn delete_named(&self, name: &str) -> Result<()>;
}

/// Names are used in credential targets: only lowercase ascii letters, digits and dashes.
fn check_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 32
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(AiError::Store("invalid secret name".into()));
    }
    Ok(())
}

/// Volatile store: tests, and the fallback on platforms without a native backend yet
/// (macOS Keychain / Secret Service come with the cross-platform milestone; until then keys
/// on those platforms are not persisted and the user re-enters them each run).
#[derive(Default)]
pub struct MemorySecretStore(
    Mutex<HashMap<ProviderKind, ApiKey>>,
    Mutex<HashMap<String, ApiKey>>,
);

impl MemorySecretStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for MemorySecretStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MemorySecretStore { keys: [REDACTED] }")
    }
}

impl SecretStore for MemorySecretStore {
    fn get(&self, provider: ProviderKind) -> Result<Option<ApiKey>> {
        let m = self
            .0
            .lock()
            .map_err(|_| AiError::Store("lock poisoned".into()))?;
        Ok(m.get(&provider).cloned())
    }
    fn set(&self, provider: ProviderKind, key: &ApiKey) -> Result<()> {
        let mut m = self
            .0
            .lock()
            .map_err(|_| AiError::Store("lock poisoned".into()))?;
        m.insert(provider, key.clone());
        Ok(())
    }
    fn delete(&self, provider: ProviderKind) -> Result<()> {
        let mut m = self
            .0
            .lock()
            .map_err(|_| AiError::Store("lock poisoned".into()))?;
        m.remove(&provider);
        Ok(())
    }
    fn get_named(&self, name: &str) -> Result<Option<ApiKey>> {
        check_name(name)?;
        let m = self
            .1
            .lock()
            .map_err(|_| AiError::Store("lock poisoned".into()))?;
        Ok(m.get(name).cloned())
    }
    fn set_named(&self, name: &str, key: &ApiKey) -> Result<()> {
        check_name(name)?;
        let mut m = self
            .1
            .lock()
            .map_err(|_| AiError::Store("lock poisoned".into()))?;
        m.insert(name.to_string(), key.clone());
        Ok(())
    }
    fn delete_named(&self, name: &str) -> Result<()> {
        check_name(name)?;
        let mut m = self
            .1
            .lock()
            .map_err(|_| AiError::Store("lock poisoned".into()))?;
        m.remove(name);
        Ok(())
    }
}

/// Default credential target prefix: entries are named `Riptide/ai/<provider>`.
pub const DEFAULT_TARGET_PREFIX: &str = "Riptide/ai";

/// The best store for this platform: Credential Manager on Windows, memory elsewhere.
pub fn platform_store() -> Box<dyn SecretStore> {
    #[cfg(windows)]
    {
        Box::new(windows::CredentialManagerStore::new(DEFAULT_TARGET_PREFIX))
    }
    #[cfg(not(windows))]
    {
        tracing::warn!("no native secret store on this platform yet; API keys are kept in memory");
        Box::new(MemorySecretStore::new())
    }
}

#[cfg(windows)]
pub use windows::CredentialManagerStore;

#[cfg(windows)]
mod windows {
    #![allow(unsafe_code)]

    use std::ptr;

    use windows_sys::Win32::Foundation::{GetLastError, ERROR_NOT_FOUND, FALSE};
    use windows_sys::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
        CRED_TYPE_GENERIC,
    };

    use super::SecretStore;
    use crate::error::{AiError, Result};
    use crate::key::ApiKey;
    use crate::types::ProviderKind;

    /// Windows Credential Manager backend (generic credentials, current user, local machine
    /// persistence). Targets are `<prefix>/<provider-id>`.
    #[derive(Debug, Clone)]
    pub struct CredentialManagerStore {
        prefix: String,
    }

    impl CredentialManagerStore {
        /// `prefix` is normally [`super::DEFAULT_TARGET_PREFIX`]; tests pass a throwaway one.
        pub fn new(prefix: impl Into<String>) -> Self {
            Self {
                prefix: prefix.into(),
            }
        }

        fn target_of(&self, leaf: &str) -> Vec<u16> {
            let s = format!("{}/{}", self.prefix, leaf);
            s.encode_utf16().chain(std::iter::once(0)).collect()
        }

        fn target(&self, p: ProviderKind) -> Vec<u16> {
            self.target_of(p.id())
        }

        /// Named secrets live at `<prefix>/ext-<name>`, which no provider id can equal.
        fn named_target(&self, name: &str) -> Result<Vec<u16>> {
            super::check_name(name)?;
            Ok(self.target_of(&format!("ext-{name}")))
        }
    }

    fn last_error() -> u32 {
        // SAFETY: plain FFI call with no arguments.
        unsafe { GetLastError() }
    }

    fn read(target: &[u16]) -> Result<Option<ApiKey>> {
        {
            let mut cred: *mut CREDENTIALW = ptr::null_mut();
            // SAFETY: `target` is NUL-terminated; on success `cred` points to a block we free
            // with CredFree after copying the blob out.
            let ok = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut cred) };
            if ok == FALSE {
                let e = last_error();
                return if e == ERROR_NOT_FOUND {
                    Ok(None)
                } else {
                    Err(AiError::Store(format!(
                        "Credential Manager read failed (code {e})"
                    )))
                };
            }
            // SAFETY: CredReadW succeeded so `cred` is valid; the blob is `CredentialBlobSize`
            // bytes at `CredentialBlob`.
            let bytes = unsafe {
                let c = &*cred;
                let v = if c.CredentialBlob.is_null() || c.CredentialBlobSize == 0 {
                    Vec::new()
                } else {
                    std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize)
                        .to_vec()
                };
                CredFree(cred as *const _);
                v
            };
            let text = String::from_utf8(bytes)
                .map_err(|_| AiError::Store("stored credential is not valid text".into()))?;
            ApiKey::new(text).map(Some)
        }
    }

    fn write(mut target: Vec<u16>, key: &ApiKey) -> Result<()> {
        {
            let mut user: Vec<u16> = "riptide".encode_utf16().chain(std::iter::once(0)).collect();
            let blob = key.expose().as_bytes();
            let blob_len =
                u32::try_from(blob.len()).map_err(|_| AiError::Store("key too large".into()))?;
            // SAFETY: zeroed CREDENTIALW is a valid starting value; we fill the fields we need.
            let mut cred: CREDENTIALW = unsafe { std::mem::zeroed() };
            cred.Type = CRED_TYPE_GENERIC;
            cred.TargetName = target.as_mut_ptr();
            cred.UserName = user.as_mut_ptr();
            cred.CredentialBlobSize = blob_len;
            cred.CredentialBlob = blob.as_ptr() as *mut u8;
            cred.Persist = CRED_PERSIST_LOCAL_MACHINE;
            // SAFETY: all pointers outlive the call; CredWriteW copies the data.
            let ok = unsafe { CredWriteW(&cred, 0) };
            if ok == FALSE {
                return Err(AiError::Store(format!(
                    "Credential Manager write failed (code {})",
                    last_error()
                )));
            }
            Ok(())
        }
    }

    fn remove(target: &[u16]) -> Result<()> {
        {
            // SAFETY: `target` is NUL-terminated.
            let ok = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
            if ok == FALSE {
                let e = last_error();
                if e != ERROR_NOT_FOUND {
                    return Err(AiError::Store(format!(
                        "Credential Manager delete failed (code {e})"
                    )));
                }
            }
            Ok(())
        }
    }

    impl SecretStore for CredentialManagerStore {
        fn get(&self, provider: ProviderKind) -> Result<Option<ApiKey>> {
            read(&self.target(provider))
        }
        fn set(&self, provider: ProviderKind, key: &ApiKey) -> Result<()> {
            write(self.target(provider), key)
        }
        fn delete(&self, provider: ProviderKind) -> Result<()> {
            remove(&self.target(provider))
        }
        fn get_named(&self, name: &str) -> Result<Option<ApiKey>> {
            read(&self.named_target(name)?)
        }
        fn set_named(&self, name: &str, key: &ApiKey) -> Result<()> {
            write(self.named_target(name)?, key)
        }
        fn delete_named(&self, name: &str) -> Result<()> {
            remove(&self.named_target(name)?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_round_trip() {
        let s = MemorySecretStore::new();
        let k = ApiKey::new("sk-test-123456789").unwrap();
        assert!(s.get(ProviderKind::Groq).unwrap().is_none());
        s.set(ProviderKind::Groq, &k).unwrap();
        assert_eq!(
            s.get(ProviderKind::Groq).unwrap().unwrap().expose(),
            k.expose()
        );
        assert!(s.get(ProviderKind::OpenAi).unwrap().is_none());
        s.delete(ProviderKind::Groq).unwrap();
        s.delete(ProviderKind::Groq).unwrap();
        assert!(s.get(ProviderKind::Groq).unwrap().is_none());
        assert!(!format!("{s:?}").contains("sk-test"));
    }

    #[test]
    fn named_secrets_are_separate_from_provider_keys() {
        let s = MemorySecretStore::new();
        let k = ApiKey::new("brave-token-123456").unwrap();
        s.set_named("brave", &k).unwrap();
        assert_eq!(s.get_named("brave").unwrap().unwrap().expose(), k.expose());
        assert!(s.get(ProviderKind::OpenAi).unwrap().is_none());
        assert!(s.get_named("jamendo").unwrap().is_none());
        assert!(s.set_named("../evil", &k).is_err());
        assert!(s.get_named("").is_err());
        s.delete_named("brave").unwrap();
        s.delete_named("brave").unwrap();
        assert!(s.get_named("brave").unwrap().is_none());
    }
}
