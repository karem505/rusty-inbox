//! Live model lists, straight from each provider (never a hard-coded catalog).

use crate::provider::{Connection, ProviderKind};
use crate::{codex, http};
use crate::{AiError, AiResult, SecretStore};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One model as the picker shows it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    /// the id sent in requests
    pub id: String,
    /// display name (the id when the provider gives none)
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// context window in tokens, when known
    #[serde(default)]
    pub context: Option<u64>,
    /// thinking levels this model accepts, in increasing order (`off`, `on`, `minimal`, `low`,
    /// `medium`, `high`, `xhigh`, `max`, `ultra`); empty = no thinking control for this model
    #[serde(default)]
    pub thinking: Vec<String>,
    /// the provider's default level, when it says
    #[serde(default)]
    pub default_thinking: Option<String>,
}

/// Every model the connection can use, in the provider's own order (newest/most relevant first
/// where the provider ranks them). Blocking.
pub fn list_models(conn: &Connection, secrets: &dyn SecretStore) -> AiResult<Vec<ModelInfo>> {
    match conn.kind {
        ProviderKind::OpenRouter => openrouter(conn),
        ProviderKind::Custom => openai_compatible(conn, secrets),
        ProviderKind::Codex => codex_models(conn, secrets),
        ProviderKind::OpenAI => openai_models(conn, secrets),
        ProviderKind::Anthropic => anthropic_models(conn, secrets),
        ProviderKind::Ollama => ollama_models(conn, secrets),
    }
}

/// Order thinking levels from lightest to heaviest and drop unknown names.
pub fn order_levels<S: AsRef<str>>(levels: &[S]) -> Vec<String> {
    const ORDER: [&str; 9] = ["off", "on", "minimal", "low", "medium", "high", "xhigh", "max", "ultra"];
    let mut v: Vec<String> = ORDER
        .iter()
        .filter(|l| levels.iter().any(|x| x.as_ref().eq_ignore_ascii_case(l)))
        .map(|l| l.to_string())
        .collect();
    v.dedup();
    v
}

// ---------------------------------------------------------------- ChatGPT (Codex) catalog

/// `GET {base}/models?client_version=99.0.0` with the sign-in headers (the account id header is what
/// makes the list non-empty); `0.0.0` when that answers empty or with an error. One refresh + retry
/// after a 401.
fn codex_models(conn: &Connection, secrets: &dyn SecretStore) -> AiResult<Vec<ModelInfo>> {
    let base = conn.base();
    let fetch = |version: &str| -> AiResult<Vec<ModelInfo>> {
        let url = format!("{base}/models?client_version={version}");
        let v = match http::get_json(&url, &codex::auth_headers(secrets, false)?) {
            Err(AiError::Auth(_)) => http::get_json(&url, &codex::auth_headers(secrets, true)?)?,
            r => r?,
        };
        Ok(parse_codex(&v))
    };
    match fetch("99.0.0") {
        Ok(m) if !m.is_empty() => Ok(m),
        first => match fetch("0.0.0") {
            Ok(m) if !m.is_empty() => Ok(m),
            // both empty: report the first failure if there was one, else the empty list
            second => first.or(second),
        },
    }
}

pub fn parse_codex(v: &Value) -> Vec<ModelInfo> {
    let mut rows: Vec<(i64, ModelInfo)> = vec![];
    for m in v["models"].as_array().into_iter().flatten() {
        let Some(id) = m["slug"].as_str().or_else(|| m["id"].as_str()) else { continue };
        if m["visibility"].as_str() == Some("hide") {
            continue;
        }
        let levels: Vec<String> = m["supported_reasoning_levels"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|l| l["effort"].as_str().or_else(|| l.as_str()))
            .map(|e| if e == "none" { "off" } else { e }.to_string())
            .collect();
        rows.push((
            m["priority"].as_i64().unwrap_or(i64::MAX),
            ModelInfo {
                id: id.to_string(),
                name: m["display_name"].as_str().filter(|n| !n.is_empty()).unwrap_or(id).to_string(),
                description: m["description"].as_str().unwrap_or_default().chars().take(200).collect(),
                context: m["context_window"].as_u64(),
                thinking: order_levels(&levels),
                default_thinking: m["default_reasoning_level"]
                    .as_str()
                    .map(|e| if e == "none" { "off" } else { e }.to_string()),
            },
        ));
    }
    rows.sort_by_key(|r| r.0); // stable: equal priorities keep the server's order
    rows.into_iter().map(|r| r.1).collect()
}

// ---------------------------------------------------------------- OpenAI

fn key_header(conn: &Connection, secrets: &dyn SecretStore) -> AiResult<(&'static str, String)> {
    match secrets.get(&conn.kind.key_secret()).filter(|k| !k.trim().is_empty()) {
        Some(k) => Ok(("Authorization", format!("Bearer {}", k.trim()))),
        None => Err(AiError::NotConfigured(format!("Add your {} API key in Settings → AI.", conn.kind.id()))),
    }
}

fn openai_models(conn: &Connection, secrets: &dyn SecretStore) -> AiResult<Vec<ModelInfo>> {
    let h = [key_header(conn, secrets)
        .map_err(|_| AiError::NotConfigured("Add your OpenAI API key in Settings → AI.".into()))?];
    Ok(parse_openai(&http::get_json(&format!("{}/models", conn.base()), &h)?))
}

/// Ids that are not chat models: embeddings, speech, images, moderation, realtime/audio, search-only,
/// legacy completion models.
fn openai_not_chat(id: &str) -> bool {
    const DENY: [&str; 17] = [
        "embedding",
        "tts",
        "whisper",
        "dall-e",
        "gpt-image",
        "moderation",
        "transcribe",
        "realtime",
        "audio",
        "search",
        "babbage",
        "davinci",
        "instruct",
        "sora",
        "computer-use",
        "deep-research",
        "diarize",
    ];
    DENY.iter().any(|d| id.contains(d))
}

/// Reasoning levels `/v1/models` does not state: by family.
fn openai_levels(id: &str) -> Vec<&'static str> {
    let is_o =
        |p: &str| id.strip_prefix(p).is_some_and(|r| r.is_empty() || !r.starts_with(|c: char| c.is_alphabetic()));
    let o_series =
        ["o1", "o3", "o4"].iter().any(|p| is_o(p)) && !id.starts_with("o1-mini") && !id.starts_with("o1-preview");
    let gpt = (id.starts_with("gpt-5") || id.starts_with("gpt-6")) && !id.contains("chat");
    if o_series {
        vec!["low", "medium", "high"]
    } else if gpt {
        vec!["minimal", "low", "medium", "high"]
    } else {
        vec![]
    }
}

pub fn parse_openai(v: &Value) -> Vec<ModelInfo> {
    let mut rows: Vec<(i64, ModelInfo)> = vec![];
    for m in v["data"].as_array().into_iter().flatten() {
        let Some(id) = m["id"].as_str() else { continue };
        let chat_family = ["gpt-", "o1", "o3", "o4", "chatgpt-"].iter().any(|p| id.starts_with(p));
        if !chat_family || openai_not_chat(id) {
            continue;
        }
        rows.push((
            m["created"].as_i64().unwrap_or(0),
            ModelInfo {
                id: id.to_string(),
                name: id.to_string(),
                thinking: order_levels(&openai_levels(id)),
                ..Default::default()
            },
        ));
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
    rows.into_iter().map(|r| r.1).collect()
}

// ---------------------------------------------------------------- Anthropic

fn anthropic_models(conn: &Connection, secrets: &dyn SecretStore) -> AiResult<Vec<ModelInfo>> {
    let Some(key) = secrets.get(&conn.kind.key_secret()).filter(|k| !k.trim().is_empty()) else {
        return Err(AiError::NotConfigured("Add your Anthropic API key in Settings → AI.".into()));
    };
    let h = [("x-api-key", key.trim().to_string()), ("anthropic-version", "2023-06-01".to_string())];
    Ok(parse_anthropic(&http::get_json(&format!("{}/models?limit=1000", conn.base()), &h)?))
}

/// The API lists newest first; that order is kept. Levels: off/low/medium/high except for the claude-3
/// family (including `*-haiku-3*`).
pub fn parse_anthropic(v: &Value) -> Vec<ModelInfo> {
    let mut out = vec![];
    for m in v["data"].as_array().into_iter().flatten() {
        let Some(id) = m["id"].as_str() else { continue };
        let old = id.starts_with("claude-3")
            || id.starts_with("claude-2")
            || id.starts_with("claude-instant")
            || id.contains("-haiku-3");
        out.push(ModelInfo {
            id: id.to_string(),
            name: m["display_name"].as_str().filter(|n| !n.is_empty()).unwrap_or(id).to_string(),
            context: m["max_input_tokens"].as_u64(),
            thinking: if old { vec![] } else { order_levels(&["off", "low", "medium", "high"]) },
            ..Default::default()
        });
    }
    out
}

// ---------------------------------------------------------------- Ollama

/// Levels Ollama accepts for a thinking model: gpt-oss takes low/medium/high (it cannot be turned
/// off); the others only on/off.
fn ollama_levels(name: &str) -> Vec<String> {
    if name.to_ascii_lowercase().contains("gpt-oss") {
        order_levels(&["low", "medium", "high"])
    } else {
        order_levels(&["off", "on"])
    }
}

fn ollama_models(conn: &Connection, secrets: &dyn SecretStore) -> AiResult<Vec<ModelInfo>> {
    let base = conn.base();
    let headers: Vec<(&str, String)> = secrets
        .get(&conn.kind.key_secret())
        .filter(|k| !k.trim().is_empty())
        .map(|k| vec![("Authorization", format!("Bearer {}", k.trim()))])
        .unwrap_or_default();
    let tags = http::get_json(&format!("{base}/api/tags"), &headers)?;
    let mut models = parse_ollama(&tags);
    // older servers list no capabilities: ask /api/show, for up to 40 models
    let mut asked = 0;
    let mut embedding_only = vec![];
    for (m, known) in models.iter_mut() {
        if *known || asked >= 40 {
            continue;
        }
        asked += 1;
        let body = serde_json::json!({"model": m.id});
        let Ok(show) = http::post_json(&format!("{base}/api/show"), &headers, &body).and_then(http::read_json) else {
            continue;
        };
        if let Some(caps) = show["capabilities"].as_array() {
            if !caps.is_empty() && !caps.iter().any(|c| c == "completion") {
                embedding_only.push(m.id.clone());
            }
            apply_ollama_caps(m, caps);
        }
        if m.context.is_none() {
            m.context = show["model_info"]
                .as_object()
                .and_then(|o| o.iter().find(|(k, _)| k.ends_with(".context_length")).and_then(|(_, v)| v.as_u64()));
        }
    }
    Ok(models.into_iter().map(|(m, _)| m).filter(|m| !embedding_only.contains(&m.id)).collect())
}

fn apply_ollama_caps(m: &mut ModelInfo, caps: &[Value]) {
    let has = |c: &str| caps.iter().any(|x| x == c);
    m.thinking = if has("thinking") { ollama_levels(&m.id) } else { vec![] };
}

/// `(model, capabilities were present)`; embedding-only models are dropped.
pub fn parse_ollama(v: &Value) -> Vec<(ModelInfo, bool)> {
    let mut out = vec![];
    for m in v["models"].as_array().into_iter().flatten() {
        let Some(id) = m["name"].as_str().or_else(|| m["model"].as_str()) else { continue };
        let d = &m["details"];
        let desc: Vec<&str> = [d["parameter_size"].as_str(), d["family"].as_str(), d["quantization_level"].as_str()]
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
            .collect();
        let mut info =
            ModelInfo {
                id: id.to_string(), name: id.to_string(), description: desc.join(" · "), ..Default::default()
            };
        let caps = m["capabilities"].as_array();
        if let Some(caps) = caps {
            if !caps.iter().any(|c| c == "completion") && !caps.is_empty() {
                continue;
            }
            apply_ollama_caps(&mut info, caps);
        }
        out.push((info, caps.is_some()));
    }
    out
}

/// OpenRouter's catalog is public; `reasoning.supported_efforts` gives the levels per model.
fn openrouter(conn: &Connection) -> AiResult<Vec<ModelInfo>> {
    let v = http::get_json(&format!("{}/models", conn.base()), &[])?;
    Ok(parse_openrouter(&v))
}

pub fn parse_openrouter(v: &Value) -> Vec<ModelInfo> {
    let mut out = vec![];
    for m in v["data"].as_array().into_iter().flatten() {
        let Some(id) = m["id"].as_str() else { continue };
        let supports =
            m["supported_parameters"].as_array().map(|a| a.iter().any(|p| p == "reasoning")).unwrap_or(false);
        let r = &m["reasoning"];
        let mut levels: Vec<String> = r["supported_efforts"]
            .as_array()
            .map(|a| {
                a.iter().filter_map(Value::as_str).map(|s| if s == "none" { "off" } else { s }.to_string()).collect()
            })
            .unwrap_or_default();
        if levels.is_empty() && supports {
            levels = vec!["off".into(), "low".into(), "medium".into(), "high".into()];
        }
        if r["mandatory"] == true {
            levels.retain(|l| l != "off");
        }
        out.push(ModelInfo {
            id: id.to_string(),
            name: m["name"].as_str().unwrap_or(id).to_string(),
            description: m["description"].as_str().unwrap_or_default().chars().take(200).collect(),
            context: m["context_length"].as_u64(),
            thinking: order_levels(&levels),
            default_thinking: r["default_effort"].as_str().map(str::to_string),
        });
    }
    out
}

/// `GET {base}/models` of an OpenAI-compatible server. It says nothing about reasoning, so the usual
/// effort levels are offered (servers that ignore `reasoning_effort` simply answer normally).
fn openai_compatible(conn: &Connection, secrets: &dyn SecretStore) -> AiResult<Vec<ModelInfo>> {
    let base = conn.base();
    if base.is_empty() {
        return Err(AiError::NotConfigured("Enter the server address first.".into()));
    }
    let key = secrets.get(&conn.kind.key_secret()).filter(|k| !k.trim().is_empty());
    let headers: Vec<(&str, String)> =
        key.map(|k| vec![("Authorization", format!("Bearer {}", k.trim()))]).unwrap_or_default();
    let v = http::get_json(&format!("{base}/models"), &headers)?;
    Ok(v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["id"].as_str())
        .map(|id| ModelInfo {
            id: id.to_string(),
            name: id.to_string(),
            thinking: order_levels(&["low", "medium", "high"]),
            ..Default::default()
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn openrouter_levels_come_from_the_catalog() {
        let v = json!({"data": [
            {"id": "a/think", "name": "Think", "context_length": 1000, "supported_parameters": ["reasoning"],
             "reasoning": {"mandatory": false, "supported_efforts": ["max", "high", "low", "none"], "default_effort": "high"}},
            {"id": "b/plain", "name": "Plain", "supported_parameters": ["tools"]},
            {"id": "c/must", "supported_parameters": ["reasoning"], "reasoning": {"mandatory": true, "supported_efforts": ["none", "low"]}}
        ]});
        let m = parse_openrouter(&v);
        assert_eq!(m[0].thinking, vec!["off", "low", "high", "max"]);
        assert_eq!(m[0].default_thinking.as_deref(), Some("high"));
        assert_eq!(m[0].context, Some(1000));
        assert!(m[1].thinking.is_empty());
        assert_eq!(m[2].thinking, vec!["low"]);
        assert_eq!(m[2].name, "c/must");
    }
}
