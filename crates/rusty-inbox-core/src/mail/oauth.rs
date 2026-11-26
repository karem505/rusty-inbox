//! Google OAuth2 for Gmail (RFC 8252 installed-app flow): PKCE (RFC 7636), a loopback redirect on
//! `127.0.0.1:<random port>`, and the XOAUTH2 SASL string IMAP and SMTP use to log in with the
//! access token. The refresh token is kept in the secret store under `oauth:<email>` and access
//! tokens are renewed on demand by the engine.
//!
//! Everything that can be tested without a network (URLs, PKCE, the redirect parser, the token
//! response parser, the XOAUTH2 string) is a plain function; only [`AuthSession`] and
//! [`refresh_access_token`] talk to Google.

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::store::Store;

pub const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
/// Full Gmail IMAP/SMTP access. This is a Google "restricted" scope.
pub const GMAIL_SCOPE: &str = "https://mail.google.com/";

/// How long the browser sign-in may take before it is abandoned.
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// What an IMAP or SMTP login presents: a password, or an OAuth2 access token (XOAUTH2).
#[derive(Clone, PartialEq, Eq)]
pub enum Credential {
    Password(String),
    XOAuth2(String),
}

impl Credential {
    pub fn password(p: &str) -> Credential {
        Credential::Password(p.to_string())
    }
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // never print the secret
        match self {
            Credential::Password(_) => f.write_str("Credential::Password(..)"),
            Credential::XOAuth2(_) => f.write_str("Credential::XOAuth2(..)"),
        }
    }
}

/// The Google OAuth client the app signs in with. For a desktop app Google issues a client
/// secret too, but it is not confidential (it ships with the app), so it may be empty.
#[derive(Clone, PartialEq, Eq)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: Option<String>,
}

impl std::fmt::Debug for OAuthConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthConfig").field("client_id", &self.client_id).finish_non_exhaustive()
    }
}

/// Client ID compiled in by the maintainer (`RUSTY_INBOX_GOOGLE_CLIENT_ID` at build time).
const BUILT_IN_CLIENT_ID: Option<&str> = option_env!("RUSTY_INBOX_GOOGLE_CLIENT_ID");
const BUILT_IN_CLIENT_SECRET: Option<&str> = option_env!("RUSTY_INBOX_GOOGLE_CLIENT_SECRET");

fn non_empty(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Pick the client: the user's own (Settings, `googleClientId` / `googleClientSecret`) beats the
/// environment at run time, which beats the one compiled in. `None` means Gmail sign-in is not
/// set up in this build.
pub fn resolve_config(
    user_id: Option<String>,
    user_secret: Option<String>,
    env_id: Option<String>,
    env_secret: Option<String>,
    built_id: Option<&str>,
    built_secret: Option<&str>,
) -> Option<OAuthConfig> {
    if let Some(client_id) = non_empty(user_id) {
        return Some(OAuthConfig { client_id, client_secret: non_empty(user_secret) });
    }
    if let Some(client_id) = non_empty(env_id) {
        return Some(OAuthConfig { client_id, client_secret: non_empty(env_secret) });
    }
    let client_id = non_empty(built_id.map(str::to_string))?;
    Some(OAuthConfig { client_id, client_secret: non_empty(built_secret.map(str::to_string)) })
}

/// The configured client, if any (see [`resolve_config`]).
pub fn config(store: &Store) -> Option<OAuthConfig> {
    let setting = |k: &str| {
        store.setting(&format!("rustyInbox.{k}")).ok().flatten().and_then(|v| v.as_str().map(str::to_string))
    };
    resolve_config(
        setting("googleClientId"),
        setting("googleClientSecret"),
        std::env::var("RUSTY_INBOX_GOOGLE_CLIENT_ID").ok(),
        std::env::var("RUSTY_INBOX_GOOGLE_CLIENT_SECRET").ok(),
        BUILT_IN_CLIENT_ID,
        BUILT_IN_CLIENT_SECRET,
    )
}

/// Shown when no client ID is configured.
pub const NOT_CONFIGURED: &str = "Gmail sign-in is not set up in this build (no Google client ID). \
Use a Google App Password instead, or enter your own Google OAuth client ID in Settings.";

// ---------------------------------------------------------------------------------------------
// XOAUTH2
// ---------------------------------------------------------------------------------------------

/// The XOAUTH2 initial client response (before base64): `user=<user>^Aauth=Bearer <token>^A^A`.
pub fn xoauth2_string(user: &str, access_token: &str) -> String {
    format!("user={user}\x01auth=Bearer {access_token}\x01\x01")
}

/// IMAP `AUTHENTICATE XOAUTH2`. On a failed login the server sends a (base64 JSON) error
/// challenge; the client answers with an empty line so the server can finish with `NO`.
pub struct XOAuth2Auth {
    pub user: String,
    pub token: String,
}

impl async_imap::Authenticator for XOAuth2Auth {
    type Response = String;
    fn process(&mut self, challenge: &[u8]) -> Self::Response {
        if challenge.is_empty() {
            xoauth2_string(&self.user, &self.token)
        } else {
            String::new()
        }
    }
}

// ---------------------------------------------------------------------------------------------
// PKCE and URLs
// ---------------------------------------------------------------------------------------------

/// Everything except the RFC 3986 unreserved characters is escaped.
const URL_ESCAPE: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~');

pub fn url_encode(s: &str) -> String {
    utf8_percent_encode(s, URL_ESCAPE).to_string()
}

pub fn form_encode(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", url_encode(k), url_encode(v))).collect::<Vec<_>>().join("&")
}

fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// 32 random bytes as unpadded base64url (43 characters), for the PKCE verifier and `state`.
pub fn random_token() -> String {
    let mut bytes = Vec::with_capacity(32);
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    b64url(&bytes)
}

/// PKCE S256: `BASE64URL(SHA256(verifier))`.
pub fn pkce_challenge(verifier: &str) -> String {
    b64url(&Sha256::digest(verifier.as_bytes()))
}

pub fn authorization_url(
    cfg: &OAuthConfig,
    redirect_uri: &str,
    challenge: &str,
    state: &str,
    login_hint: Option<&str>,
) -> String {
    let mut pairs: Vec<(&str, &str)> = vec![
        ("client_id", &cfg.client_id),
        ("redirect_uri", redirect_uri),
        ("response_type", "code"),
        ("scope", GMAIL_SCOPE),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("state", state),
        // a refresh token is only issued with offline access, and again only after consent
        ("access_type", "offline"),
        ("prompt", "consent"),
    ];
    if let Some(h) = login_hint.filter(|h| !h.is_empty()) {
        pairs.push(("login_hint", h));
    }
    format!("{AUTH_ENDPOINT}?{}", form_encode(&pairs))
}

/// What the browser sent to the loopback redirect.
#[derive(Debug, PartialEq, Eq)]
pub enum Redirect {
    /// The authorization code, with a matching `state`.
    Code(String),
    /// The user denied access or Google reported an error.
    Error(String),
    /// Not the redirect (e.g. `/favicon.ico`), or the `state` does not match.
    Ignore,
}

/// Parse the first line of the browser's request (`GET /?code=...&state=... HTTP/1.1`).
pub fn parse_redirect(request_line: &str, expected_state: &str) -> Redirect {
    let mut parts = request_line.split_whitespace();
    if parts.next() != Some("GET") {
        return Redirect::Ignore;
    }
    let Some(target) = parts.next() else { return Redirect::Ignore };
    let Some((path, query)) = target.split_once('?') else { return Redirect::Ignore };
    if path != "/" {
        return Redirect::Ignore;
    }
    let (mut code, mut error, mut state) = (None, None, None);
    for kv in query.split('&') {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        let v = percent_decode_str(&v.replace('+', " ")).decode_utf8_lossy().to_string();
        match k {
            "code" => code = Some(v),
            "error" => error = Some(v),
            "state" => state = Some(v),
            _ => {}
        }
    }
    // a request without our state did not come from the sign-in we started
    if state.as_deref() != Some(expected_state) {
        return Redirect::Ignore;
    }
    match (code, error) {
        (_, Some(e)) => Redirect::Error(e),
        (Some(c), None) if !c.is_empty() => Redirect::Code(c),
        _ => Redirect::Ignore,
    }
}

// ---------------------------------------------------------------------------------------------
// tokens
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenSet {
    pub access_token: String,
    /// Only present when the user signs in (and not on a refresh).
    pub refresh_token: Option<String>,
    pub expires_in: Duration,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    error: Option<String>,
    error_description: Option<String>,
}

/// Parse Google's token endpoint answer; error answers become readable messages.
pub fn parse_token_response(status: u16, body: &str) -> Result<TokenSet> {
    let r: TokenResponse = serde_json::from_str(body).map_err(|_| anyhow!("Google answered with HTTP {status}"))?;
    if let Some(e) = r.error {
        if e == "invalid_grant" {
            bail!("Google no longer accepts this sign-in (it was revoked or has expired). Remove the account and add it again");
        }
        let d = r.error_description.unwrap_or_default();
        bail!("Google sign-in failed: {e}{}", if d.is_empty() { String::new() } else { format!(" ({d})") });
    }
    if !(200..300).contains(&status) {
        bail!("Google answered with HTTP {status}");
    }
    let access_token = r.access_token.filter(|t| !t.is_empty()).ok_or_else(|| anyhow!("Google sent no access token"))?;
    Ok(TokenSet {
        access_token,
        refresh_token: r.refresh_token.filter(|t| !t.is_empty()),
        expires_in: Duration::from_secs(r.expires_in.unwrap_or(3600)),
    })
}

/// A cached access token is renewed this long before it expires.
pub const EXPIRY_MARGIN: Duration = Duration::from_secs(60);

fn client_fields<'a>(cfg: &'a OAuthConfig, out: &mut Vec<(&'a str, &'a str)>) {
    out.push(("client_id", &cfg.client_id));
    if let Some(s) = &cfg.client_secret {
        out.push(("client_secret", s));
    }
}

pub fn exchange_body(cfg: &OAuthConfig, code: &str, verifier: &str, redirect_uri: &str) -> String {
    let mut pairs = vec![
        ("code", code),
        ("code_verifier", verifier),
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect_uri),
    ];
    client_fields(cfg, &mut pairs);
    form_encode(&pairs)
}

pub fn refresh_body(cfg: &OAuthConfig, refresh_token: &str) -> String {
    let mut pairs = vec![("refresh_token", refresh_token), ("grant_type", "refresh_token")];
    client_fields(cfg, &mut pairs);
    form_encode(&pairs)
}

/// POST a form to the token endpoint (blocking; call from `spawn_blocking`).
fn post_token_form(body: String) -> Result<TokenSet> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .user_agent(concat!("RustyInbox/", env!("CARGO_PKG_VERSION")))
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_global(Some(Duration::from_secs(40)))
        .build()
        .into();
    let mut resp = agent
        .post(TOKEN_ENDPOINT)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(body.as_str())
        .context("could not reach Google")?;
    let status = resp.status().as_u16();
    let text = resp.body_mut().read_to_string().context("reading Google's answer")?;
    parse_token_response(status, &text)
}

/// Trade a refresh token for a fresh access token.
pub async fn refresh_access_token(cfg: &OAuthConfig, refresh_token: &str) -> Result<TokenSet> {
    let body = refresh_body(cfg, refresh_token);
    tokio::task::spawn_blocking(move || post_token_form(body)).await?
}

// ---------------------------------------------------------------------------------------------
// the browser sign-in
// ---------------------------------------------------------------------------------------------

static CANCEL: tokio::sync::Notify = tokio::sync::Notify::const_new();

/// Abandon a sign-in that is waiting for the browser.
pub fn cancel_sign_in() {
    CANCEL.notify_waiters();
}

/// A started sign-in: open [`AuthSession::url`] in the browser, then [`AuthSession::finish`].
pub struct AuthSession {
    pub url: String,
    cfg: OAuthConfig,
    listener: tokio::net::TcpListener,
    verifier: String,
    state: String,
    redirect_uri: String,
}

const DONE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Rusty Inbox</title>\
<body style=\"font-family:sans-serif;text-align:center;margin-top:20vh\">\
<h2>You are signed in</h2><p>You can close this tab and return to Rusty Inbox.</p></body>";
const FAILED_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Rusty Inbox</title>\
<body style=\"font-family:sans-serif;text-align:center;margin-top:20vh\">\
<h2>Sign-in was not completed</h2><p>Return to Rusty Inbox to try again.</p></body>";

impl AuthSession {
    /// Listen on a random loopback port and build the Google sign-in URL.
    pub async fn start(cfg: &OAuthConfig, login_hint: Option<&str>) -> Result<AuthSession> {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.context("cannot listen on 127.0.0.1")?;
        let port = listener.local_addr()?.port();
        let redirect_uri = format!("http://127.0.0.1:{port}");
        let verifier = random_token();
        let state = random_token();
        let url = authorization_url(cfg, &redirect_uri, &pkce_challenge(&verifier), &state, login_hint);
        Ok(AuthSession { url, cfg: cfg.clone(), listener, verifier, state, redirect_uri })
    }

    /// Wait for the browser to come back, then exchange the code for tokens.
    pub async fn finish(self, limit: Duration) -> Result<TokenSet> {
        let code = tokio::select! {
            r = tokio::time::timeout(limit, self.wait_for_code()) => r.map_err(|_| anyhow!("Sign-in timed out. Try again"))??,
            _ = CANCEL.notified() => bail!("Sign-in cancelled"),
        };
        let body = exchange_body(&self.cfg, &code, &self.verifier, &self.redirect_uri);
        tokio::task::spawn_blocking(move || post_token_form(body)).await?
    }

    async fn wait_for_code(&self) -> Result<String> {
        loop {
            let (mut sock, peer) = self.listener.accept().await?;
            if !peer.ip().is_loopback() {
                continue;
            }
            let mut buf = vec![0u8; 8192];
            let mut n = 0;
            // the request line is all we need
            let read = tokio::time::timeout(Duration::from_secs(5), async {
                while n < buf.len() {
                    let k = sock.read(&mut buf[n..]).await?;
                    if k == 0 {
                        break;
                    }
                    n += k;
                    if buf[..n].windows(2).any(|w| w == b"\r\n") {
                        break;
                    }
                }
                Ok::<_, std::io::Error>(())
            })
            .await;
            if read.is_err() || n == 0 {
                continue;
            }
            let head = String::from_utf8_lossy(&buf[..n]).to_string();
            let line = head.lines().next().unwrap_or("");
            let (status, page, outcome) = match parse_redirect(line, &self.state) {
                Redirect::Code(c) => ("200 OK", DONE_PAGE, Some(Ok(c))),
                Redirect::Error(e) => ("200 OK", FAILED_PAGE, Some(Err(e))),
                Redirect::Ignore => ("404 Not Found", "", None),
            };
            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
                page.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.shutdown().await;
            match outcome {
                Some(Ok(code)) => return Ok(code),
                Some(Err(e)) if e == "access_denied" => bail!("Access was not granted in the browser"),
                Some(Err(e)) => bail!("Google sign-in failed: {e}"),
                None => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_imap::Authenticator;

    fn cfg() -> OAuthConfig {
        OAuthConfig { client_id: "id.apps.googleusercontent.com".into(), client_secret: Some("s3cret".into()) }
    }

    #[test]
    fn xoauth2_matches_googles_documented_format() {
        let s = xoauth2_string("someuser@example.com", "ya29.vF9dft4qmTc2Nvb3RlckBhdHRhdmlzdGEuY29tCg");
        assert_eq!(
            s,
            "user=someuser@example.com\x01auth=Bearer ya29.vF9dft4qmTc2Nvb3RlckBhdHRhdmlzdGEuY29tCg\x01\x01"
        );
        // Google's documented base64 form of the same string
        let b = base64::engine::general_purpose::STANDARD.encode(xoauth2_string("someuser@example.com", "ya29.vF9dft4qmTc2Nvb3RlckBhdHRhdmlzdGEuY29tCg"));
        assert!(b.starts_with("dXNlcj1zb21ldXNlckBleGFtcGxlLmNvbQFhdXRoPUJlYXJlciB5YTI5"));
    }

    #[test]
    fn imap_authenticator_answers_once_then_declines() {
        let mut a = XOAuth2Auth { user: "u@x.org".into(), token: "tok".into() };
        assert_eq!(a.process(b""), "user=u@x.org\x01auth=Bearer tok\x01\x01");
        // the server's error challenge gets an empty answer
        assert_eq!(a.process(b"{\"status\":\"401\"}"), "");
    }

    #[test]
    fn pkce_challenge_matches_rfc7636_example() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn random_tokens_are_distinct_and_url_safe() {
        let (a, b) = (random_token(), random_token());
        assert_ne!(a, b);
        assert_eq!(a.len(), 43);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn authorization_url_carries_pkce_scope_and_hint() {
        let u = authorization_url(&cfg(), "http://127.0.0.1:5555", "CHAL", "STATE", Some("me@gmail.com"));
        assert!(u.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
        assert!(u.contains("client_id=id.apps.googleusercontent.com"));
        assert!(u.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A5555"));
        assert!(u.contains("scope=https%3A%2F%2Fmail.google.com%2F"));
        assert!(u.contains("code_challenge=CHAL&code_challenge_method=S256"));
        assert!(u.contains("state=STATE"));
        assert!(u.contains("access_type=offline"));
        assert!(u.contains("login_hint=me%40gmail.com"));
        assert!(!u.contains("s3cret"), "the client secret never goes in the browser URL");
    }

    #[test]
    fn redirect_parser_accepts_only_our_state() {
        assert_eq!(
            parse_redirect("GET /?state=ST&code=4%2F0AbC&scope=x HTTP/1.1", "ST"),
            Redirect::Code("4/0AbC".into())
        );
        assert_eq!(parse_redirect("GET /?state=EVIL&code=c HTTP/1.1", "ST"), Redirect::Ignore);
        assert_eq!(parse_redirect("GET /?code=c HTTP/1.1", "ST"), Redirect::Ignore);
        assert_eq!(parse_redirect("GET /favicon.ico HTTP/1.1", "ST"), Redirect::Ignore);
        assert_eq!(parse_redirect("POST /?state=ST&code=c HTTP/1.1", "ST"), Redirect::Ignore);
        assert_eq!(
            parse_redirect("GET /?error=access_denied&state=ST HTTP/1.1", "ST"),
            Redirect::Error("access_denied".into())
        );
    }

    #[test]
    fn token_bodies_include_the_secret_only_when_configured() {
        let b = exchange_body(&cfg(), "c/1", "ver", "http://127.0.0.1:9");
        assert!(b.contains("grant_type=authorization_code"));
        assert!(b.contains("code=c%2F1"));
        assert!(b.contains("code_verifier=ver"));
        assert!(b.contains("client_secret=s3cret"));
        let public = OAuthConfig { client_secret: None, ..cfg() };
        assert!(!refresh_body(&public, "r").contains("client_secret"));
        assert!(refresh_body(&public, "1//r").contains("refresh_token=1%2F%2Fr&grant_type=refresh_token"));
    }

    #[test]
    fn token_response_parsing() {
        let t = parse_token_response(200, r#"{"access_token":"a","expires_in":3599,"refresh_token":"r","scope":"x"}"#).unwrap();
        assert_eq!(t.access_token, "a");
        assert_eq!(t.refresh_token.as_deref(), Some("r"));
        assert_eq!(t.expires_in, Duration::from_secs(3599));
        // a refresh answer has no new refresh token
        let t = parse_token_response(200, r#"{"access_token":"a2","expires_in":3599}"#).unwrap();
        assert_eq!(t.refresh_token, None);
        let e = parse_token_response(400, r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#)
            .unwrap_err();
        assert!(e.to_string().contains("Remove the account and add it again"));
        assert!(parse_token_response(400, r#"{"error":"invalid_client"}"#).unwrap_err().to_string().contains("invalid_client"));
        assert!(parse_token_response(502, "<html>").is_err());
        assert!(parse_token_response(200, r#"{"expires_in":1}"#).is_err());
    }

    #[test]
    fn config_precedence() {
        let s = |x: &str| Some(x.to_string());
        // nothing configured: Gmail sign-in is off
        assert_eq!(resolve_config(None, None, None, None, None, None), None);
        assert_eq!(resolve_config(s("  "), None, s(""), None, Some(""), None), None);
        // compiled in
        let c = resolve_config(None, None, None, None, Some("built"), Some("bs")).unwrap();
        assert_eq!((c.client_id.as_str(), c.client_secret.as_deref()), ("built", Some("bs")));
        // the environment beats the build, the user's setting beats both
        let c = resolve_config(None, None, s("env"), None, Some("built"), Some("bs")).unwrap();
        assert_eq!((c.client_id.as_str(), c.client_secret.as_deref()), ("env", None));
        let c = resolve_config(s(" mine "), s("ms"), s("env"), s("es"), Some("built"), None).unwrap();
        assert_eq!((c.client_id.as_str(), c.client_secret.as_deref()), ("mine", Some("ms")));
    }

    #[test]
    fn debug_output_hides_secrets() {
        assert!(!format!("{:?}", Credential::XOAuth2("tok".into())).contains("tok"));
        assert!(!format!("{:?}", cfg()).contains("s3cret"));
    }

    #[tokio::test]
    async fn loopback_redirect_returns_the_code() {
        let session = AuthSession::start(&cfg(), None).await.unwrap();
        let addr = session.listener.local_addr().unwrap();
        assert!(addr.ip().is_loopback());
        let state = session.state.clone();
        let client = tokio::spawn(async move {
            use tokio::net::TcpStream;
            // a stray request first (browsers ask for the favicon), then the real redirect
            for line in ["GET /favicon.ico HTTP/1.1", &format!("GET /?state={state}&code=abc%2F123 HTTP/1.1")] {
                let mut s = TcpStream::connect(addr).await.unwrap();
                s.write_all(format!("{line}\r\nHost: x\r\n\r\n").as_bytes()).await.unwrap();
                let mut out = String::new();
                s.read_to_string(&mut out).await.unwrap();
                if line.contains("code=") {
                    assert!(out.starts_with("HTTP/1.1 200 OK"));
                } else {
                    assert!(out.starts_with("HTTP/1.1 404"));
                }
            }
        });
        let code = session.wait_for_code().await.unwrap();
        assert_eq!(code, "abc/123");
        client.await.unwrap();
    }
}
