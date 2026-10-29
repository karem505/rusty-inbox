//! Features 1–5, used from the composer. WORKER C1 implements the prompts and parsers; keep these
//! signatures (add more if needed).

use super::{Lang, MailForAi};
use crate::ChatRequest;

/// Rewrite styles offered in the composer menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Formal,
    Friendly,
    Shorter,
    Longer,
    Clearer,
    Persuasive,
}

impl Style {
    pub const ALL: [Style; 6] =
        [Style::Formal, Style::Friendly, Style::Shorter, Style::Longer, Style::Clearer, Style::Persuasive];
    pub fn id(self) -> &'static str {
        match self {
            Style::Formal => "formal",
            Style::Friendly => "friendly",
            Style::Shorter => "shorter",
            Style::Longer => "longer",
            Style::Clearer => "clearer",
            Style::Persuasive => "persuasive",
        }
    }
    pub fn from_id(id: &str) -> Option<Style> {
        Style::ALL.into_iter().find(|s| s.id() == id)
    }
}

/// Feature 1: Proofread: fix spelling, grammar and punctuation only; keep wording, tone and language.
/// The answer is the corrected text alone.
pub fn proofread(text: &str) -> ChatRequest {
    let _ = text;
    ChatRequest::default()
}

/// One step of a word-level diff between the draft and the corrected text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Diff {
    Same(String),
    Removed(String),
    Added(String),
}

/// Word-level diff (whitespace kept with the words) for highlighting the proofreading changes.
pub fn word_diff(old: &str, new: &str) -> Vec<Diff> {
    let _ = (old, new);
    vec![]
}

/// The diff as HTML for the preview: removed words `<del>`, added words `<ins>`, text escaped.
pub fn diff_html(diff: &[Diff]) -> String {
    let _ = diff;
    String::new()
}

/// Feature 2: Rewrite the draft in `style`, same language and meaning. Answer = the new text.
pub fn rewrite(text: &str, style: Style) -> ChatRequest {
    let _ = (text, style);
    ChatRequest::default()
}

/// Feature 3: Translate the draft into `target` (a language name, e.g. "English", "Arabic").
pub fn translate(text: &str, target: &str) -> ChatRequest {
    let _ = (text, target);
    ChatRequest::default()
}

/// Feature 4: Write a whole email from an instruction or bullet points. `thread` is the conversation being
/// replied to (oldest first), empty for a new message; `sender` is the user's name for the sign-off.
pub fn write_for_me(instruction: &str, thread: &[MailForAi], sender: &str, lang: &Lang) -> ChatRequest {
    let _ = (instruction, thread, sender, lang);
    ChatRequest::default()
}

/// Feature 5: Three subject lines for the draft (JSON answer).
pub fn subject_ideas(body: &str, lang: &Lang) -> ChatRequest {
    let _ = (body, lang);
    ChatRequest::default()
}

/// Parse [`subject_ideas`]' answer (up to 3, de-duplicated, without quotes).
pub fn parse_subject_ideas(answer: &str) -> Vec<String> {
    let _ = answer;
    vec![]
}
