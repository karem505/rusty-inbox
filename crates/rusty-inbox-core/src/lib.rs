//! rusty-inbox-core: the mail engine behind Rusty Inbox.
//!
//! - [`store`]  SQLite persistence: accounts, folders, threads, messages, settings, secrets.
//! - [`mail`]   IMAP sync, SMTP send, MIME parsing, server autodetection, threading helpers.
//! - [`engine`] the [`Engine`]: account sync, thread actions (done, pin, snooze, move, delete),
//!   on-demand body download and new-mail callbacks.
//! - [`compose`] drafts, reply / reply-all / forward, outgoing message building.
//! - [`search`] the search query language.

pub mod ai_support;
pub mod compose;
pub mod engine;
pub mod mail;
pub mod search;
pub mod store;
pub mod thunderbird;

pub use engine::{Engine, EngineConfig, NewMail};

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("{0}")]
    Message(String),
}

impl From<anyhow::Error> for CoreError {
    fn from(e: anyhow::Error) -> Self {
        CoreError::Message(format!("{e:#}"))
    }
}

impl From<rusqlite::Error> for CoreError {
    fn from(e: rusqlite::Error) -> Self {
        CoreError::Message(e.to_string())
    }
}

pub type CoreResult<T> = Result<T, CoreError>;
