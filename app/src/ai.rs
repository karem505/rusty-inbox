//! Glue between the app and `rusty-inbox-ai`: credentials live in the engine's local secret store,
//! the active provider / model / thinking level and the feature switches come from settings, and
//! every feature calls [`run`]. WORKER B owns this file (keep these functions; add what Settings needs).
//!
//! Test hook: `RUSTY_INBOX_AI_URL=<base>` (and optionally `RUSTY_INBOX_AI_MODEL`) points everything at
//! an OpenAI-compatible server such as `tools/ai-mock/mock_server.py`, with every feature switched on.
//! The scripted UI check uses it (it runs offline otherwise).

// the feature workers start using these; remove once every function has a caller
#![allow(dead_code)]

use crate::engine::core;
use crate::settings;
use rusty_inbox_ai::features::Lang;
use rusty_inbox_ai::{ChatOutput, ChatRequest, Connection, ProviderKind, SecretStore};
use std::sync::atomic::AtomicBool;

/// [`SecretStore`] over the engine's secret table (file readable only by the user).
pub struct StoreSecrets;

impl SecretStore for StoreSecrets {
    fn get(&self, key: &str) -> Option<String> {
        core().store().secret(key).ok().flatten()
    }
    fn set(&self, key: &str, value: &str) {
        if let Err(e) = core().store().set_secret(key, value) {
            tracing::warn!("saving {key}: {e}");
        }
    }
    fn delete(&self, key: &str) {
        let _ = core().store().delete_secret(key);
    }
}

/// The scripted UI check points every feature at a mock server (`RUSTY_INBOX_AI_URL`).
pub fn test_hook() -> bool {
    test_url().is_some()
}

fn test_url() -> Option<String> {
    std::env::var("RUSTY_INBOX_AI_URL").ok().filter(|s| !s.trim().is_empty())
}

/// The connection for `kind` with the base URL the user set (Ollama, Custom).
pub fn connection(kind: ProviderKind) -> Connection {
    let base = match kind {
        ProviderKind::Ollama => settings::get(core().store(), "aiOllamaUrl"),
        ProviderKind::Custom => settings::get(core().store(), "aiCustomUrl"),
        _ => serde_json::Value::Null,
    };
    Connection::with_base_url(kind, base.as_str().unwrap_or_default())
}

/// What a feature call uses right now.
#[derive(Clone, Debug)]
pub struct Active {
    pub conn: Connection,
    pub model: String,
    pub thinking: Option<String>,
}

/// The configured provider + model, or a message saying what is missing (shown to the user).
pub fn active() -> Result<Active, String> {
    if let Some(url) = test_url() {
        let model = std::env::var("RUSTY_INBOX_AI_MODEL").unwrap_or_else(|_| "mock".into());
        return Ok(Active { conn: Connection::with_base_url(ProviderKind::Custom, &url), model, thinking: None });
    }
    let p = settings::all(core().store());
    if p["aiEnabled"] != serde_json::json!(true) {
        return Err("AI is off. Turn it on in Settings → AI.".into());
    }
    selected()
}

/// The provider + model chosen in Settings, whether or not the master switch is on and ignoring the
/// test hook (Settings → AI uses it for "Test connection" and the model list).
pub fn selected() -> Result<Active, String> {
    let p = settings::all(core().store());
    let kind =
        p["aiProvider"].as_str().and_then(ProviderKind::from_id).ok_or("Choose an AI provider in Settings → AI.")?;
    let model = p["aiModel"].as_str().unwrap_or_default().trim().to_string();
    if model.is_empty() {
        return Err("Choose a model in Settings → AI.".into());
    }
    let thinking = p["aiThinking"].as_str().filter(|s| !s.is_empty()).map(str::to_string);
    Ok(Active { conn: connection(kind), model, thinking })
}

/// AI is configured and on (the AI buttons show).
pub fn available() -> bool {
    active().is_ok()
}

/// The master switch and the feature's own switch (`aiComposeTools`, `aiSmartFilter`, ...) are on.
pub fn feature_on(key: &str) -> bool {
    if test_url().is_some() {
        return true;
    }
    let p = settings::all(core().store());
    p["aiEnabled"] == serde_json::json!(true) && p[key] == serde_json::json!(true)
}

/// Nothing from this account may be sent to the provider (setting `aiExcluded`, comma separated).
pub fn excluded(account: &str) -> bool {
    let p = settings::get(core().store(), "aiExcluded");
    p.as_str().unwrap_or_default().split(',').any(|a| a.trim().eq_ignore_ascii_case(account.trim()))
}

/// Answer language from the `aiLanguage` setting.
pub fn lang() -> Lang {
    Lang::from_setting(settings::get(core().store(), "aiLanguage").as_str().unwrap_or("auto"))
}

/// Run a feature request with the active model and thinking level. Blocking: call it from a helper
/// thread. The error is a sentence for the user.
pub fn run(mut req: ChatRequest, on_delta: &mut dyn FnMut(&str), cancel: &AtomicBool) -> Result<ChatOutput, String> {
    let a = active()?;
    req.model = a.model;
    if req.thinking.is_none() {
        req.thinking = a.thinking;
    }
    rusty_inbox_ai::chat(&a.conn, &StoreSecrets, &req, on_delta, cancel).map_err(|e| e.to_string())
}
