//! Features 6–10 and 16, used from the reader. WORKER C1 implements the prompts and parsers; keep
//! these signatures (add more if needed).

use super::{Lang, MailForAi};
use crate::ChatRequest;
use serde::{Deserialize, Serialize};

/// Feature 6 + 7: Summary card of a conversation.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ThreadSummary {
    /// 1–3 sentences
    pub summary: String,
    pub actions: Vec<ActionItem>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ActionItem {
    pub task: String,
    /// who should do it ("you", a name), empty when unclear
    #[serde(default)]
    pub owner: String,
    /// as written or as a date, empty when none
    #[serde(default)]
    pub due: String,
}

pub fn summarize_thread(thread: &[MailForAi], me: &str, lang: &Lang) -> ChatRequest {
    let _ = (thread, me, lang);
    ChatRequest::default()
}

pub fn parse_summary(answer: &str) -> Option<ThreadSummary> {
    let _ = answer;
    None
}

/// Feature 8: Translate a received message into `target`. Answer = plain text translation.
pub fn translate_message(msg: &MailForAi, target: &str) -> ChatRequest {
    let _ = (msg, target);
    ChatRequest::default()
}

/// Feature 9: Three short replies the user might send (JSON answer).
pub fn smart_replies(thread: &[MailForAi], me: &str, lang: &Lang) -> ChatRequest {
    let _ = (thread, me, lang);
    ChatRequest::default()
}

pub fn parse_smart_replies(answer: &str) -> Vec<String> {
    let _ = answer;
    vec![]
}

/// Feature 10: Summary of an attachment's text (from [`crate::extract::text_of`]). Answer = short summary.
pub fn summarize_attachment(name: &str, text: &str, lang: &Lang) -> ChatRequest {
    let _ = (name, text, lang);
    ChatRequest::default()
}

/// Feature 16: Phishing verdict.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PhishingVerdict {
    /// low | medium | high
    pub risk: String,
    pub reasons: Vec<String>,
}

/// What the check looks at besides the message itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PhishingEvidence {
    pub from: String,
    pub reply_to: String,
    pub return_path: String,
    /// raw `Authentication-Results` (SPF/DKIM/DMARC) when present
    pub authentication_results: String,
    /// every link in the HTML as (shown text, actual URL)
    pub links: Vec<(String, String)>,
}

pub fn phishing_check(msg: &MailForAi, evidence: &PhishingEvidence, lang: &Lang) -> ChatRequest {
    let _ = (msg, evidence, lang);
    ChatRequest::default()
}

pub fn parse_phishing(answer: &str) -> Option<PhishingVerdict> {
    let _ = answer;
    None
}
