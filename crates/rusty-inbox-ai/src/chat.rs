//! Streamed chat with a thinking level, one entry point for every provider.
//!
//! Provider dialects (wire facts in `docs/ai/DESIGN.md`):
//! - OpenRouter, Custom: OpenAI Chat Completions (`POST {base}/chat/completions`, SSE)
//! - OpenAI, Codex: Responses API (`POST {base}/responses`, SSE `response.output_text.delta`)
//! - Anthropic: Messages API (`POST {base}/messages`, SSE `content_block_delta`)
//! - Ollama: native `POST {base}/api/chat` (NDJSON)

use crate::http;
use crate::provider::{Connection, ProviderKind};
use crate::{AiError, AiResult, SecretStore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::AtomicBool;

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
    match conn.kind {
        ProviderKind::OpenRouter | ProviderKind::Custom => chat_completions(conn, secrets, req, on_delta, cancel),
        // WORKER A: Responses API (OpenAI with key, Codex with the ChatGPT sign-in), Anthropic
        // Messages, Ollama native /api/chat
        ProviderKind::OpenAI | ProviderKind::Codex | ProviderKind::Anthropic | ProviderKind::Ollama => {
            Err(AiError::NotConfigured(format!("{} is not implemented yet", conn.kind.label())))
        }
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
            return Err(AiError::NotConfigured(format!("Add your {} API key in Settings → AI.", conn.kind.label())))
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
