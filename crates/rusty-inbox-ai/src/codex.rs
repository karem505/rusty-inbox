//! ChatGPT subscription sign-in (the Codex OAuth client) and token upkeep. WORKER A implements.
//!
//! Two ways to sign in, both ending in a [`CodexTokens`] saved under `ai:codex`:
//! - browser: PKCE + state, a one-shot loopback server on 127.0.0.1:1455 (the redirect URI
//!   `http://localhost:1455/auth/callback` is fixed by the client registration);
//! - device code (when port 1455 is busy, or the user asks): show a URL and a code, poll.
//!
//! Refresh tokens rotate and are single-use: refresh under a process-wide lock and persist the new
//! pair before using it. Never import another app's token file (it would revoke that app's grant).

use crate::{AiError, AiResult, SecretStore};
use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const ISSUER: &str = "https://auth.openai.com";
pub const SECRET_KEY: &str = "ai:codex";

/// What is stored after a sign-in.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CodexTokens {
    pub access_token: String,
    pub refresh_token: String,
    #[serde(default)]
    pub id_token: String,
    /// `chatgpt_account_id` claim (sent as `ChatGPT-Account-ID`)
    #[serde(default)]
    pub account_id: String,
    /// data residency claim (sent as `x-openai-internal-codex-residency` when set)
    #[serde(default)]
    pub residency: String,
    #[serde(default)]
    pub email: String,
    /// plus, pro, team ...
    #[serde(default)]
    pub plan: String,
    /// unix seconds
    #[serde(default)]
    pub expires_at: i64,
}

/// Shown in Settings when signed in.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Account {
    pub email: String,
    pub plan: String,
}

/// The signed-in account, if any (no network).
pub fn account(secrets: &dyn SecretStore) -> Option<Account> {
    let t: CodexTokens = serde_json::from_str(&secrets.get(SECRET_KEY)?).ok()?;
    Some(Account { email: t.email, plan: t.plan })
}

/// Forget the sign-in.
pub fn sign_out(secrets: &dyn SecretStore) {
    secrets.delete(SECRET_KEY);
}

/// A started browser sign-in: open `url` in the browser, then call [`BrowserLogin::wait`].
pub struct BrowserLogin {
    pub url: String,
    // WORKER A: listener, verifier, state
}

/// Bind 127.0.0.1:1455 and build the authorize URL. `Err` (with a message saying so) when the port is
/// busy, so the caller falls back to [`start_device_login`].
pub fn start_browser_login() -> AiResult<BrowserLogin> {
    Err(AiError::NotConfigured("ChatGPT sign-in is not implemented yet".into()))
}

impl BrowserLogin {
    /// Wait for the redirect (answering the browser with a small "you can close this tab" page),
    /// exchange the code, save the tokens and return the account.
    pub fn wait(self, _secrets: &dyn SecretStore, _timeout: Duration, _cancel: &AtomicBool) -> AiResult<Account> {
        Err(AiError::NotConfigured("ChatGPT sign-in is not implemented yet".into()))
    }
}

/// A started device-code sign-in: show `verification_url` and `user_code`, then call
/// [`DeviceLogin::wait`].
#[derive(Clone, Debug, Default)]
pub struct DeviceLogin {
    pub verification_url: String,
    pub user_code: String,
    pub device_auth_id: String,
    pub interval_secs: u64,
}

pub fn start_device_login() -> AiResult<DeviceLogin> {
    Err(AiError::NotConfigured("ChatGPT sign-in is not implemented yet".into()))
}

impl DeviceLogin {
    /// Poll until approved (15 min max), exchange, save, return the account.
    pub fn wait(&self, _secrets: &dyn SecretStore, _cancel: &AtomicBool) -> AiResult<Account> {
        Err(AiError::NotConfigured("ChatGPT sign-in is not implemented yet".into()))
    }
}

/// A valid access token plus the headers the Codex backend needs (`Authorization`,
/// `ChatGPT-Account-ID`, residency, `originator: rusty-inbox`, `User-Agent`), refreshing first when it
/// expires within 2 minutes. `force_refresh` after a 401.
pub fn auth_headers(_secrets: &dyn SecretStore, _force_refresh: bool) -> AiResult<Vec<(&'static str, String)>> {
    Err(AiError::NotConfigured("Sign in with ChatGPT in Settings → AI.".into()))
}
