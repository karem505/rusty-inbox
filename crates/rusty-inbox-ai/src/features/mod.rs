//! The mail features: each one is a pure prompt builder (returns a [`ChatRequest`] without the model,
//! which the caller fills in) plus a parser for the answer. No network here, so every prompt and
//! parser is unit-tested.
//!
//! - [`compose`]: proofread, rewrite, translate draft, write for me, subject ideas (features 1–5)
//! - [`read`]: thread summary + action items, translate message, smart replies, attachment summary,
//!   phishing check (6–10, 16)
//! - [`organize`]: classification + needs reply, follow-up check, plain-words search, daily briefing
//!   (11–15)

pub mod compose;
pub mod organize;
pub mod read;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One message as the model sees it. Callers fill it from the store; bodies are plain text.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MailForAi {
    /// the store's message id, echoed back by batch features
    pub id: i64,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub cc: String,
    /// RFC 3339 or a readable date
    pub date: String,
    pub subject: String,
    pub body: String,
    /// attachment file names
    #[serde(default)]
    pub attachments: Vec<String>,
    /// true when the user (one of their accounts) sent it
    #[serde(default)]
    pub from_me: bool,
}

/// Language of the answer: `Auto` = the language of the email / text itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Lang {
    #[default]
    Auto,
    /// a language name in English ("English", "Arabic", ...)
    Named(String),
}

impl Lang {
    /// From the `aiLanguage` setting: `auto`, `en`, `ar`, or a language name.
    pub fn from_setting(s: &str) -> Lang {
        match s.trim() {
            "" | "auto" => Lang::Auto,
            "en" => Lang::Named("English".into()),
            "ar" => Lang::Named("Arabic".into()),
            other => Lang::Named(other.to_string()),
        }
    }

    /// The sentence appended to every system prompt.
    pub fn instruction(&self) -> String {
        match self {
            Lang::Auto => "Answer in the same language as the email or text you are given (Arabic stays Arabic, English stays English).".into(),
            Lang::Named(l) => format!("Answer in {l}."),
        }
    }
}

/// Cut `s` to at most `max` characters on a char boundary, marking the cut.
pub fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}\n[…]", s.chars().take(max).collect::<String>())
    }
}

/// A thread rendered as plain text for a prompt, oldest first, each body clipped to `per_message`
/// characters and the whole to `total` (newest messages are kept when it is too long).
pub fn render_thread(msgs: &[MailForAi], per_message: usize, total: usize) -> String {
    let mut parts: Vec<String> = msgs
        .iter()
        .map(|m| {
            let mut s = format!("From: {}\nTo: {}\n", m.from, m.to);
            if !m.cc.is_empty() {
                s += &format!("Cc: {}\n", m.cc);
            }
            s += &format!("Date: {}\nSubject: {}\n", m.date, m.subject);
            if !m.attachments.is_empty() {
                s += &format!("Attachments: {}\n", m.attachments.join(", "));
            }
            s += &format!("\n{}", clip(m.body.trim(), per_message));
            s
        })
        .collect();
    while parts.len() > 1 && parts.iter().map(|p| p.chars().count()).sum::<usize>() > total {
        parts.remove(0);
    }
    parts.join("\n\n-----\n\n")
}

/// The first JSON object or array in a model answer: tolerates code fences, text around it and a
/// trailing comma before a closing bracket. `None` when there is no complete JSON value.
pub fn extract_json(text: &str) -> Option<Value> {
    let t = text.trim();
    if let Ok(v) = serde_json::from_str::<Value>(t) {
        if v.is_object() || v.is_array() {
            return Some(v);
        }
    }
    let start = t.find(['{', '['])?;
    let bytes = t.as_bytes();
    let (mut depth, mut in_str, mut esc) = (0i32, false, false);
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            match (esc, b) {
                (true, _) => esc = false,
                (false, b'\\') => esc = true,
                (false, b'"') => in_str = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth -= 1;
                if depth == 0 {
                    let candidate = &t[start..=i];
                    if let Ok(v) = serde_json::from_str::<Value>(candidate) {
                        return Some(v);
                    }
                    return serde_json::from_str::<Value>(&strip_trailing_commas(candidate)).ok();
                }
            }
            _ => {}
        }
    }
    None
}

/// Remove a comma that directly precedes `}` or `]` (outside strings). Never touches other commas.
fn strip_trailing_commas(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let (mut in_str, mut esc) = (false, false);
    for (i, &c) in chars.iter().enumerate() {
        if in_str {
            out.push(c);
            match (esc, c) {
                (true, _) => esc = false,
                (false, '\\') => esc = true,
                (false, '"') => in_str = false,
                _ => {}
            }
            continue;
        }
        if c == '"' {
            in_str = true;
        }
        if c == ',' {
            let next = chars[i + 1..].iter().find(|c| !c.is_whitespace());
            if matches!(next, Some('}') | Some(']')) {
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// Strip a ``` fence (with or without a language) around a plain-text answer, and surrounding quotes
/// a model sometimes adds.
pub fn clean_text(answer: &str) -> String {
    let mut t = answer.trim();
    if let Some(rest) = t.strip_prefix("```") {
        let rest = rest.split_once('\n').map(|x| x.1).unwrap_or(rest);
        t = rest.trim_end().strip_suffix("```").unwrap_or(rest).trim();
    }
    t.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_is_found_inside_fences_and_prose() {
        assert_eq!(extract_json("```json\n{\"a\": 1}\n```"), Some(json!({"a": 1})));
        assert_eq!(
            extract_json("Sure! {\"a\": [1, 2,], \"b\": \"x, }\"} thanks"),
            Some(json!({"a": [1, 2], "b": "x, }"}))
        );
        assert_eq!(extract_json("[{\"id\":1},]"), Some(json!([{"id": 1}])));
        assert_eq!(extract_json("{\"a\": 1, \"b\": "), None, "a truncated answer is not fabricated");
        assert_eq!(extract_json("no json"), None);
    }

    #[test]
    fn threads_render_newest_kept_and_clipped() {
        let m =
            |i: i64, body: &str| MailForAi { id: i, from: format!("p{i}"), body: body.into(), ..Default::default() };
        let t = render_thread(&[m(1, &"x".repeat(500)), m(2, "short")], 100, 10_000);
        assert!(t.contains("[…]") && t.contains("short"));
        let t = render_thread(&[m(1, &"x".repeat(500)), m(2, "short")], 1000, 200);
        assert!(!t.contains("p1") && t.contains("p2"));
        assert_eq!(clean_text("```\nHello\n```"), "Hello");
        assert_eq!(Lang::from_setting("ar"), Lang::Named("Arabic".into()));
    }
}
