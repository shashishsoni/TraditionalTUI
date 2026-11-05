//! Credential resolution for BYOK providers.
//!
//! Secrets are never read from `config.toml`. Resolution order is:
//!
//! 1. An in-session key installed with `CredentialStore::set` (from `:ai set-key`).
//! 2. The provider environment variable (`OPENAI_API_KEY`, ...).
//!
//! Keys are wrapped in [`secrecy::SecretString`], which redacts `Debug` output
//! and zeroizes memory on drop.

use std::collections::HashMap;
use std::sync::Mutex;

use secrecy::SecretString;

use super::types::{ProviderConfig, ProviderKind};

/// Thread-safe credential resolver.
#[derive(Debug, Default)]
pub struct CredentialStore {
    session: Mutex<HashMap<String, SecretString>>,
}

impl CredentialStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Store a key for this process only.
    pub fn set(&self, provider_id: &str, key: impl Into<String>) {
        if let Ok(mut session) = self.session.lock() {
            session.insert(provider_id.to_string(), SecretString::from(key.into()));
        }
    }

    pub fn remove(&self, provider_id: &str) {
        if let Ok(mut session) = self.session.lock() {
            session.remove(provider_id);
        }
    }

    pub fn has_key(&self, provider_id: &str) -> bool {
        self.session
            .lock()
            .map(|session| session.contains_key(provider_id))
            .unwrap_or(false)
            || env_key(provider_id, None).is_some()
    }

    /// Resolve a key for a provider, honoring a custom environment variable.
    pub fn resolve(&self, provider_id: &str, env_var: Option<&str>) -> Option<SecretString> {
        if let Ok(session) = self.session.lock() {
            if let Some(key) = session.get(provider_id) {
                return Some(key.clone());
            }
        }
        env_key(provider_id, env_var)
    }

    /// Fill `api_key` on a provider config when one is available.
    pub fn apply(&self, mut config: ProviderConfig) -> ProviderConfig {
        if config.api_key.is_none() {
            let env_var = config.kind.default_env_var().map(str::to_string);
            let key = self.resolve(&config.id, env_var.as_deref());
            if key.is_some() {
                config.api_key = key;
            }
        }
        config
    }

    /// Environment variable that will be consulted for a provider.
    pub fn env_var_for(kind: ProviderKind) -> Option<&'static str> {
        kind.default_env_var()
    }
}

fn env_key(provider_id: &str, custom_env: Option<&str>) -> Option<SecretString> {
    let variable = custom_env
        .filter(|name| !name.trim().is_empty())
        .map(str::to_string)
        .or_else(|| {
            ProviderKind::from_id(provider_id)
                .and_then(|kind| kind.default_env_var().map(str::to_string))
        })?;
    let value = std::env::var(variable).ok()?;
    if value.trim().is_empty() {
        None
    } else {
        Some(SecretString::from(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_keys_take_priority() {
        let store = CredentialStore::new();
        store.set("openai", "sk-session");
        let key = store
            .resolve("openai", Some("TTUI_UNSET_TEST_VAR"))
            .expect("key");
        assert_eq!(secrecy::ExposeSecret::expose_secret(&key), "sk-session");
    }

    #[test]
    fn missing_key_returns_none_for_unknown_env() {
        let store = CredentialStore::new();
        assert!(
            store
                .resolve("unknown-provider", Some("TTUI_SURELY_UNSET_VAR"))
                .is_none()
        );
    }

    #[test]
    fn env_var_mapping_is_stable() {
        assert_eq!(
            CredentialStore::env_var_for(ProviderKind::Anthropic),
            Some("ANTHROPIC_API_KEY")
        );
        assert_eq!(CredentialStore::env_var_for(ProviderKind::Ollama), None);
    }
}
