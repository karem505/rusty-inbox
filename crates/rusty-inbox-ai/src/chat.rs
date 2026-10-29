//! Streamed chat with a thinking level, one entry point for every provider.
//!
//! Provider dialects (wire facts in `docs/ai/DESIGN.md`):
//! - OpenRouter, Custom: OpenAI Chat Completions (`POST {base}/chat/completions`, SSE)
//! - OpenAI, Codex: Responses API (`POST {base}/responses`, SSE `response.output_text.delta`)
//! - Anthropic: Messages API (`POST {base}/messages`, SSE `content_block_delta`)
//! - Ollama: native `POST {base}/api/chat` (NDJSON)

use crate::provider::{Connection, ProviderKind};
use crate::{codex, http};
use crate::{AiError, AiResult, SecretStore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn user(s: impl Into<String>) -> Message {
        Message { role: Role::User, content: s.into() }
    }
    pub fn assistant(s: impl Into<String>) -> Message {
        Message { role: Role::Assistant, content: s.into() }
    }
}

/// One request. `thinking` is a level from the model's [`crate::ModelInfo::thinking`] list
/// (`off`, `on`, `minimal`, `low`, `medium`, `high`, `xhigh`, `max`, `ultra`), or `None` for the
/// model's default. `json`: the answer must be one JSON object (providers that support a JSON mode get
/// it switched on; the prompt asks for it as well, and callers parse with
/// [`crate::features::extract_json`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChatRequest {
    pub model: String,
    pub thinking: Option<String>,
    pub system: String,
    pub messages: Vec<Message>,
    pub json: bool,
    /// answer budget (not counting thinking); `None` = provider default / 4096
    pub max_tokens: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChatOutput {
    pub text: String,
    /// the model that answered, as the provider reports it
    pub model: String,
    pub usage: Option<Usage>,
}

/// Run `req` against `conn`, calling `on_delta` with each new piece of answer text as it streams.
/// Thinking text is never passed to `on_delta`. Blocking; run it on a helper thread.
pub fn chat(
    conn: &Connection,
    secrets: &dyn SecretStore,
    req: &ChatRequest,
    on_delta: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> AiResult<ChatOutput> {
    if req.model.trim().is_empty() {
        return Err(AiError::NotConfigured("Choose a model in Settings → AI.".into()));
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(AiError::Cancelled);
    }
    match conn.kind {
        ProviderKind::OpenRouter | ProviderKind::Custom => chat_completions(conn, secrets, req, on_delta, cancel),
        ProviderKind::OpenAI | ProviderKind::Codex => responses(conn, secrets, req, on_delta, cancel),
        ProviderKind::Anthropic => anthropic(conn, secrets, req, on_delta, cancel),
        ProviderKind::Ollama => ollama(conn, secrets, req, on_delta, cancel),
    }
}

/// `Authorization: Bearer <key>` when a key is stored (OpenRouter also gets its attribution headers).
fn bearer_headers(
    conn: &Connection,
    secrets: &dyn SecretStore,
    required: bool,
) -> AiResult<Vec<(&'static str, String)>> {
    let key = secrets.get(&conn.kind.key_secret()).filter(|k| !k.trim().is_empty());
    let mut h = vec![];
    match key {
        Some(k) => h.push(("Authorization", format!("Bearer {}", k.trim()))),
        None if required => {
            let label = conn.kind.label();
            let what = if label.ends_with("key") { label.to_string() } else { format!("{label} API key") };
            return Err(AiError::NotConfigured(format!("Add your {what} in Settings → AI.")));
        }
        None => {}
    }
    if conn.kind == ProviderKind::OpenRouter {
        h.push(("HTTP-Referer", "https://github.com/karem505/rusty-inbox".into()));
        h.push(("X-Title", "Rusty Inbox".into()));
    }
    Ok(h)
}

/// Body of an OpenAI Chat Completions request (OpenRouter's `reasoning` object; `reasoning_effort`
/// for other OpenAI-compatible servers).
pub fn chat_completions_body(kind: ProviderKind, req: &ChatRequest) -> Value {
    let mut messages = vec![];
    if !req.system.is_empty() {
        messages.push(json!({"role": "system", "content": req.system}));
    }
    for m in &req.messages {
        messages.push(json!({"role": m.role, "content": m.content}));
    }
    let mut body = json!({"model": req.model, "messages": messages, "stream": true});
    if let Some(n) = req.max_tokens {
        body["max_tokens"] = json!(n);
    }
    if req.json {
        body["response_format"] = json!({"type": "json_object"});
    }
    match (kind, req.thinking.as_deref()) {
        (_, None | Some("")) => {}
        (ProviderKind::OpenRouter, Some("off")) => body["reasoning"] = json!({"effort": "none", "exclude": true}),
        (ProviderKind::OpenRouter, Some("on")) => body["reasoning"] = json!({"enabled": true, "exclude": true}),
        (ProviderKind::OpenRouter, Some(level)) => body["reasoning"] = json!({"effort": level, "exclude": true}),
        (_, Some("off" | "on")) => {}
        (_, Some(level)) => body["reasoning_effort"] = json!(level),
    }
    body
}

fn chat_completions(
    conn: &Connection,
    secrets: &dyn SecretStore,
    req: &ChatRequest,
    on_delta: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> AiResult<ChatOutput> {
    let base = conn.base();
    if base.is_empty() {
        return Err(AiError::NotConfigured("Enter the server address in Settings → AI.".into()));
    }
    let headers = bearer_headers(conn, secrets, conn.kind == ProviderKind::OpenRouter)?;
    let mut body = chat_completions_body(conn.kind, req);
    body["stream_options"] = json!({"include_usage": true});
    let resp = match http::post_json(&format!("{base}/chat/completions"), &headers, &body) {
        // some servers reject response_format or stream_options: retry once without them
        Err(AiError::Http { status: 400, message })
            if message.contains("response_format") || message.contains("stream_options") =>
        {
            body.as_object_mut().map(|o| {
                o.remove("response_format");
                o.remove("stream_options")
            });
            http::post_json(&format!("{base}/chat/completions"), &headers, &body)?
        }
        r => r?,
    };
    let mut out = ChatOutput { model: req.model.clone(), ..Default::default() };
    http::for_each_sse(resp, cancel, |_, data| {
        let v: Value = serde_json::from_str(data).map_err(|e| AiError::Protocol(e.to_string()))?;
        if let Some(err) = v.get("error") {
            return Err(AiError::Protocol(http::error_message(&json!({"error": err}).to_string())));
        }
        if let Some(m) = v["model"].as_str() {
            out.model = m.to_string();
        }
        if let Some(piece) = v.pointer("/choices/0/delta/content").and_then(Value::as_str) {
            if !piece.is_empty() {
                out.text.push_str(piece);
                on_delta(piece);
            }
        }
        if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
            out.usage = Some(Usage {
                input_tokens: u["prompt_tokens"].as_u64().unwrap_or(0),
                output_tokens: u["completion_tokens"].as_u64().unwrap_or(0),
            });
        }
        Ok(())
    })?;
    if out.text.trim().is_empty() {
        return Err(AiError::Protocol("the model returned no text".into()));
    }
    Ok(out)
}

// ---------------------------------------------------------------- Responses API (OpenAI, Codex)

/// Body of a Responses API request. Codex (ChatGPT backend) always needs `instructions` and does not
/// accept `max_output_tokens`; OpenAI takes both as given.
pub fn responses_body(kind: ProviderKind, req: &ChatRequest) -> Value {
    let input: Vec<Value> = req
        .messages
        .iter()
        .map(|m| {
            let (role, part) = match m.role {
                Role::User => ("user", "input_text"),
                Role::Assistant => ("assistant", "output_text"),
            };
            json!({"type": "message", "role": role, "content": [{"type": part, "text": m.content}]})
        })
        .collect();
    let mut body = json!({"model": req.model, "input": input, "store": false, "stream": true});
    if kind == ProviderKind::Codex {
        let sys = if req.system.trim().is_empty() { "You are a helpful assistant." } else { req.system.as_str() };
        body["instructions"] = json!(sys);
    } else {
        if !req.system.is_empty() {
            body["instructions"] = json!(req.system);
        }
        if let Some(n) = req.max_tokens {
            body["max_output_tokens"] = json!(n);
        }
    }
    // off / on / Default send nothing: the model keeps its own default
    if let Some(level) = req.thinking.as_deref().filter(|l| !matches!(*l, "" | "off" | "on")) {
        body["reasoning"] = json!({"effort": level});
    }
    if req.json {
        body["text"] = json!({"format": {"type": "json_object"}});
    }
    body
}

/// An error reported inside a stream (`error` / `response.failed` / Anthropic `error`) as the matching
/// [`AiError`], keeping the provider's text.
fn stream_error(code: &str, message: &str) -> AiError {
    let c = code.to_ascii_lowercase();
    let message = if message.is_empty() { code.to_string() } else { message.to_string() };
    if c.contains("rate_limit") || c.contains("quota") || c.contains("usage_limit") {
        AiError::RateLimited(message)
    } else if c.contains("authentication") || c.contains("permission") || c.contains("invalid_api_key") {
        AiError::Auth(message)
    } else if c.contains("overloaded") {
        AiError::Http { status: 529, message }
    } else if c == "api_error" || c == "server_error" {
        AiError::Http { status: 500, message }
    } else if c == "invalid_request_error" {
        AiError::Http { status: 400, message }
    } else {
        AiError::Protocol(message)
    }
}

fn responses(
    conn: &Connection,
    secrets: &dyn SecretStore,
    req: &ChatRequest,
    on_delta: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> AiResult<ChatOutput> {
    let base = conn.base();
    if base.is_empty() {
        return Err(AiError::NotConfigured("Enter the server address in Settings → AI.".into()));
    }
    let url = format!("{base}/responses");
    let body = responses_body(conn.kind, req);
    let resp = if conn.kind == ProviderKind::Codex {
        let h = codex::auth_headers(secrets, false)?;
        match http::post_json_status(&url, &h, &body) {
            // the access token was refused: refresh once and retry once
            Err((401, _)) => http::post_json(&url, &codex::auth_headers(secrets, true)?, &body)?,
            r => r.map_err(|(_, e)| e)?,
        }
    } else {
        http::post_json(&url, &bearer_headers(conn, secrets, true)?, &body)?
    };
    let mut out = ChatOutput { model: req.model.clone(), ..Default::default() };
    let mut completed_text = String::new();
    http::for_each_sse(resp, cancel, |event, data| {
        let v: Value = serde_json::from_str(data).map_err(|e| AiError::Protocol(e.to_string()))?;
        let kind = v["type"].as_str().filter(|t| !t.is_empty()).unwrap_or(event);
        match kind {
            "response.output_text.delta" => {
                if let Some(piece) = v["delta"].as_str().filter(|p| !p.is_empty()) {
                    out.text.push_str(piece);
                    on_delta(piece);
                }
            }
            "response.completed" | "response.incomplete" => {
                let r = &v["response"];
                if let Some(m) = r["model"].as_str() {
                    out.model = m.to_string();
                }
                if r["usage"].is_object() {
                    out.usage = Some(Usage {
                        input_tokens: r["usage"]["input_tokens"].as_u64().unwrap_or(0),
                        output_tokens: r["usage"]["output_tokens"].as_u64().unwrap_or(0),
                    });
                }
                for item in r["output"].as_array().into_iter().flatten() {
                    for part in item["content"].as_array().into_iter().flatten() {
                        if part["type"] == "output_text" {
                            completed_text.push_str(part["text"].as_str().unwrap_or_default());
                        }
                    }
                }
            }
            "response.failed" => {
                let e = &v["response"]["error"];
                return Err(stream_error(
                    e["code"].as_str().unwrap_or_default(),
                    e["message"].as_str().unwrap_or("the request failed"),
                ));
            }
            "error" => {
                let e = if v["error"].is_object() { &v["error"] } else { &v };
                return Err(stream_error(
                    e["code"].as_str().or_else(|| e["type"].as_str()).unwrap_or_default(),
                    e["message"].as_str().unwrap_or("the provider reported an error"),
                ));
            }
            _ => {}
        }
        Ok(())
    })?;
    if out.text.trim().is_empty() && !completed_text.trim().is_empty() {
        // a provider that sent the answer only in the final event
        on_delta(&completed_text);
        out.text = completed_text;
    }
    if out.text.trim().is_empty() {
        return Err(AiError::Protocol("the model returned no text".into()));
    }
    Ok(out)
}

// ---------------------------------------------------------------- Anthropic Messages

const ANTHROPIC_VERSION: &str = "2023-06-01";
/// answer budget when the caller gives none
const DEFAULT_ANSWER_TOKENS: u32 = 4096;

/// Thinking budget (tokens) for a level; `None` = no extended thinking.
pub fn anthropic_budget(level: &str) -> Option<u32> {
    match level {
        "minimal" => Some(1024),
        "low" => Some(2048),
        "medium" => Some(8192),
        "high" => Some(24576),
        "xhigh" => Some(32768),
        "max" | "ultra" => Some(49152),
        _ => None,
    }
}

/// Body of a Messages request. `adaptive`: the fallback for models that reject budgets
/// (`thinking: {type: adaptive}` + `output_config.effort`).
pub fn anthropic_body(req: &ChatRequest, adaptive: bool) -> Value {
    let answer = req.max_tokens.unwrap_or(DEFAULT_ANSWER_TOKENS);
    let mut system = req.system.clone();
    if req.json {
        // no JSON mode on this API: the instruction does it
        if !system.is_empty() {
            system.push_str("\n\n");
        }
        system.push_str(
            "Respond with a single valid JSON object only, with no text before or after it and no code fences.",
        );
    }
    let messages: Vec<Value> = req.messages.iter().map(|m| json!({"role": m.role, "content": m.content})).collect();
    let mut body = json!({"model": req.model, "max_tokens": answer, "messages": messages, "stream": true});
    if !system.is_empty() {
        body["system"] = json!(system);
    }
    if let Some(level) = req.thinking.as_deref() {
        if let Some(budget) = anthropic_budget(level) {
            if adaptive {
                body["thinking"] = json!({"type": "adaptive"});
                body["output_config"] = json!({"effort": if level == "minimal" { "low" } else { level }});
                body["max_tokens"] = json!(answer + budget);
            } else {
                body["thinking"] = json!({"type": "enabled", "budget_tokens": budget});
                body["max_tokens"] = json!(answer + budget);
            }
        }
    }
    body
}

fn anthropic(
    conn: &Connection,
    secrets: &dyn SecretStore,
    req: &ChatRequest,
    on_delta: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> AiResult<ChatOutput> {
    let base = conn.base();
    let key = secrets.get(&conn.kind.key_secret()).filter(|k| !k.trim().is_empty());
    let Some(key) = key else {
        return Err(AiError::NotConfigured("Add your Anthropic API key in Settings → AI.".into()));
    };
    let headers = [("x-api-key", key.trim().to_string()), ("anthropic-version", ANTHROPIC_VERSION.to_string())];
    let url = format!("{base}/messages");
    let resp = match http::post_json(&url, &headers, &anthropic_body(req, false)) {
        // newer models take `adaptive` thinking instead of a token budget
        Err(AiError::Http { status: 400, message }) if message.to_ascii_lowercase().contains("adaptive") => {
            http::post_json(&url, &headers, &anthropic_body(req, true))?
        }
        r => r?,
    };
    let mut out = ChatOutput { model: req.model.clone(), ..Default::default() };
    let mut usage = Usage::default();
    let mut have_usage = false;
    http::for_each_sse(resp, cancel, |event, data| {
        let v: Value = serde_json::from_str(data).map_err(|e| AiError::Protocol(e.to_string()))?;
        match v["type"].as_str().filter(|t| !t.is_empty()).unwrap_or(event) {
            "message_start" => {
                if let Some(m) = v.pointer("/message/model").and_then(Value::as_str) {
                    out.model = m.to_string();
                }
                if let Some(n) = v.pointer("/message/usage/input_tokens").and_then(Value::as_u64) {
                    usage.input_tokens = n;
                    have_usage = true;
                }
            }
            "content_block_delta" => {
                // `thinking_delta` / `signature_delta` are not shown
                if v["delta"]["type"] == "text_delta" {
                    if let Some(piece) = v["delta"]["text"].as_str().filter(|p| !p.is_empty()) {
                        out.text.push_str(piece);
                        on_delta(piece);
                    }
                }
            }
            "message_delta" => {
                if let Some(n) = v.pointer("/usage/output_tokens").and_then(Value::as_u64) {
                    usage.output_tokens = n;
                    have_usage = true;
                }
                if let Some(n) = v.pointer("/usage/input_tokens").and_then(Value::as_u64) {
                    usage.input_tokens = n;
                }
            }
            "error" => {
                let e = &v["error"];
                return Err(stream_error(
                    e["type"].as_str().unwrap_or_default(),
                    e["message"].as_str().unwrap_or("the provider reported an error"),
                ));
            }
            _ => {}
        }
        Ok(())
    })?;
    if have_usage {
        out.usage = Some(usage);
    }
    if out.text.trim().is_empty() {
        return Err(AiError::Protocol("the model returned no text".into()));
    }
    Ok(out)
}

// ---------------------------------------------------------------- Ollama native

/// The `think` value for Ollama: `false`/`true`, or "low"/"medium"/"high" for gpt-oss models (which
/// cannot be switched off). `None` = send nothing.
pub fn ollama_think(model: &str, level: Option<&str>) -> Option<Value> {
    let level = level.filter(|l| !l.is_empty())?;
    let gpt_oss = model.to_ascii_lowercase().contains("gpt-oss");
    Some(match (level, gpt_oss) {
        ("off", true) => return None,
        ("off", false) => json!(false),
        (_, false) => json!(true),
        ("on", true) => return None,
        ("minimal" | "low", true) => json!("low"),
        ("medium", true) => json!("medium"),
        (_, true) => json!("high"),
    })
}

pub fn ollama_body(req: &ChatRequest) -> Value {
    let mut messages = vec![];
    if !req.system.is_empty() {
        messages.push(json!({"role": "system", "content": req.system}));
    }
    for m in &req.messages {
        messages.push(json!({"role": m.role, "content": m.content}));
    }
    let mut body = json!({"model": req.model, "messages": messages, "stream": true});
    if let Some(t) = ollama_think(&req.model, req.thinking.as_deref()) {
        body["think"] = t;
    }
    if req.json {
        body["format"] = json!("json");
    }
    if let Some(n) = req.max_tokens {
        body["options"] = json!({"num_predict": n});
    }
    body
}

fn ollama(
    conn: &Connection,
    secrets: &dyn SecretStore,
    req: &ChatRequest,
    on_delta: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> AiResult<ChatOutput> {
    let base = conn.base();
    let headers = bearer_headers(conn, secrets, false)?;
    let resp = http::post_json(&format!("{base}/api/chat"), &headers, &ollama_body(req))?;
    let mut out = ChatOutput { model: req.model.clone(), ..Default::default() };
    http::for_each_line(resp, cancel, |line| {
        if line.trim().is_empty() {
            return Ok(true);
        }
        let v: Value = serde_json::from_str(line).map_err(|e| AiError::Protocol(format!("{e}: {line}")))?;
        if let Some(e) = v.get("error") {
            let msg = e.as_str().map(str::to_string).unwrap_or_else(|| e.to_string());
            return Err(AiError::Protocol(msg));
        }
        if let Some(m) = v["model"].as_str() {
            out.model = m.to_string();
        }
        // `message.thinking` is not shown
        if let Some(piece) = v.pointer("/message/content").and_then(Value::as_str).filter(|p| !p.is_empty()) {
            out.text.push_str(piece);
            on_delta(piece);
        }
        if v["done"] == true {
            if v["prompt_eval_count"].is_u64() || v["eval_count"].is_u64() {
                out.usage = Some(Usage {
                    input_tokens: v["prompt_eval_count"].as_u64().unwrap_or(0),
                    output_tokens: v["eval_count"].as_u64().unwrap_or(0),
                });
            }
            return Ok(false);
        }
        Ok(true)
    })?;
    if out.text.trim().is_empty() {
        return Err(AiError::Protocol("the model returned no text".into()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_completions_bodies_carry_the_thinking_level_per_provider() {
        let req = ChatRequest {
            model: "m".into(),
            thinking: Some("high".into()),
            system: "sys".into(),
            messages: vec![Message::user("hi")],
            json: true,
            max_tokens: Some(100),
        };
        let b = chat_completions_body(ProviderKind::OpenRouter, &req);
        assert_eq!(b["reasoning"]["effort"], "high");
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][1]["content"], "hi");
        assert_eq!(b["response_format"]["type"], "json_object");
        let c = chat_completions_body(ProviderKind::Custom, &req);
        assert_eq!(c["reasoning_effort"], "high");
        assert!(c.get("reasoning").is_none());
        let off = ChatRequest { thinking: Some("off".into()), ..req };
        assert_eq!(chat_completions_body(ProviderKind::OpenRouter, &off)["reasoning"]["effort"], "none");
        assert!(chat_completions_body(ProviderKind::Custom, &off).get("reasoning_effort").is_none());
    }
}

#[cfg(test)]
mod wire_tests {
    use super::*;
    use crate::codex::tests::{jwt, now, signed_in, with_issuer};
    use crate::testutil::{Mock, Resp};
    use crate::MemorySecrets;
    use std::time::{Duration, Instant};

    fn req(model: &str, thinking: Option<&str>) -> ChatRequest {
        ChatRequest {
            model: model.into(),
            thinking: thinking.map(str::to_string),
            system: "Be brief.".into(),
            messages: vec![Message::user("Hello"), Message::assistant("Hi"), Message::user("Say pong")],
            ..Default::default()
        }
    }

    fn run(
        kind: ProviderKind,
        url: &str,
        secrets: &MemorySecrets,
        r: &ChatRequest,
    ) -> (AiResult<ChatOutput>, Vec<String>) {
        let mut deltas = vec![];
        let out = chat(
            &Connection::with_base_url(kind, url),
            secrets,
            r,
            &mut |d| deltas.push(d.to_string()),
            &AtomicBool::new(false),
        );
        (out, deltas)
    }

    fn keyed(kind: ProviderKind, key: &str) -> MemorySecrets {
        let s = MemorySecrets::default();
        s.set(&kind.key_secret(), key);
        s
    }

    fn responses_stream(text: &[&str]) -> Resp {
        let mut ev: Vec<(String, String)> = vec![
            (
                "response.created".into(),
                json!({"type": "response.created", "response": {"model": "m-real"}}).to_string(),
            ),
            (
                "response.reasoning_summary_text.delta".into(),
                json!({"type": "response.reasoning_summary_text.delta", "delta": "SECRET THOUGHT"}).to_string(),
            ),
        ];
        for t in text {
            ev.push((
                "response.output_text.delta".into(),
                json!({"type": "response.output_text.delta", "delta": t}).to_string(),
            ));
        }
        ev.push((
            "response.completed".into(),
            json!({"type": "response.completed", "response": {"model": "m-real", "usage": {"input_tokens": 11, "output_tokens": 3}}}).to_string(),
        ));
        let pairs: Vec<(&str, &str)> = ev.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        Resp::sse(&pairs)
    }

    #[test]
    fn openai_uses_the_responses_api_with_bearer_key_and_reasoning() {
        let mock = Mock::scripted(vec![responses_stream(&["po", "ng"])]);
        let (out, deltas) = run(
            ProviderKind::OpenAI,
            &mock.url,
            &keyed(ProviderKind::OpenAI, "sk-test"),
            &ChatRequest { json: true, max_tokens: Some(77), ..req("gpt-5", Some("high")) },
        );
        let out = out.unwrap();
        assert_eq!(out.text, "pong");
        assert_eq!(deltas, vec!["po", "ng"], "thinking text must not reach on_delta");
        assert_eq!(out.model, "m-real");
        assert_eq!(out.usage, Some(Usage { input_tokens: 11, output_tokens: 3 }));
        let r = &mock.requests()[0];
        assert_eq!((r.method.as_str(), r.path.as_str()), ("POST", "/responses"));
        assert_eq!(r.header("authorization"), Some("Bearer sk-test"));
        let b = r.json();
        assert_eq!(b["model"], "gpt-5");
        assert_eq!(b["instructions"], "Be brief.");
        assert_eq!(b["store"], false);
        assert_eq!(b["stream"], true);
        assert_eq!(b["reasoning"], json!({"effort": "high"}));
        assert_eq!(b["text"]["format"]["type"], "json_object");
        assert_eq!(b["max_output_tokens"], 77);
        assert_eq!(
            b["input"][0],
            json!({"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Hello"}]})
        );
        assert_eq!(
            b["input"][1],
            json!({"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Hi"}]})
        );
        assert_eq!(b["input"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn responses_bodies_map_thinking_levels() {
        for (lvl, want) in [
            (None, None),
            (Some(""), None),
            (Some("off"), None),
            (Some("on"), None),
            (Some("low"), Some("low")),
            (Some("ultra"), Some("ultra")),
        ] {
            let b = responses_body(ProviderKind::OpenAI, &req("m", lvl));
            assert_eq!(
                b.get("reasoning").map(|r| r["effort"].as_str().unwrap().to_string()),
                want.map(str::to_string),
                "{lvl:?}"
            );
            assert!(b.get("text").is_none());
        }
        // Codex: instructions always present, never max_output_tokens
        let c = responses_body(
            ProviderKind::Codex,
            &ChatRequest { system: String::new(), max_tokens: Some(5), ..req("m", None) },
        );
        assert!(c["instructions"].as_str().unwrap().len() > 5);
        assert!(c.get("max_output_tokens").is_none());
    }

    #[test]
    fn missing_openai_key_is_a_clear_not_configured_error() {
        let (out, _) = run(ProviderKind::OpenAI, "http://127.0.0.1:9", &MemorySecrets::default(), &req("gpt-5", None));
        assert!(matches!(out, Err(AiError::NotConfigured(m)) if m == "Add your OpenAI API key in Settings → AI."));
        let (out, _) = run(ProviderKind::Anthropic, "http://127.0.0.1:9", &MemorySecrets::default(), &req("m", None));
        assert!(matches!(out, Err(AiError::NotConfigured(_))));
        let (out, _) = run(ProviderKind::Codex, "http://127.0.0.1:9", &MemorySecrets::default(), &req("m", None));
        assert!(matches!(out, Err(AiError::NotConfigured(_))));
        let (out, _) = run(ProviderKind::OpenAI, "http://127.0.0.1:9", &MemorySecrets::default(), &req("", None));
        assert!(matches!(out, Err(AiError::NotConfigured(_))));
    }

    #[test]
    fn codex_sends_the_account_headers_and_no_output_token_cap() {
        let mock = Mock::scripted(vec![responses_stream(&["pong"])]);
        let s = MemorySecrets::default();
        signed_in(&s, "tok-A", "ref", now() + 3600);
        let (out, _) =
            run(ProviderKind::Codex, &mock.url, &s, &ChatRequest { max_tokens: Some(50), ..req("gpt-6", Some("low")) });
        assert_eq!(out.unwrap().text, "pong");
        let r = &mock.requests()[0];
        assert_eq!(r.path, "/responses");
        assert_eq!(r.header("authorization"), Some("Bearer tok-A"));
        assert_eq!(r.header("chatgpt-account-id"), Some("acct-1"));
        assert_eq!(r.header("originator"), Some("rusty-inbox"));
        assert_eq!(r.header("x-openai-internal-codex-residency"), Some("eu"));
        assert!(r.header("user-agent").unwrap().starts_with("RustyInbox/"));
        let b = r.json();
        assert_eq!(b["reasoning"]["effort"], "low");
        assert_eq!(b["instructions"], "Be brief.");
        assert!(b.get("max_output_tokens").is_none());
    }

    #[test]
    fn codex_401_refreshes_once_and_retries_with_the_new_token() {
        let new_access = jwt(now() + 7200, "acct-1", "plus", "eu", "me@example.com");
        let na = new_access.clone();
        let mock = Mock::start(move |r, _| match r.path.as_str() {
            "/oauth/token" => {
                Resp::json(200, &json!({"access_token": na, "refresh_token": "ref-2", "expires_in": 3600}))
            }
            "/responses" if r.header("authorization") == Some("Bearer stale") => {
                Resp::json(401, &json!({"error": {"message": "token expired"}}))
            }
            "/responses" => responses_stream(&["pong"]),
            p => panic!("{p}"),
        });
        let s = MemorySecrets::default();
        signed_in(&s, "stale", "ref-1", now() + 3600); // not expiring by the clock: only the 401 reveals it
        let (out, _) = with_issuer(&mock.url, || run(ProviderKind::Codex, &mock.url, &s, &req("gpt-6", None)));
        assert_eq!(out.unwrap().text, "pong");
        let paths: Vec<String> = mock.requests().iter().map(|r| r.path.clone()).collect();
        assert_eq!(paths, ["/responses", "/oauth/token", "/responses"]);
        assert_eq!(mock.requests()[1].form()["refresh_token"], "ref-1");
        assert_eq!(mock.requests()[2].header("authorization"), Some(format!("Bearer {new_access}").as_str()));
        assert_eq!(crate::codex::load_tokens(&s).unwrap().refresh_token, "ref-2");
    }

    #[test]
    fn codex_gives_up_after_one_retry() {
        let new_access = jwt(now() + 7200, "acct-1", "plus", "", "x@y.z");
        let mock = Mock::start(move |r, _| match r.path.as_str() {
            "/oauth/token" => Resp::json(200, &json!({"access_token": new_access, "refresh_token": "ref-2"})),
            _ => Resp::json(401, &json!({"error": {"message": "still not allowed"}})),
        });
        let s = MemorySecrets::default();
        signed_in(&s, "stale2", "ref-1", now() + 3600);
        let (out, _) = with_issuer(&mock.url, || run(ProviderKind::Codex, &mock.url, &s, &req("m", None)));
        assert!(matches!(out, Err(AiError::Auth(m)) if m == "still not allowed"));
        assert_eq!(mock.requests().len(), 3); // request, refresh, request: no loop
    }

    #[test]
    fn http_errors_keep_the_providers_own_message() {
        let cases: [(ProviderKind, u16, Value); 8] = [
            (
                ProviderKind::OpenAI,
                401,
                json!({"error": {"message": "Incorrect API key provided: sk-...abcd", "type": "invalid_request_error"}}),
            ),
            (ProviderKind::OpenAI, 429, json!({"error": {"message": "You exceeded your current quota"}})),
            (ProviderKind::OpenAI, 500, json!({"error": {"message": "The server had an error"}})),
            (
                ProviderKind::Anthropic,
                403,
                json!({"type": "error", "error": {"type": "permission_error", "message": "not allowed"}}),
            ),
            (
                ProviderKind::Anthropic,
                429,
                json!({"type": "error", "error": {"type": "rate_limit_error", "message": "slow down"}}),
            ),
            (
                ProviderKind::Anthropic,
                529,
                json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}}),
            ),
            (ProviderKind::Ollama, 429, json!({"error": "you have reached your weekly usage limit, upgrade for more"})),
            (ProviderKind::Ollama, 404, json!({"error": "model 'x' not found"})),
        ];
        for (kind, status, body) in cases {
            let mock = Mock::scripted(vec![Resp::json(status, &body)]);
            let (out, deltas) = run(kind, &mock.url, &keyed(kind, "k"), &req("x", None));
            let e = out.unwrap_err();
            assert!(deltas.is_empty());
            let want = http::error_message(&body.to_string());
            match (status, &e) {
                (401 | 403, AiError::Auth(m)) | (429, AiError::RateLimited(m)) => {
                    assert_eq!(m, &want, "{kind:?} {status}")
                }
                (s, AiError::Http { status, message }) if *status == s => {
                    assert_eq!(message, &want, "{kind:?} {status}")
                }
                _ => panic!("{kind:?} {status}: {e:?}"),
            }
            assert!(e.to_string().contains(&want), "{e}");
        }
    }

    #[test]
    fn responses_stream_errors_become_errors_and_empty_answers_are_protocol_errors() {
        let failed = Resp::sse(&[("response.failed", &json!({"type": "response.failed", "response": {"error": {"code": "server_error", "message": "model crashed"}}}).to_string())]);
        let (out, _) = run(
            ProviderKind::OpenAI,
            &Mock::scripted(vec![failed]).url,
            &keyed(ProviderKind::OpenAI, "k"),
            &req("m", None),
        );
        assert!(matches!(out, Err(AiError::Http { status: 500, message }) if message == "model crashed"));
        let err = Resp::sse(&[(
            "error",
            &json!({"type": "error", "code": "rate_limit_exceeded", "message": "Too many"}).to_string(),
        )]);
        let (out, _) = run(
            ProviderKind::Codex,
            &Mock::scripted(vec![err]).url,
            &{
                let s = MemorySecrets::default();
                signed_in(&s, "t", "r", now() + 3600);
                s
            },
            &req("m", None),
        );
        assert!(matches!(out, Err(AiError::RateLimited(m)) if m == "Too many"));
        let empty =
            Resp::sse(&[("response.completed", &json!({"type": "response.completed", "response": {}}).to_string())]);
        let (out, _) = run(
            ProviderKind::OpenAI,
            &Mock::scripted(vec![empty]).url,
            &keyed(ProviderKind::OpenAI, "k"),
            &req("m", None),
        );
        assert!(matches!(out, Err(AiError::Protocol(_))));
        // the answer only in the final event is still delivered
        let only_final = Resp::sse(&[("response.completed", &json!({"type": "response.completed", "response": {"output": [{"content": [{"type": "output_text", "text": "late"}]}]}}).to_string())]);
        let (out, deltas) = run(
            ProviderKind::OpenAI,
            &Mock::scripted(vec![only_final]).url,
            &keyed(ProviderKind::OpenAI, "k"),
            &req("m", None),
        );
        assert_eq!(out.unwrap().text, "late");
        assert_eq!(deltas, vec!["late"]);
    }

    fn anthropic_stream() -> Resp {
        Resp::sse(&[
            ("message_start", &json!({"type": "message_start", "message": {"model": "claude-x", "usage": {"input_tokens": 25, "output_tokens": 1}}}).to_string()),
            ("content_block_start", &json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking"}}).to_string()),
            ("content_block_delta", &json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "SECRET THOUGHT"}}).to_string()),
            ("content_block_delta", &json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "abc"}}).to_string()),
            ("content_block_delta", &json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "po"}}).to_string()),
            ("ping", &json!({"type": "ping"}).to_string()),
            ("content_block_delta", &json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "ng"}}).to_string()),
            ("message_delta", &json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 9}}).to_string()),
            ("message_stop", &json!({"type": "message_stop"}).to_string()),
        ])
    }

    #[test]
    fn anthropic_headers_thinking_budget_and_stream() {
        for (level, budget) in [("low", 2048u32), ("medium", 8192), ("high", 24576)] {
            let mock = Mock::scripted(vec![anthropic_stream()]);
            let (out, deltas) = run(
                ProviderKind::Anthropic,
                &mock.url,
                &keyed(ProviderKind::Anthropic, "sk-ant-test"),
                &ChatRequest { max_tokens: Some(1000), ..req("claude-x", Some(level)) },
            );
            let out = out.unwrap();
            assert_eq!((out.text.as_str(), deltas), ("pong", vec!["po".to_string(), "ng".to_string()]));
            assert_eq!(out.model, "claude-x");
            assert_eq!(out.usage, Some(Usage { input_tokens: 25, output_tokens: 9 }));
            let r = &mock.requests()[0];
            assert_eq!(r.path, "/messages");
            assert_eq!(r.header("x-api-key"), Some("sk-ant-test"));
            assert_eq!(r.header("anthropic-version"), Some("2023-06-01"));
            assert!(r.header("authorization").is_none());
            let b = r.json();
            assert_eq!(b["thinking"], json!({"type": "enabled", "budget_tokens": budget}));
            assert_eq!(b["max_tokens"], budget + 1000);
            assert_eq!(b["system"], "Be brief.");
            assert_eq!(b["stream"], true);
            assert_eq!(b["messages"].as_array().unwrap().len(), 3);
            assert_eq!(b["messages"][1], json!({"role": "assistant", "content": "Hi"}));
        }
    }

    #[test]
    fn anthropic_without_thinking_and_json_mode() {
        let mock = Mock::scripted(vec![anthropic_stream(), anthropic_stream()]);
        let s = keyed(ProviderKind::Anthropic, "k");
        for lvl in [None, Some("off")] {
            let (out, _) =
                run(ProviderKind::Anthropic, &mock.url, &s, &ChatRequest { json: true, ..req("claude-x", lvl) });
            out.unwrap();
        }
        for r in mock.requests() {
            let b = r.json();
            assert!(b.get("thinking").is_none());
            assert_eq!(b["max_tokens"], 4096);
            assert!(b["system"].as_str().unwrap().starts_with("Be brief."));
            assert!(b["system"].as_str().unwrap().contains("single valid JSON object"));
        }
    }

    #[test]
    fn anthropic_falls_back_to_adaptive_thinking_when_budgets_are_rejected() {
        let mock = Mock::scripted(vec![
            Resp::json(
                400,
                &json!({"type": "error", "error": {"type": "invalid_request_error", "message": "\"thinking.type.enabled\" is not supported for this model. Use \"thinking.type.adaptive\" and \"output_config.effort\"."}}),
            ),
            anthropic_stream(),
        ]);
        let (out, _) = run(
            ProviderKind::Anthropic,
            &mock.url,
            &keyed(ProviderKind::Anthropic, "k"),
            &ChatRequest { max_tokens: Some(1000), ..req("claude-new", Some("medium")) },
        );
        assert_eq!(out.unwrap().text, "pong");
        let reqs = mock.requests();
        assert_eq!(reqs.len(), 2);
        assert_eq!(reqs[0].json()["thinking"]["type"], "enabled");
        let b = reqs[1].json();
        assert_eq!(b["thinking"], json!({"type": "adaptive"}));
        assert_eq!(b["output_config"], json!({"effort": "medium"}));
        assert_eq!(b["max_tokens"], 1000 + 8192);
        // an unrelated 400 is not retried
        let mock = Mock::scripted(vec![Resp::json(400, &json!({"error": {"message": "max_tokens too large"}}))]);
        let (out, _) = run(
            ProviderKind::Anthropic,
            &mock.url,
            &keyed(ProviderKind::Anthropic, "k"),
            &req("claude-new", Some("high")),
        );
        assert!(matches!(out, Err(AiError::Http { status: 400, .. })));
        assert_eq!(mock.requests().len(), 1);
    }

    #[test]
    fn anthropic_error_events_and_empty_answers() {
        let ev = Resp::sse(&[
            (
                "content_block_delta",
                &json!({"type": "content_block_delta", "delta": {"type": "text_delta", "text": "par"}}).to_string(),
            ),
            (
                "error",
                &json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}}).to_string(),
            ),
        ]);
        let (out, deltas) = run(
            ProviderKind::Anthropic,
            &Mock::scripted(vec![ev]).url,
            &keyed(ProviderKind::Anthropic, "k"),
            &req("m", None),
        );
        assert!(matches!(out, Err(AiError::Http { status: 529, message }) if message == "Overloaded"));
        assert_eq!(deltas, vec!["par"]);
        let only_thinking = Resp::sse(&[(
            "content_block_delta",
            &json!({"type": "content_block_delta", "delta": {"type": "thinking_delta", "thinking": "hmm"}}).to_string(),
        )]);
        let (out, deltas) = run(
            ProviderKind::Anthropic,
            &Mock::scripted(vec![only_thinking]).url,
            &keyed(ProviderKind::Anthropic, "k"),
            &req("m", Some("low")),
        );
        assert!(matches!(out, Err(AiError::Protocol(_))));
        assert!(deltas.is_empty());
    }

    #[test]
    fn ollama_think_mapping() {
        use serde_json::json as j;
        assert_eq!(ollama_think("llama3", None), None);
        assert_eq!(ollama_think("llama3", Some("")), None);
        assert_eq!(ollama_think("qwen3:8b", Some("off")), Some(j!(false)));
        assert_eq!(ollama_think("qwen3:8b", Some("on")), Some(j!(true)));
        assert_eq!(ollama_think("qwen3:8b", Some("high")), Some(j!(true)));
        assert_eq!(ollama_think("gpt-oss:20b", Some("low")), Some(j!("low")));
        assert_eq!(ollama_think("gpt-oss:120b-cloud", Some("medium")), Some(j!("medium")));
        assert_eq!(ollama_think("gpt-oss:20b", Some("high")), Some(j!("high")));
        assert_eq!(ollama_think("gpt-oss:20b", Some("off")), None);
    }

    #[test]
    fn ollama_streams_ndjson_and_ignores_thinking() {
        let mock = Mock::scripted(vec![Resp::ndjson(&[
            json!({"model": "qwen3", "message": {"role": "assistant", "content": "", "thinking": "SECRET"}, "done": false}),
            json!({"model": "qwen3", "message": {"role": "assistant", "content": "po"}, "done": false}),
            json!({"model": "qwen3", "message": {"role": "assistant", "content": "ng"}, "done": false}),
            json!({"model": "qwen3", "message": {"role": "assistant", "content": ""}, "done": true, "prompt_eval_count": 14, "eval_count": 5}),
        ])]);
        let (out, deltas) = run(
            ProviderKind::Ollama,
            &mock.url,
            &MemorySecrets::default(),
            &ChatRequest { json: true, max_tokens: Some(64), ..req("qwen3", Some("off")) },
        );
        let out = out.unwrap();
        assert_eq!((out.text.as_str(), deltas), ("pong", vec!["po".to_string(), "ng".to_string()]));
        assert_eq!(out.usage, Some(Usage { input_tokens: 14, output_tokens: 5 }));
        let r = &mock.requests()[0];
        assert_eq!(r.path, "/api/chat");
        assert!(r.header("authorization").is_none(), "no key, no Authorization");
        let b = r.json();
        assert_eq!(b["think"], false);
        assert_eq!(b["format"], "json");
        assert_eq!(b["stream"], true);
        assert_eq!(b["options"]["num_predict"], 64);
        assert_eq!(b["messages"][0], json!({"role": "system", "content": "Be brief."}));
        assert_eq!(b["messages"].as_array().unwrap().len(), 4);
    }

    #[test]
    fn ollama_cloud_sends_the_bearer_key_and_shows_the_weekly_limit_message() {
        let msg = "you (karem) have reached your weekly usage limit, upgrade for higher limits";
        let mock = Mock::scripted(vec![Resp::json(429, &json!({"error": msg}))]);
        let (out, _) = run(
            ProviderKind::Ollama,
            &mock.url,
            &keyed(ProviderKind::Ollama, "ollama-key"),
            &req("gpt-oss:120b", Some("high")),
        );
        let e = out.unwrap_err();
        assert!(matches!(&e, AiError::RateLimited(m) if m == msg), "{e:?}");
        assert!(e.to_string().contains(msg));
        let r = &mock.requests()[0];
        assert_eq!(r.header("authorization"), Some("Bearer ollama-key"));
        assert_eq!(r.json()["think"], "high");
    }

    #[test]
    fn ollama_error_lines_and_empty_answers() {
        let (out, _) = run(
            ProviderKind::Ollama,
            &Mock::scripted(vec![Resp::ndjson(&[json!({"error": "model runner stopped"})])]).url,
            &MemorySecrets::default(),
            &req("m", None),
        );
        assert!(matches!(out, Err(AiError::Protocol(m)) if m == "model runner stopped"));
        let (out, _) = run(
            ProviderKind::Ollama,
            &Mock::scripted(vec![Resp::ndjson(&[json!({"message": {"content": ""}, "done": true})])]).url,
            &MemorySecrets::default(),
            &req("m", None),
        );
        assert!(matches!(out, Err(AiError::Protocol(_))));
    }

    #[test]
    fn cancel_stops_a_stalled_stream_promptly_for_every_dialect() {
        let sse = |first: &str| Resp::text(200, "text/event-stream", first).held(Duration::from_secs(30));
        let cases: Vec<(ProviderKind, Resp)> = vec![
            (
                ProviderKind::OpenAI,
                sse(&format!("data: {}\n\n", json!({"type": "response.output_text.delta", "delta": "x"}))),
            ),
            (
                ProviderKind::Anthropic,
                sse(&format!(
                    "data: {}\n\n",
                    json!({"type": "content_block_delta", "delta": {"type": "text_delta", "text": "x"}})
                )),
            ),
            (
                ProviderKind::Ollama,
                Resp::text(
                    200,
                    "application/x-ndjson",
                    &format!("{}\n", json!({"message": {"content": "x"}, "done": false})),
                )
                .held(Duration::from_secs(30)),
            ),
            (
                ProviderKind::OpenRouter,
                sse(&format!("data: {}\n\n", json!({"choices": [{"delta": {"content": "x"}}]}))),
            ),
        ];
        for (kind, resp) in cases {
            let mock = Mock::scripted(vec![resp]);
            let cancel = std::sync::Arc::new(AtomicBool::new(false));
            let c2 = cancel.clone();
            let seen = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let s2 = seen.clone();
            let s = keyed(kind, "k");
            let conn = Connection::with_base_url(kind, &mock.url);
            let t = Instant::now();
            let th = std::thread::spawn(move || {
                // cancel as soon as the first piece arrived
                chat(
                    &conn,
                    &s,
                    &req("m", None),
                    &mut |_| {
                        s2.store(true, Ordering::Relaxed);
                        c2.store(true, Ordering::Relaxed)
                    },
                    &cancel,
                )
            });
            let out = th.join().unwrap();
            assert!(matches!(out, Err(AiError::Cancelled)), "{kind:?}: {out:?}");
            assert!(seen.load(Ordering::Relaxed));
            assert!(t.elapsed() < Duration::from_secs(3), "{kind:?} took {:?}", t.elapsed());
        }
        // already cancelled: no request at all
        let mock = Mock::scripted(vec![]);
        let out = chat(
            &Connection::with_base_url(ProviderKind::OpenAI, &mock.url),
            &keyed(ProviderKind::OpenAI, "k"),
            &req("m", None),
            &mut |_| {},
            &AtomicBool::new(true),
        );
        assert!(matches!(out, Err(AiError::Cancelled)));
        assert!(mock.requests().is_empty());
    }
}
