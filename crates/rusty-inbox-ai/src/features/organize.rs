//! Features 11–15 (background / inbox-wide). WORKER C2 implements the prompts and parsers; keep these
//! signatures (add more if needed).

use super::{Lang, MailForAi};
use crate::ChatRequest;
use serde::{Deserialize, Serialize};

/// Feature 11 + 12: Classification of one new message.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Label {
    /// the [`MailForAi::id`] it belongs to
    pub id: i64,
    /// personal | notifications | newsletters | invitations (the Smart Inbox groups)
    pub category: String,
    /// the user is expected to answer
    pub needs_reply: bool,
    /// 0 = low, 1 = normal, 2 = high
    #[serde(default)]
    pub priority: u8,
    /// a few words, shown as a tooltip
    #[serde(default)]
    pub reason: String,
}

/// Classify a batch (≤ 20 messages, bodies clipped). JSON answer with one entry per id.
pub fn classify(batch: &[MailForAi], me: &str) -> ChatRequest {
    let _ = (batch, me);
    ChatRequest::default()
}

/// Labels for the ids of the batch only; unknown categories become `personal`.
pub fn parse_labels(answer: &str, batch: &[MailForAi]) -> Vec<Label> {
    let _ = (answer, batch);
    vec![]
}

/// Feature 13: Does this sent message ask the recipient for something (an answer, a document, a decision)?
pub fn expects_reply(sent: &MailForAi) -> ChatRequest {
    let _ = sent;
    ChatRequest::default()
}

/// `(expects a reply, what is awaited in a few words)`.
pub fn parse_expects_reply(answer: &str) -> Option<(bool, String)> {
    let _ = answer;
    None
}

/// Feature 14: Turn a question into the app's search query language (see `rusty_inbox_core::search`),
/// for example `"text" from:x to:y subject:z has:attachments in:sent is:unread since:2026-01-01`.
/// `today` is YYYY-MM-DD; `accounts` are the user's addresses.
pub fn search_query(question: &str, today: &str, accounts: &[String]) -> ChatRequest {
    let _ = (question, today, accounts);
    ChatRequest::default()
}

/// The query string out of the answer (one line, no quotes around the whole thing).
pub fn parse_search_query(answer: &str) -> Option<String> {
    let _ = answer;
    None
}

/// Feature 15: The daily briefing over unread mail (people first). Answer = Markdown.
pub fn briefing(unread: &[MailForAi], me: &str, lang: &Lang) -> ChatRequest {
    let _ = (unread, me, lang);
    ChatRequest::default()
}
