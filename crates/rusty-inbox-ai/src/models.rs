//! Live model lists, straight from each provider (never a hard-coded catalog).

use crate::http;
use crate::provider::{Connection, ProviderKind};
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
        // WORKER A: Codex catalog, OpenAI /models (chat models only), Anthropic /models, Ollama /api/tags
        ProviderKind::OpenAI | ProviderKind::Codex | ProviderKind::Anthropic | ProviderKind::Ollama => {
            Err(AiError::NotConfigured(format!("{} is not implemented yet", conn.kind.label())))
        }
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
