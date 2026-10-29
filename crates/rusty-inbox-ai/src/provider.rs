//! Provider catalog and connections.

use serde::{Deserialize, Serialize};

/// The providers the app can talk to. The order of [`ProviderKind::ALL`] is the order in the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    /// ChatGPT subscription through the Codex backend (sign-in, no API key)
    Codex,
    OpenAI,
    Anthropic,
    OpenRouter,
    /// local Ollama (no key) or ollama.com (key)
    Ollama,
    /// any OpenAI-compatible endpoint (LM Studio, vLLM, a gateway): base URL + optional key
    Custom,
}

/// How a provider authenticates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthKind {
    ApiKey,
    /// a key only for the hosted service (Ollama: none for localhost, a key for ollama.com)
    OptionalKey,
    /// ChatGPT account sign-in (Codex OAuth)
    ChatGpt,
}

impl ProviderKind {
    pub const ALL: [ProviderKind; 6] = [
        ProviderKind::Codex,
        ProviderKind::OpenAI,
        ProviderKind::Anthropic,
        ProviderKind::OpenRouter,
        ProviderKind::Ollama,
        ProviderKind::Custom,
    ];

    /// Stable id used in settings and secret keys.
    pub fn id(self) -> &'static str {
        match self {
            ProviderKind::Codex => "codex",
            ProviderKind::OpenAI => "openai",
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::OpenRouter => "openrouter",
            ProviderKind::Ollama => "ollama",
            ProviderKind::Custom => "custom",
        }
    }

    pub fn from_id(id: &str) -> Option<ProviderKind> {
        ProviderKind::ALL.into_iter().find(|k| k.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            ProviderKind::Codex => "ChatGPT subscription",
            ProviderKind::OpenAI => "OpenAI API key",
            ProviderKind::Anthropic => "Anthropic",
            ProviderKind::OpenRouter => "OpenRouter",
            ProviderKind::Ollama => "Ollama",
            ProviderKind::Custom => "Custom (OpenAI-compatible)",
        }
    }

    /// One line under the label in Settings.
    pub fn blurb(self) -> &'static str {
        match self {
            ProviderKind::Codex => "Sign in with your ChatGPT account (Plus, Pro, Business). No API key.",
            ProviderKind::OpenAI => "Pay-as-you-go API key from platform.openai.com.",
            ProviderKind::Anthropic => "Claude models with an API key from console.anthropic.com.",
            ProviderKind::OpenRouter => "Hundreds of models from many vendors with one key.",
            ProviderKind::Ollama => "Models on this computer (no key), or on ollama.com with a key.",
            ProviderKind::Custom => "LM Studio, vLLM, LiteLLM or any server that speaks the OpenAI API.",
        }
    }

    pub fn auth(self) -> AuthKind {
        match self {
            ProviderKind::Codex => AuthKind::ChatGpt,
            ProviderKind::Ollama | ProviderKind::Custom => AuthKind::OptionalKey,
            _ => AuthKind::ApiKey,
        }
    }

    /// Base URL used when the connection has no override. Custom has none (the user must give one).
    pub fn default_base_url(self) -> &'static str {
        match self {
            ProviderKind::Codex => "https://chatgpt.com/backend-api/codex",
            ProviderKind::OpenAI => "https://api.openai.com/v1",
            ProviderKind::Anthropic => "https://api.anthropic.com/v1",
            ProviderKind::OpenRouter => "https://openrouter.ai/api/v1",
            ProviderKind::Ollama => "http://localhost:11434",
            ProviderKind::Custom => "",
        }
    }

    /// The user may change the base URL (Ollama: local or ollama.com; Custom: required).
    pub fn base_url_editable(self) -> bool {
        matches!(self, ProviderKind::Ollama | ProviderKind::Custom)
    }

    /// Secret-store key of this provider's API key.
    pub fn key_secret(self) -> String {
        format!("ai:key:{}", self.id())
    }
}

/// One provider as configured by the user. The API key is not part of it: it is read from the
/// [`crate::SecretStore`] at call time (so a rotated key or refreshed sign-in is always current).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Connection {
    pub kind: ProviderKind,
    /// override of [`ProviderKind::default_base_url`] (empty = default)
    #[serde(default)]
    pub base_url: String,
}

impl Connection {
    pub fn new(kind: ProviderKind) -> Connection {
        Connection { kind, base_url: String::new() }
    }

    pub fn with_base_url(kind: ProviderKind, base_url: &str) -> Connection {
        Connection { kind, base_url: base_url.trim().trim_end_matches('/').to_string() }
    }

    /// The base URL requests go to, without a trailing slash. Precedence: override > provider default.
    /// (Never another provider's default: a key must only go to its own provider's host.)
    pub fn base(&self) -> String {
        let b = if self.base_url.trim().is_empty() { self.kind.default_base_url() } else { self.base_url.trim() };
        b.trim_end_matches('/').to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_and_bases_stay_per_provider() {
        for k in ProviderKind::ALL {
            assert_eq!(ProviderKind::from_id(k.id()), Some(k));
        }
        assert_eq!(Connection::new(ProviderKind::OpenRouter).base(), "https://openrouter.ai/api/v1");
        assert_eq!(Connection::with_base_url(ProviderKind::Ollama, "https://ollama.com/").base(), "https://ollama.com");
        assert_eq!(Connection::new(ProviderKind::Custom).base(), "");
    }
}
