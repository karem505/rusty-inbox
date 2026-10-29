//! ChatGPT subscription sign-in (the Codex OAuth client) and token upkeep.
//!
//! Two ways to sign in, both ending in a [`CodexTokens`] saved under `ai:codex`:
//! - browser: PKCE + state, a one-shot loopback server on 127.0.0.1:1455 (the redirect URI
//!   `http://localhost:1455/auth/callback` is fixed by the client registration);
//! - device code (when port 1455 is busy, or the user asks): show a URL and a code, poll.
//!
//! Refresh tokens rotate and are single-use: refresh under a process-wide lock and persist the new
//! pair before using it. Never import another app's token file (it would revoke that app's grant).
//!
//! # Port 1455 busy
//! [`start_browser_login`] fails with `AiError::NotConfigured` whose text starts with
//! [`PORT_BUSY_MESSAGE`]; test it with [`is_port_busy`] and fall back to [`start_device_login`].

use crate::http::{self, urlencode};
use crate::{AiError, AiResult, SecretStore};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const ISSUER: &str = "https://auth.openai.com";
pub const SECRET_KEY: &str = "ai:codex";
/// The loopback port the client registration allows.
pub const CALLBACK_PORT: u16 = 1455;
/// Prefix of the error text when [`CALLBACK_PORT`] is taken (see [`is_port_busy`]).
pub const PORT_BUSY_MESSAGE: &str = "Port 1455 is already in use";

/// Refresh when the access token expires within this many seconds.
const REFRESH_MARGIN_SECS: i64 = 120;
/// Device sign-in gives up after this long.
const DEVICE_LIMIT: Duration = Duration::from_secs(15 * 60);

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
    let t = load_tokens(secrets)?;
    Some(Account { email: t.email, plan: t.plan })
}

/// The stored token set, if any (no network).
pub fn load_tokens(secrets: &dyn SecretStore) -> Option<CodexTokens> {
    serde_json::from_str(&secrets.get(SECRET_KEY)?).ok()
}

fn save_tokens(secrets: &dyn SecretStore, t: &CodexTokens) {
    if let Ok(s) = serde_json::to_string(t) {
        secrets.set(SECRET_KEY, &s);
    }
}

/// Forget the sign-in.
pub fn sign_out(secrets: &dyn SecretStore) {
    secrets.delete(SECRET_KEY);
}

/// True when `e` is the "port 1455 is busy" error of [`start_browser_login`].
pub fn is_port_busy(e: &AiError) -> bool {
    matches!(e, AiError::NotConfigured(m) if m.starts_with(PORT_BUSY_MESSAGE))
}

// ---------------------------------------------------------------- endpoints

/// Where the OAuth server lives and how patiently we poll; tests point it at a local mock.
#[derive(Clone, Debug)]
struct Endpoints {
    issuer: String,
    /// lower bound of the device polling interval
    poll_floor: Duration,
    /// first wait after a 429 (doubles up to 8x)
    backoff: Duration,
    device_limit: Duration,
}

impl Endpoints {
    fn production() -> Endpoints {
        Endpoints {
            issuer: ISSUER.to_string(),
            poll_floor: Duration::from_secs(3),
            backoff: Duration::from_secs(5),
            device_limit: DEVICE_LIMIT,
        }
    }
    fn token_url(&self) -> String {
        format!("{}/oauth/token", self.issuer)
    }
    fn authorize_url(&self) -> String {
        format!("{}/oauth/authorize", self.issuer)
    }
}

#[cfg(test)]
thread_local! {
    static OVERRIDE: std::cell::RefCell<Option<Endpoints>> = const { std::cell::RefCell::new(None) };
}

#[cfg(not(test))]
fn endpoints() -> Endpoints {
    Endpoints::production()
}

/// In tests the endpoints can be redirected per thread (see `tests::with_issuer`).
#[cfg(test)]
fn endpoints() -> Endpoints {
    OVERRIDE.with(|o| o.borrow().clone()).unwrap_or_else(Endpoints::production)
}

// ---------------------------------------------------------------- PKCE, JWT, helpers

fn random_b64(bytes: usize) -> AiResult<String> {
    let mut b = vec![0u8; bytes];
    getrandom::fill(&mut b).map_err(|e| AiError::NotConfigured(format!("no secure random source: {e}")))?;
    Ok(URL_SAFE_NO_PAD.encode(b))
}

/// PKCE S256: base64url(sha256(verifier)), no padding (RFC 7636).
pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// What the app needs from the access-token (or id-token) JWT payload.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Claims {
    pub exp: i64,
    pub account_id: String,
    pub residency: String,
    pub plan: String,
    pub email: String,
}

/// Decode the payload of a JWT (no signature check: the token came from the token endpoint over TLS
/// and the backend validates it).
pub fn jwt_payload(jwt: &str) -> Option<Value> {
    let part = jwt.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(part.trim_end_matches('=')).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Claims of a JWT: `exp`; `https://api.openai.com/auth` → `chatgpt_account_id`, `chatgpt_plan_type`,
/// `chatgpt_data_residency` (else `chatgpt_compute_residency`); `https://api.openai.com/profile` → `email`.
pub fn parse_claims(jwt: &str) -> Option<Claims> {
    let v = jwt_payload(jwt)?;
    let auth = &v["https://api.openai.com/auth"];
    let s = |x: &Value| x.as_str().unwrap_or_default().to_string();
    let residency = [&auth["chatgpt_data_residency"], &auth["chatgpt_compute_residency"]]
        .into_iter()
        .map(s)
        .find(|r| !r.is_empty())
        .unwrap_or_default();
    let email = [&v["https://api.openai.com/profile"]["email"], &v["email"]]
        .into_iter()
        .map(s)
        .find(|r| !r.is_empty())
        .unwrap_or_default();
    Some(Claims {
        exp: v["exp"].as_i64().unwrap_or(0),
        account_id: s(&auth["chatgpt_account_id"]),
        residency,
        plan: s(&auth["chatgpt_plan_type"]),
        email,
    })
}

/// Turn a token-endpoint answer into stored tokens. `previous` supplies values the answer may omit
/// (a refresh often returns no new `id_token`).
fn tokens_from_response(v: &Value, previous: Option<&CodexTokens>) -> AiResult<CodexTokens> {
    let text = |k: &str| v[k].as_str().unwrap_or_default().to_string();
    let access = text("access_token");
    if access.is_empty() {
        return Err(AiError::Protocol("the sign-in server returned no access token".into()));
    }
    let mut refresh = text("refresh_token");
    let mut id_token = text("id_token");
    if let Some(p) = previous {
        if refresh.is_empty() {
            refresh = p.refresh_token.clone();
        }
        if id_token.is_empty() {
            id_token = p.id_token.clone();
        }
    }
    let ac = parse_claims(&access).unwrap_or_default();
    let ic = parse_claims(&id_token).unwrap_or_default();
    let pick = |a: &str, b: &str, c: Option<&str>| -> String {
        [a, b, c.unwrap_or_default()].into_iter().find(|s| !s.is_empty()).unwrap_or_default().to_string()
    };
    let account_id = pick(&ac.account_id, &ic.account_id, previous.map(|p| p.account_id.as_str()));
    if account_id.is_empty() {
        return Err(AiError::Protocol("the ChatGPT account id is missing from the sign-in token".into()));
    }
    let expires_at = if ac.exp > 0 { ac.exp } else { now_secs() + v["expires_in"].as_i64().unwrap_or(3600) };
    Ok(CodexTokens {
        access_token: access,
        refresh_token: refresh,
        id_token,
        account_id,
        residency: pick(&ac.residency, &ic.residency, previous.map(|p| p.residency.as_str())),
        email: pick(&ac.email, &ic.email, previous.map(|p| p.email.as_str())),
        plan: pick(&ac.plan, &ic.plan, previous.map(|p| p.plan.as_str())),
        expires_at,
    })
}

/// Exchange an authorization code for tokens and save them.
fn exchange_code(
    ep: &Endpoints,
    secrets: &dyn SecretStore,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> AiResult<Account> {
    let resp = http::post_form(
        &ep.token_url(),
        &[],
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", CLIENT_ID),
            ("code_verifier", verifier),
        ],
    )?;
    let t = tokens_from_response(&http::read_json(resp)?, None)?;
    save_tokens(secrets, &t);
    Ok(Account { email: t.email, plan: t.plan })
}

/// Sleep in short slices so a cancel is honoured within ~50 ms.
fn sleep_cancellable(d: Duration, cancel: &AtomicBool) -> AiResult<()> {
    let end = Instant::now() + d;
    while Instant::now() < end {
        if cancel.load(Ordering::Relaxed) {
            return Err(AiError::Cancelled);
        }
        std::thread::sleep((end - Instant::now()).min(Duration::from_millis(50)));
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(AiError::Cancelled);
    }
    Ok(())
}

// ---------------------------------------------------------------- browser sign-in

/// A started browser sign-in: open `url` in the browser, then call [`BrowserLogin::wait`].
pub struct BrowserLogin {
    pub url: String,
    listener: TcpListener,
    verifier: String,
    state: String,
    redirect_uri: String,
}

/// Bind 127.0.0.1:1455 and build the authorize URL. `Err` (see [`is_port_busy`]) when the port is
/// busy, so the caller falls back to [`start_device_login`].
pub fn start_browser_login() -> AiResult<BrowserLogin> {
    start_browser_login_on(CALLBACK_PORT)
}

/// [`start_browser_login`] on another port (tests; the real client registration only allows 1455).
fn start_browser_login_on(port: u16) -> AiResult<BrowserLogin> {
    let ep = endpoints();
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            AiError::NotConfigured(format!("{PORT_BUSY_MESSAGE} by another program. Use the device code instead."))
        } else {
            AiError::NotConfigured(format!("Could not start the local sign-in listener: {e}"))
        }
    })?;
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    let verifier = random_b64(64)?;
    let state = random_b64(32)?;
    let redirect_uri = format!("http://localhost:{port}/auth/callback");
    let url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&code_challenge={}&code_challenge_method=S256\
         &id_token_add_organizations=true&codex_cli_simplified_flow=true&state={}",
        ep.authorize_url(),
        CLIENT_ID,
        urlencode(&redirect_uri),
        urlencode("openid profile email offline_access"),
        pkce_challenge(&verifier),
        state
    );
    Ok(BrowserLogin { url, listener, verifier, state, redirect_uri })
}

const PAGE_OK: &str =
    "<!doctype html><meta charset=utf-8><title>Rusty Inbox</title><body style=\"font-family:sans-serif;\
text-align:center;margin-top:20vh\"><h2>Signed in</h2><p>You can close this tab and return to Rusty Inbox.</p></body>";

fn page_error(msg: &str) -> String {
    let esc = msg.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!(
        "<!doctype html><meta charset=utf-8><title>Rusty Inbox</title><body style=\"font-family:sans-serif;\
         text-align:center;margin-top:20vh\"><h2>Sign-in failed</h2><p>{esc}</p><p>You can close this tab and \
         return to Rusty Inbox.</p></body>"
    )
}

fn respond(conn: &mut TcpStream, status: &str, html: &str) {
    let out = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
        html.len()
    );
    let _ = conn.write_all(out.as_bytes());
    let _ = conn.flush();
}

/// The request target (`/auth/callback?code=..`) of an HTTP GET, read from `conn`.
fn read_request_target(conn: &mut TcpStream) -> Option<String> {
    let _ = conn.set_nonblocking(false);
    let _ = conn.set_read_timeout(Some(Duration::from_secs(3)));
    let mut buf = vec![];
    let mut chunk = [0u8; 2048];
    while !buf.windows(2).any(|w| w == b"\r\n") && buf.len() < 16 * 1024 {
        match conn.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let line = String::from_utf8_lossy(&buf);
    let mut it = line.lines().next()?.split_whitespace();
    (it.next()? == "GET").then(|| it.next().map(str::to_string))?
}

fn query_param(target: &str, key: &str) -> Option<String> {
    let q = target.split_once('?')?.1;
    q.split('&').filter_map(|kv| kv.split_once('=')).find(|(k, _)| *k == key).map(|(_, v)| percent_decode(v))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl BrowserLogin {
    /// Wait for the redirect (answering the browser with a small "you can close this tab" page),
    /// exchange the code, save the tokens and return the account.
    ///
    /// A callback with a wrong `state` is answered with HTTP 400 and ignored (so a stray request
    /// cannot abort a real sign-in); other paths get 404. The wait ends with `Cancelled` when `cancel`
    /// is set, and with an error after `timeout`.
    pub fn wait(self, secrets: &dyn SecretStore, timeout: Duration, cancel: &AtomicBool) -> AiResult<Account> {
        let ep = endpoints();
        self.listener
            .set_nonblocking(true)
            .map_err(|e| AiError::NotConfigured(format!("Could not start the local sign-in listener: {e}")))?;
        let deadline = Instant::now() + timeout;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(AiError::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(AiError::Auth("the sign-in was not completed in time".into()));
            }
            let mut conn = match self.listener.accept() {
                Ok((c, _)) => c,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(40));
                    continue;
                }
                Err(e) => return Err(AiError::Network(e.to_string())),
            };
            let Some(target) = read_request_target(&mut conn) else {
                respond(&mut conn, "400 Bad Request", &page_error("Bad request."));
                continue;
            };
            if target.split('?').next() != Some("/auth/callback") {
                respond(&mut conn, "404 Not Found", &page_error("Not found."));
                continue;
            }
            if query_param(&target, "state").as_deref() != Some(self.state.as_str()) {
                respond(&mut conn, "400 Bad Request", &page_error("The sign-in state did not match; request ignored."));
                continue;
            }
            if let Some(err) = query_param(&target, "error") {
                let msg = query_param(&target, "error_description").unwrap_or(err);
                respond(&mut conn, "400 Bad Request", &page_error(&msg));
                return Err(AiError::Auth(msg));
            }
            let Some(code) = query_param(&target, "code").filter(|c| !c.is_empty()) else {
                respond(&mut conn, "400 Bad Request", &page_error("No authorization code was received."));
                continue;
            };
            return match exchange_code(&ep, secrets, &code, &self.redirect_uri, &self.verifier) {
                Ok(acc) => {
                    respond(&mut conn, "200 OK", PAGE_OK);
                    Ok(acc)
                }
                Err(e) => {
                    respond(&mut conn, "502 Bad Gateway", &page_error(&e.to_string()));
                    Err(e)
                }
            };
        }
    }
}

// ---------------------------------------------------------------- device code sign-in

/// A started device-code sign-in: show `verification_url` and `user_code`, then call
/// [`DeviceLogin::wait`].
#[derive(Clone, Debug, Default)]
pub struct DeviceLogin {
    pub verification_url: String,
    pub user_code: String,
    pub device_auth_id: String,
    pub interval_secs: u64,
}

fn json_u64(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

pub fn start_device_login() -> AiResult<DeviceLogin> {
    let ep = endpoints();
    let url = format!("{}/api/accounts/deviceauth/usercode", ep.issuer);
    let mut wait = ep.backoff;
    let mut attempt = 0;
    let resp = loop {
        match http::post_json_status(&url, &[], &json!({"client_id": CLIENT_ID})) {
            Ok(r) => break r,
            // 429: back off and retry a few times
            Err((429, e)) => {
                attempt += 1;
                if attempt >= 3 {
                    return Err(e);
                }
                std::thread::sleep(wait);
                wait *= 2;
            }
            Err((404, _)) => {
                return Err(AiError::NotConfigured("Device sign-in is not available for this account.".into()))
            }
            Err((_, e)) => return Err(e),
        }
    };
    let v = http::read_json(resp)?;
    let user_code = v["user_code"].as_str().or_else(|| v["usercode"].as_str()).unwrap_or_default();
    let id = v["device_auth_id"].as_str().unwrap_or_default();
    if user_code.is_empty() || id.is_empty() {
        return Err(AiError::Protocol("the sign-in server returned no device code".into()));
    }
    Ok(DeviceLogin {
        verification_url: format!("{}/codex/device", ep.issuer),
        user_code: user_code.to_string(),
        device_auth_id: id.to_string(),
        interval_secs: json_u64(&v["interval"]).unwrap_or(5).max(ep.poll_floor.as_secs()),
    })
}

impl DeviceLogin {
    /// Poll until approved (15 min max), exchange, save, return the account. 403/404 mean "not yet";
    /// 429 doubles the wait.
    pub fn wait(&self, secrets: &dyn SecretStore, cancel: &AtomicBool) -> AiResult<Account> {
        let ep = endpoints();
        let url = format!("{}/api/accounts/deviceauth/token", ep.issuer);
        let body = json!({"device_auth_id": self.device_auth_id, "user_code": self.user_code});
        let interval = Duration::from_secs(self.interval_secs).max(ep.poll_floor);
        let start = Instant::now();
        let mut extra = Duration::ZERO;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(AiError::Cancelled);
            }
            if start.elapsed() >= ep.device_limit {
                return Err(AiError::Auth("the sign-in code expired after 15 minutes; start again".into()));
            }
            match http::post_json_status(&url, &[], &body) {
                Ok(r) => {
                    let v = http::read_json(r)?;
                    let code = v["authorization_code"].as_str().unwrap_or_default();
                    let verifier = v["code_verifier"].as_str().unwrap_or_default();
                    if code.is_empty() || verifier.is_empty() {
                        return Err(AiError::Protocol("the sign-in server returned no authorization code".into()));
                    }
                    let redirect = format!("{}/deviceauth/callback", ep.issuer);
                    return exchange_code(&ep, secrets, code, &redirect, verifier);
                }
                Err((403 | 404, _)) => extra = Duration::ZERO,
                Err((429, _)) => extra = if extra.is_zero() { ep.backoff } else { (extra * 2).min(ep.backoff * 8) },
                Err((_, e)) => return Err(e),
            }
            sleep_cancellable(interval + extra, cancel)?;
        }
    }
}

// ---------------------------------------------------------------- tokens for requests

static REFRESH_LOCK: Mutex<()> = Mutex::new(());
/// The access token our last refresh produced and when (to not refresh twice for one 401 burst).
static LAST_REFRESH: Mutex<Option<(String, Instant)>> = Mutex::new(None);

fn refresh(ep: &Endpoints, secrets: &dyn SecretStore, old: &CodexTokens) -> AiResult<CodexTokens> {
    if old.refresh_token.is_empty() {
        return Err(AiError::Auth("Your ChatGPT sign-in expired. Sign in again in Settings → AI.".into()));
    }
    let resp = http::post_form(
        &ep.token_url(),
        &[],
        &[("grant_type", "refresh_token"), ("refresh_token", &old.refresh_token), ("client_id", CLIENT_ID)],
    )
    .map_err(|e| match e {
        // the refresh token was refused (expired, reused, revoked): only a new sign-in helps
        AiError::Auth(m) | AiError::Http { message: m, status: 400 } => {
            AiError::Auth(format!("Your ChatGPT sign-in expired. Sign in again in Settings → AI. ({m})"))
        }
        e => e,
    })?;
    let t = tokens_from_response(&http::read_json(resp)?, Some(old))?;
    // the old refresh token is dead from now on: persist the new pair before anyone uses it
    save_tokens(secrets, &t);
    if let Ok(mut l) = LAST_REFRESH.lock() {
        *l = Some((t.access_token.clone(), Instant::now()));
    }
    Ok(t)
}

/// A valid access token plus the headers the Codex backend needs (`Authorization`,
/// `ChatGPT-Account-ID`, residency, `originator: rusty-inbox`, `User-Agent`), refreshing first when it
/// expires within 2 minutes. `force_refresh` after a 401.
pub fn auth_headers(secrets: &dyn SecretStore, force_refresh: bool) -> AiResult<Vec<(&'static str, String)>> {
    let _guard = REFRESH_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let Some(mut t) = load_tokens(secrets).filter(|t| !t.access_token.is_empty()) else {
        return Err(AiError::NotConfigured("Sign in with ChatGPT in Settings → AI.".into()));
    };
    let expiring = t.expires_at - now_secs() < REFRESH_MARGIN_SECS;
    // a 401 right after another thread refreshed means that thread's token is the fresh one
    let just_refreshed = LAST_REFRESH
        .lock()
        .ok()
        .and_then(|l| l.clone())
        .is_some_and(|(a, at)| a == t.access_token && at.elapsed() < Duration::from_secs(10));
    if expiring || (force_refresh && !just_refreshed) {
        t = refresh(&endpoints(), secrets, &t)?;
    }
    let mut h = vec![
        ("Authorization", format!("Bearer {}", t.access_token)),
        ("ChatGPT-Account-ID", t.account_id.clone()),
        ("originator", "rusty-inbox".to_string()),
        ("User-Agent", http::USER_AGENT.to_string()),
    ];
    if !t.residency.is_empty() {
        h.push(("x-openai-internal-codex-residency", t.residency.clone()));
    }
    Ok(h)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::testutil::{Mock, Resp};
    use crate::MemorySecrets;

    /// Run `f` with the OAuth server redirected to `issuer` (this thread only) and no waiting.
    pub fn with_issuer<T>(issuer: &str, f: impl FnOnce() -> T) -> T {
        let ep = Endpoints {
            issuer: issuer.to_string(),
            poll_floor: Duration::ZERO,
            backoff: Duration::from_millis(30),
            device_limit: Duration::from_secs(5),
        };
        OVERRIDE.with(|o| *o.borrow_mut() = Some(ep));
        let r = f();
        OVERRIDE.with(|o| *o.borrow_mut() = None);
        r
    }

    /// A fake (unsigned) JWT with the claims the real access token carries.
    pub fn jwt(exp: i64, account: &str, plan: &str, residency: &str, email: &str) -> String {
        let payload = json!({
            "exp": exp,
            "https://api.openai.com/auth": {"chatgpt_account_id": account, "chatgpt_plan_type": plan,
                                            "chatgpt_data_residency": residency},
            "https://api.openai.com/profile": {"email": email},
        });
        format!("e30.{}.sig", URL_SAFE_NO_PAD.encode(payload.to_string()))
    }

    pub fn signed_in(secrets: &MemorySecrets, access: &str, refresh: &str, expires_at: i64) {
        let t = CodexTokens {
            access_token: access.into(),
            refresh_token: refresh.into(),
            account_id: "acct-1".into(),
            residency: "eu".into(),
            email: "me@example.com".into(),
            plan: "plus".into(),
            expires_at,
            ..Default::default()
        };
        secrets.set(SECRET_KEY, &serde_json::to_string(&t).unwrap());
    }

    pub fn now() -> i64 {
        now_secs()
    }

    fn token_answer(access: &str, refresh: &str) -> Value {
        json!({"access_token": access, "refresh_token": refresh, "id_token": "", "expires_in": 3600})
    }

    #[test]
    fn pkce_matches_the_rfc_7636_test_vector() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn claims_come_from_the_jwt_payload() {
        let c = parse_claims(&jwt(1_900_000_000, "acct-9", "pro", "us", "a@b.c")).unwrap();
        assert_eq!(
            c,
            Claims {
                exp: 1_900_000_000,
                account_id: "acct-9".into(),
                residency: "us".into(),
                plan: "pro".into(),
                email: "a@b.c".into()
            }
        );
        // compute residency is the fallback
        let p = json!({"exp": 5, "https://api.openai.com/auth": {"chatgpt_account_id": "x", "chatgpt_compute_residency": "eu"}});
        let c = parse_claims(&format!("h.{}.s", URL_SAFE_NO_PAD.encode(p.to_string()))).unwrap();
        assert_eq!((c.residency.as_str(), c.email.as_str()), ("eu", ""));
        assert!(parse_claims("not a jwt").is_none());
        assert!(parse_claims("a.@@@.c").is_none());
    }

    #[test]
    fn auth_headers_carry_account_residency_and_originator() {
        let s = MemorySecrets::default();
        signed_in(&s, "tok-1", "ref-1", now_secs() + 3600);
        let h = auth_headers(&s, false).unwrap();
        let get = |k: &str| h.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone());
        assert_eq!(get("Authorization").unwrap(), "Bearer tok-1");
        assert_eq!(get("ChatGPT-Account-ID").unwrap(), "acct-1");
        assert_eq!(get("x-openai-internal-codex-residency").unwrap(), "eu");
        assert_eq!(get("originator").unwrap(), "rusty-inbox");
        assert!(get("User-Agent").unwrap().starts_with("RustyInbox/"));
        assert_eq!(account(&s), Some(Account { email: "me@example.com".into(), plan: "plus".into() }));
        sign_out(&s);
        assert!(account(&s).is_none());
        assert!(matches!(auth_headers(&s, false), Err(AiError::NotConfigured(_))));
    }

    #[test]
    fn an_expiring_token_is_refreshed_and_the_rotated_pair_is_saved() {
        let access2 = jwt(now_secs() + 7200, "acct-1", "plus", "", "me@example.com");
        let mock = Mock::scripted(vec![Resp::json(200, &token_answer(&access2, "ref-2"))]);
        let s = MemorySecrets::default();
        signed_in(&s, "old-access", "ref-1", now_secs() + 30);
        let h = with_issuer(&mock.url, || auth_headers(&s, false)).unwrap();
        assert_eq!(h[0].1, format!("Bearer {access2}"));
        let req = &mock.requests()[0];
        assert_eq!(req.path, "/oauth/token");
        let f = req.form();
        assert_eq!(f["grant_type"], "refresh_token");
        assert_eq!(f["refresh_token"], "ref-1");
        assert_eq!(f["client_id"], CLIENT_ID);
        let saved = load_tokens(&s).unwrap();
        assert_eq!(saved.refresh_token, "ref-2");
        assert_eq!(saved.access_token, access2);
        // fields the answer omitted are kept
        assert_eq!(saved.account_id, "acct-1");
        assert_eq!(saved.plan, "plus");
        // a fresh token is not refreshed again
        with_issuer(&mock.url, || auth_headers(&s, false)).unwrap();
        assert_eq!(mock.requests().len(), 1);
    }

    #[test]
    fn a_rejected_refresh_token_asks_for_a_new_sign_in_and_keeps_the_provider_message() {
        let mock = Mock::scripted(vec![Resp::json(
            400,
            &json!({"error": {"message": "Your refresh token has already been used", "code": "refresh_token_reused"}}),
        )]);
        let s = MemorySecrets::default();
        signed_in(&s, "a", "r", 0);
        let e = with_issuer(&mock.url, || auth_headers(&s, false)).unwrap_err();
        match e {
            AiError::Auth(m) => {
                assert!(m.contains("Sign in again"), "{m}");
                assert!(m.contains("already been used"), "{m}");
            }
            e => panic!("{e:?}"),
        }
        // an empty refresh token never touches the network
        signed_in(&s, "a", "", 0);
        assert!(matches!(with_issuer(&mock.url, || auth_headers(&s, true)), Err(AiError::Auth(_))));
        assert_eq!(mock.requests().len(), 1);
    }

    #[test]
    fn concurrent_callers_refresh_once() {
        let access2 = jwt(now_secs() + 7200, "acct-1", "plus", "", "me@example.com");
        let a2 = access2.clone();
        let mock = Mock::start(move |_, _| {
            std::thread::sleep(Duration::from_millis(100));
            Resp::json(200, &token_answer(&a2, "ref-2"))
        });
        let s = std::sync::Arc::new(MemorySecrets::default());
        signed_in(&s, "old", "ref-1", 0);
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let (s, url) = (s.clone(), mock.url.clone());
                std::thread::spawn(move || with_issuer(&url, || auth_headers(&*s, false)).unwrap()[0].1.clone())
            })
            .collect();
        for h in handles {
            assert_eq!(h.join().unwrap(), format!("Bearer {access2}"));
        }
        assert_eq!(mock.requests().len(), 1, "one refresh for four callers");
    }

    #[test]
    fn browser_login_builds_the_exact_authorize_url() {
        let l = start_browser_login_on(0).unwrap();
        let (base, q) = l.url.split_once('?').unwrap();
        assert_eq!(base, "https://auth.openai.com/oauth/authorize");
        let want = [
            ("response_type", "code"),
            ("client_id", CLIENT_ID),
            ("scope", "openid%20profile%20email%20offline_access"),
            ("code_challenge_method", "S256"),
            ("id_token_add_organizations", "true"),
            ("codex_cli_simplified_flow", "true"),
        ];
        let params: Vec<(&str, &str)> = q.split('&').filter_map(|kv| kv.split_once('=')).collect();
        for (k, v) in want {
            assert!(params.contains(&(k, v)), "missing {k}={v} in {q}");
        }
        let get = |k: &str| params.iter().find(|(n, _)| *n == k).unwrap().1;
        assert_eq!(get("code_challenge"), pkce_challenge(&l.verifier));
        assert_eq!(get("state"), l.state);
        assert!(l.state.len() >= 32 && l.verifier.len() >= 43);
        assert!(get("redirect_uri").starts_with("http%3A%2F%2Flocalhost%3A"));
        assert!(get("redirect_uri").ends_with("%2Fauth%2Fcallback"));
        // the default listener asks for the registered port
        assert_eq!(urlencode("http://localhost:1455/auth/callback"), "http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback");
    }

    fn http_get(port: u16, target: &str) -> String {
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(c, "GET {target} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
        let mut s = String::new();
        c.read_to_string(&mut s).unwrap();
        s
    }

    #[test]
    fn browser_callback_with_a_bad_state_is_rejected_and_a_good_one_signs_in() {
        let access = jwt(now_secs() + 3600, "acct-7", "plus", "eu", "who@example.com");
        let mock = Mock::scripted(vec![Resp::json(200, &token_answer(&access, "ref-new"))]);
        let s = MemorySecrets::default();
        let (bad, good) = with_issuer(&mock.url, || {
            let login = start_browser_login_on(0).unwrap();
            let port = login.listener.local_addr().unwrap().port();
            let state = login.state.clone();
            let verifier = login.verifier.clone();
            let t = std::thread::spawn(move || {
                let bad = http_get(port, "/auth/callback?code=evil&state=WRONG");
                let other = http_get(port, "/favicon.ico");
                let good = http_get(port, &format!("/auth/callback?code=the%20code&state={state}"));
                (bad, other, good, verifier)
            });
            let acc = login.wait(&s, Duration::from_secs(10), &AtomicBool::new(false)).unwrap();
            let (bad, other, good, verifier) = t.join().unwrap();
            assert!(other.starts_with("HTTP/1.1 404"));
            assert_eq!(acc, Account { email: "who@example.com".into(), plan: "plus".into() });
            let reqs = mock.requests();
            assert_eq!(reqs.len(), 1, "the wrong-state callback must not reach the token endpoint");
            let f = reqs[0].form();
            assert_eq!(f["grant_type"], "authorization_code");
            assert_eq!(f["code"], "the code");
            assert_eq!(f["client_id"], CLIENT_ID);
            assert_eq!(f["code_verifier"], verifier);
            assert!(f["redirect_uri"].starts_with("http://localhost:"));
            assert!(f["redirect_uri"].ends_with("/auth/callback"));
            (bad, good)
        });
        assert!(bad.starts_with("HTTP/1.1 400"), "{bad}");
        assert!(good.starts_with("HTTP/1.1 200"), "{good}");
        assert!(good.contains("You can close this tab and return to Rusty Inbox"));
        let saved = load_tokens(&s).unwrap();
        assert_eq!(
            (saved.account_id.as_str(), saved.residency.as_str(), saved.refresh_token.as_str()),
            ("acct-7", "eu", "ref-new")
        );
    }

    #[test]
    fn browser_login_reports_provider_errors_and_honours_cancel_and_timeout() {
        let s = MemorySecrets::default();
        let login = start_browser_login_on(0).unwrap();
        let port = login.listener.local_addr().unwrap().port();
        let state = login.state.clone();
        let t = std::thread::spawn(move || {
            http_get(
                port,
                &format!("/auth/callback?error=access_denied&error_description=User%20said%20no&state={state}"),
            )
        });
        let e = login.wait(&s, Duration::from_secs(10), &AtomicBool::new(false)).unwrap_err();
        assert!(matches!(&e, AiError::Auth(m) if m == "User said no"), "{e:?}");
        assert!(t.join().unwrap().contains("User said no"));

        let cancel = AtomicBool::new(true);
        let l = start_browser_login_on(0).unwrap();
        assert!(matches!(l.wait(&s, Duration::from_secs(10), &cancel), Err(AiError::Cancelled)));
        let l = start_browser_login_on(0).unwrap();
        assert!(matches!(l.wait(&s, Duration::from_millis(100), &AtomicBool::new(false)), Err(AiError::Auth(_))));
        assert!(load_tokens(&s).is_none());
    }

    #[test]
    fn a_busy_port_is_a_recognisable_error() {
        let holder = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = holder.local_addr().unwrap().port();
        let e = start_browser_login_on(port).err().unwrap();
        assert!(is_port_busy(&e), "{e:?}");
        assert!(e.to_string().starts_with(PORT_BUSY_MESSAGE));
        assert!(!is_port_busy(&AiError::Cancelled));
    }

    #[test]
    fn device_login_polls_through_pending_and_rate_limits_then_signs_in() {
        let access = jwt(now_secs() + 3600, "acct-d", "pro", "", "dev@example.com");
        let a2 = access.clone();
        let polls = std::sync::atomic::AtomicUsize::new(0);
        let mock = Mock::start(move |req, _| match req.path.as_str() {
            "/api/accounts/deviceauth/usercode" => {
                Resp::json(200, &json!({"device_auth_id": "dev-1", "user_code": "ABCD-1234", "interval": "0"}))
            }
            "/api/accounts/deviceauth/token" => match polls.fetch_add(1, Ordering::SeqCst) {
                0 => Resp::json(403, &json!({"error": {"message": "deviceauth_authorization_pending"}})),
                1 => Resp::text(404, "text/plain", ""),
                2 => Resp::json(429, &json!({"error": "slow down"})),
                _ => Resp::json(200, &json!({"authorization_code": "AC", "code_verifier": "CV"})),
            },
            "/oauth/token" => Resp::json(200, &token_answer(&a2, "ref-dev")),
            p => panic!("unexpected {p}"),
        });
        let s = MemorySecrets::default();
        let acc = with_issuer(&mock.url, || {
            let d = start_device_login().unwrap();
            assert_eq!(d.user_code, "ABCD-1234");
            assert_eq!(d.verification_url, format!("{}/codex/device", mock.url));
            d.wait(&s, &AtomicBool::new(false)).unwrap()
        });
        assert_eq!(acc, Account { email: "dev@example.com".into(), plan: "pro".into() });
        let reqs = mock.requests();
        assert_eq!(reqs[0].json()["client_id"], CLIENT_ID);
        assert_eq!(reqs[1].json(), json!({"device_auth_id": "dev-1", "user_code": "ABCD-1234"}));
        assert_eq!(reqs.iter().filter(|r| r.path.ends_with("/token")).count(), 5); // 4 polls + 1 exchange
        let ex = reqs.last().unwrap().form();
        assert_eq!(ex["code"], "AC");
        assert_eq!(ex["code_verifier"], "CV");
        assert_eq!(ex["redirect_uri"], format!("{}/deviceauth/callback", mock.url));
        assert_eq!(load_tokens(&s).unwrap().refresh_token, "ref-dev");
    }

    #[test]
    fn device_login_can_be_cancelled_and_has_a_time_limit() {
        let mock = Mock::start(|_, _| Resp::text(404, "text/plain", "pending"));
        let s = MemorySecrets::default();
        let d =
            DeviceLogin { user_code: "X".into(), device_auth_id: "d".into(), interval_secs: 0, ..Default::default() };
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let c2 = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            c2.store(true, Ordering::Relaxed);
        });
        let t = Instant::now();
        let r = with_issuer(&mock.url, || d.wait(&s, &cancel));
        assert!(matches!(r, Err(AiError::Cancelled)));
        assert!(t.elapsed() < Duration::from_secs(2));
        // the limit: shrink it through a custom endpoint set
        OVERRIDE.with(|o| {
            *o.borrow_mut() = Some(Endpoints {
                issuer: mock.url.clone(),
                poll_floor: Duration::ZERO,
                backoff: Duration::from_millis(10),
                device_limit: Duration::from_millis(100),
            })
        });
        let r = d.wait(&s, &AtomicBool::new(false));
        OVERRIDE.with(|o| *o.borrow_mut() = None);
        assert!(matches!(r, Err(AiError::Auth(m)) if m.contains("15 minutes")));
        assert!(load_tokens(&s).is_none());
    }

    #[test]
    fn device_login_start_errors_keep_the_provider_message() {
        let mock = Mock::scripted(vec![Resp::json(500, &json!({"error": {"message": "boom"}}))]);
        let e = with_issuer(&mock.url, start_device_login).unwrap_err();
        assert!(matches!(e, AiError::Http { status: 500, ref message } if message == "boom"), "{e:?}");
        let mock = Mock::scripted(vec![Resp::text(404, "text/plain", "")]);
        assert!(matches!(with_issuer(&mock.url, start_device_login), Err(AiError::NotConfigured(_))));
    }
}
