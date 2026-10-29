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
