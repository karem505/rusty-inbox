//! AI for Rusty Inbox: provider connections, live model lists, streamed chat with a thinking level,
//! the ChatGPT (Codex) subscription sign-in, and the mail features (prompts + result parsing).
//!
//! Everything here is blocking (ureq) and meant to run on a helper thread; streaming goes through an
//! `on_delta` callback and every call can be cancelled with an `AtomicBool`. The crate knows nothing
//! about the mail store: callers pass mail in as [`features::MailForAi`] and persist credentials
//! through [`SecretStore`].
//!
//! Wire facts and the contract between modules: `docs/ai/DESIGN.md`.

pub mod chat;
pub mod codex;
pub mod extract;
pub mod features;
pub mod http;
pub mod models;
pub mod provider;
#[cfg(test)]
pub(crate) mod testutil;

pub use chat::{chat, ChatOutput, ChatRequest, Message, Role, Usage};
pub use models::{list_models, ModelInfo};
pub use provider::{AuthKind, Connection, ProviderKind};

/// Errors shown to the user as-is: keep messages short and actionable.
#[derive(Debug, thiserror::Error)]
pub enum AiError {
    /// no key / not signed in / no base URL
    #[error("{0}")]
    NotConfigured(String),
    /// 401/403: the key or sign-in is not accepted
    #[error("the provider rejected the credentials: {0}")]
    Auth(String),
    /// 429: quota or rate limit (the provider's own message)
    #[error("the provider is limiting requests: {0}")]
    RateLimited(String),
    /// any other HTTP error with the provider's message
    #[error("the provider answered {status}: {message}")]
    Http { status: u16, message: String },
    /// network failure, timeout
    #[error("could not reach the provider: {0}")]
    Network(String),
    /// the answer could not be understood (bad JSON, empty)
    #[error("unexpected answer: {0}")]
    Protocol(String),
    #[error("cancelled")]
    Cancelled,
}

pub type AiResult<T> = Result<T, AiError>;

/// Where credentials live. The app implements it over its local secret store (the same place as mail
/// passwords); tests use [`MemorySecrets`]. Keys used by this crate:
/// - `ai:key:<provider id>`: an API key
/// - `ai:codex`: the ChatGPT sign-in token set (JSON, see [`codex::CodexTokens`])
pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: &str);
    fn delete(&self, key: &str);
}

/// In-memory [`SecretStore`] for tests and tools.
#[derive(Default)]
pub struct MemorySecrets(std::sync::Mutex<std::collections::HashMap<String, String>>);

impl SecretStore for MemorySecrets {
    fn get(&self, key: &str) -> Option<String> {
        self.0.lock().ok()?.get(key).cloned()
    }
    fn set(&self, key: &str, value: &str) {
        if let Ok(mut m) = self.0.lock() {
            m.insert(key.to_string(), value.to_string());
        }
    }
    fn delete(&self, key: &str) {
        if let Ok(mut m) = self.0.lock() {
            m.remove(key);
        }
    }
}
