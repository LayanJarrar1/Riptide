//! The API key newtype. `Debug` is always `[REDACTED]`; the text is zeroized on drop.

use std::fmt;
use std::sync::Arc;

use secrecy::{ExposeSecret, SecretString};

use crate::error::{AiError, Result};

/// An API key. Cheap to clone (shared), never printed, zeroized when the last clone drops.
#[derive(Clone)]
pub struct ApiKey(Arc<SecretString>);

impl ApiKey {
    /// Wraps a key. Surrounding whitespace is trimmed; empty keys, whitespace or control
    /// characters inside, and keys over 512 bytes are rejected (they would also be unsafe in a
    /// header).
    pub fn new(raw: impl Into<String>) -> Result<Self> {
        let mut raw: String = raw.into();
        let trimmed = raw.trim().to_string();
        zeroize::Zeroize::zeroize(&mut raw);
        if trimmed.is_empty() {
            return Err(AiError::Config("the API key is empty".into()));
        }
        if trimmed.len() > 512 || trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(AiError::Config("the API key has an invalid format".into()));
        }
        Ok(Self(Arc::new(SecretString::from(trimmed))))
    }

    /// The raw key. Call only where the key is put on the wire or in the secret store.
    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }

    /// UI-safe form: bullets plus at most the last four characters, e.g. `••••abcd`.
    /// Keys of 8 characters or fewer show only bullets.
    pub fn masked(&self) -> String {
        let k = self.expose();
        let n = k.chars().count();
        if n <= 8 {
            return "••••".to_string();
        }
        let tail: String = k.chars().skip(n - 4).collect();
        format!("••••{tail}")
    }

    /// Removes every occurrence of the key from `text` (defence in depth for provider messages).
    pub fn scrub(&self, text: &str) -> String {
        text.replace(self.expose(), "[REDACTED]")
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl fmt::Display for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masked_shows_last_four_only() {
        let k = ApiKey::new("sk-abcdefghijklmnop1234").unwrap();
        assert_eq!(k.masked(), "••••1234");
        assert_eq!(format!("{k:?}"), "[REDACTED]");
        assert_eq!(format!("{k}"), "[REDACTED]");
    }

    #[test]
    fn short_keys_are_fully_masked() {
        assert_eq!(ApiKey::new("abcd1234").unwrap().masked(), "••••");
    }

    #[test]
    fn rejects_bad_keys() {
        assert!(ApiKey::new("   ").is_err());
        assert!(ApiKey::new("a b").is_err());
        assert!(ApiKey::new("a\r\nX-Evil: 1").is_err());
        assert!(ApiKey::new("x".repeat(513)).is_err());
    }

    #[test]
    fn scrub_removes_the_key() {
        let k = ApiKey::new("sk-secret-value-123").unwrap();
        assert_eq!(
            k.scrub("bad key sk-secret-value-123!"),
            "bad key [REDACTED]!"
        );
    }
}
