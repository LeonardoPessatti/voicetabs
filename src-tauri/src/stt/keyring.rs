//! Thin wrapper around the OS credential store for the OpenAI API key.
//!
//! Windows backs onto Credential Manager (which uses DPAPI under the hood);
//! macOS and Linux are TODO but not in v1 scope.

const SERVICE: &str = "voicetabs";
const KEY_NAME: &str = "openai_api_key";

#[derive(Debug, thiserror::Error)]
pub enum KeyringError {
    #[error("keyring: {0}")]
    Backend(String),
    #[error("no key configured")]
    NotFound,
}

pub fn get_api_key() -> Result<String, KeyringError> {
    let entry = keyring::Entry::new(SERVICE, KEY_NAME)
        .map_err(|e| KeyringError::Backend(e.to_string()))?;
    match entry.get_password() {
        Ok(s) => Ok(s),
        Err(keyring::Error::NoEntry) => Err(KeyringError::NotFound),
        Err(e) => Err(KeyringError::Backend(e.to_string())),
    }
}

pub fn set_api_key(value: &str) -> Result<(), KeyringError> {
    let entry = keyring::Entry::new(SERVICE, KEY_NAME)
        .map_err(|e| KeyringError::Backend(e.to_string()))?;
    entry
        .set_password(value)
        .map_err(|e| KeyringError::Backend(e.to_string()))
}

pub fn clear_api_key() -> Result<(), KeyringError> {
    let entry = keyring::Entry::new(SERVICE, KEY_NAME)
        .map_err(|e| KeyringError::Backend(e.to_string()))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()), // idempotent
        Err(e) => Err(KeyringError::Backend(e.to_string())),
    }
}

/// Mask `key` in `haystack`, keeping the first 4 chars to aid in support
/// debugging (e.g. "sk-a***"). Empty key → no-op.
pub fn redact_secret(haystack: &str, key: &str) -> String {
    if key.is_empty() {
        return haystack.to_string();
    }
    let prefix: String = key.chars().take(4).collect();
    let mask = format!("{prefix}{}", "*".repeat(10));
    haystack.replace(key, &mask)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_replaces_full_key() {
        let key = "sk-abcdefghijklmnop";
        let s = format!("Bearer {key}");
        let red = redact_secret(&s, key);
        assert!(!red.contains("abcdef"));
        assert!(red.contains("sk-"));
        assert!(red.contains("****"));
    }

    #[test]
    fn redact_handles_short_keys() {
        let key = "abc";
        let s = "leaked: abc here";
        let red = redact_secret(s, key);
        assert!(!red.contains("abc here"));
    }

    #[test]
    fn redact_noop_on_empty_key() {
        assert_eq!(redact_secret("hello", ""), "hello");
    }
}
