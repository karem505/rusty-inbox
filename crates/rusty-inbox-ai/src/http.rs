//! Small blocking HTTP layer over ureq shared by every provider: JSON GET/POST, error mapping
//! (401/403 → Auth, 429 → RateLimited, provider message kept), and line readers for SSE / NDJSON.

use crate::{AiError, AiResult};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

pub const USER_AGENT: &str = concat!("RustyInbox/", env!("CARGO_PKG_VERSION"));

/// One agent per call is fine (calls are seconds apart); HTTP errors come back as responses so the
/// provider's own error message can be shown.
pub fn agent(read_timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(USER_AGENT)
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_response(Some(read_timeout))
        .timeout_recv_body(Some(Duration::from_secs(15 * 60)))
        .build()
        .into()
}

pub type Headers<'a> = &'a [(&'a str, String)];

/// A response whose status was 2xx; the body is still unread.
pub struct Ok200 {
    pub body: Box<dyn Read + Send>,
}

fn map_err(e: ureq::Error) -> AiError {
    AiError::Network(e.to_string())
}

/// The provider's error text out of a JSON error body (`{"error":{"message":..}}`, `{"error":".."}`,
/// `{"message":..}`, `{"detail":..}`), else the raw body (shortened).
pub fn error_message(body: &str) -> String {
    let short = |s: &str| {
        let s = s.trim();
        if s.chars().count() > 300 {
            format!("{}…", s.chars().take(300).collect::<String>())
        } else {
            s.to_string()
        }
    };
    if let Ok(v) = serde_json::from_str::<Value>(body) {
        for p in ["/error/message", "/error", "/message", "/detail", "/error_description"] {
            if let Some(s) = v.pointer(p).and_then(Value::as_str) {
                return short(s);
            }
        }
    }
    short(body)
}

/// Map a failed HTTP status and its message to the [`AiError`] the user sees.
pub fn status_error(status: u16, message: String) -> AiError {
    match status {
        401 | 403 => AiError::Auth(message),
        429 => AiError::RateLimited(message),
        _ => AiError::Http { status, message },
    }
}

/// An error together with the HTTP status that caused it (0 = no response at all).
pub type StatusErr = (u16, AiError);

fn check(resp: ureq::http::Response<ureq::Body>) -> Result<Ok200, StatusErr> {
    let status = resp.status().as_u16();
    let mut body = resp.into_body();
    if (200..300).contains(&status) {
        return Ok(Ok200 { body: Box::new(body.into_reader()) });
    }
    let text = body.read_to_string().unwrap_or_default();
    Err((status, status_error(status, error_message(&text))))
}

fn drop_status((_, e): StatusErr) -> AiError {
    e
}

pub fn get(url: &str, headers: Headers) -> AiResult<Ok200> {
    let mut r = agent(Duration::from_secs(60)).get(url);
    for (k, v) in headers {
        r = r.header(*k, v);
    }
    check(r.call().map_err(map_err)?).map_err(drop_status)
}

pub fn get_json(url: &str, headers: Headers) -> AiResult<Value> {
    read_json(get(url, headers)?)
}

pub fn post_json(url: &str, headers: Headers, body: &Value) -> AiResult<Ok200> {
    post_json_status(url, headers, body).map_err(drop_status)
}

/// Like [`post_json`], but the error carries the HTTP status (callers that react to a specific
/// status: Codex 401 → refresh, device-code 403/404 → pending).
pub fn post_json_status(url: &str, headers: Headers, body: &Value) -> Result<Ok200, StatusErr> {
    let mut r = agent(Duration::from_secs(300)).post(url).header("Content-Type", "application/json");
    for (k, v) in headers {
        r = r.header(*k, v);
    }
    check(r.send(body.to_string()).map_err(|e| (0, map_err(e)))?)
}

pub fn post_form(url: &str, headers: Headers, form: &[(&str, &str)]) -> AiResult<Ok200> {
    let body: String =
        form.iter().map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v))).collect::<Vec<_>>().join("&");
    let mut r = agent(Duration::from_secs(60)).post(url).header("Content-Type", "application/x-www-form-urlencoded");
    for (k, v) in headers {
        r = r.header(*k, v);
    }
    check(r.send(body).map_err(map_err)?).map_err(drop_status)
}

pub fn read_json(mut r: Ok200) -> AiResult<Value> {
    let mut s = String::new();
    r.body.read_to_string(&mut s).map_err(|e| AiError::Network(e.to_string()))?;
    serde_json::from_str(&s).map_err(|e| AiError::Protocol(format!("{e}: {}", error_message(&s))))
}

/// Percent-encode for URLs and form bodies (RFC 3986 unreserved characters stay as they are).
pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Call `on_line` for every line of a streamed body until it ends, `on_line` returns false, or
/// `cancel` is set (then `Err(Cancelled)`). The body is read on a helper thread so a cancel takes
/// effect within ~50 ms even while the server is silent (the thread ends when the connection does).
pub fn for_each_line(r: Ok200, cancel: &AtomicBool, mut on_line: impl FnMut(&str) -> AiResult<bool>) -> AiResult<()> {
    let (tx, rx) = mpsc::channel::<Result<String, String>>();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(r.body);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) => break,
                Ok(_) => {
                    if tx.send(Ok(String::from_utf8_lossy(&buf).into_owned())).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e.to_string()));
                    break;
                }
            }
        }
    });
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AiError::Cancelled);
        }
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(Ok(line)) => {
                if !on_line(line.trim_end_matches(['\r', '\n']))? {
                    return Ok(());
                }
            }
            Ok(Err(e)) => return Err(AiError::Network(e)),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

/// Server-sent events: calls `on_event(event_name, data)` for each `data:` payload (multi-line data
/// joined with `\n`). `[DONE]` ends the stream.
pub fn for_each_sse(
    r: Ok200,
    cancel: &AtomicBool,
    mut on_event: impl FnMut(&str, &str) -> AiResult<()>,
) -> AiResult<()> {
    let mut event = String::new();
    let mut data = String::new();
    let mut done = false;
    for_each_line(r, cancel, |line| {
        if line.is_empty() {
            if !data.is_empty() {
                if data == "[DONE]" {
                    done = true;
                    return Ok(false);
                }
                on_event(&event, &data)?;
            }
            event.clear();
            data.clear();
            return Ok(true);
        }
        if let Some(v) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(v.strip_prefix(' ').unwrap_or(v));
        } else if let Some(v) = line.strip_prefix("event:") {
            event = v.trim().to_string();
        }
        Ok(true)
    })?;
    if !done && !data.is_empty() && data != "[DONE]" {
        on_event(&event, &data)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(s: &'static str) -> Ok200 {
        Ok200 { body: Box::new(std::io::Cursor::new(s.as_bytes())) }
    }

    #[test]
    fn sse_events_are_split_and_done_ends_them() {
        let mut got = vec![];
        let stop = AtomicBool::new(false);
        for_each_sse(
            body("event: a\ndata: {\"x\":1}\n\n: comment\ndata: two\ndata: lines\n\ndata: [DONE]\n\ndata: after\n\n"),
            &stop,
            |e, d| {
                got.push((e.to_string(), d.to_string()));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(got, vec![("a".into(), "{\"x\":1}".into()), ("".into(), "two\nlines".into())]);
    }

    #[test]
    fn provider_error_messages_are_extracted() {
        assert_eq!(error_message(r#"{"error":{"message":"bad key","code":401}}"#), "bad key");
        assert_eq!(error_message(r#"{"error":"weekly limit"}"#), "weekly limit");
        assert_eq!(error_message("plain"), "plain");
        assert_eq!(urlencode("a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
    }
}
