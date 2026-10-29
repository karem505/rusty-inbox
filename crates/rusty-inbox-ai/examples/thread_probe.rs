//! Run the reader prompts (summary, smart replies) on a conversation given as JSON, with a real model.
//!
//! ```sh
//! AI_PROBE_CODEX="$(…tokens json…)" cargo run -p rusty-inbox-ai --example thread_probe -- thread.json gpt-6-luna
//! ```
//! `thread.json` = `{"me": "Name <a@x>, b@y", "mails": [MailForAi, ...]}` (oldest first). The ChatGPT
//! tokens come from `$AI_PROBE_CODEX` only; give it an empty `refresh_token` so the probe can never
//! rotate (and so invalidate) the app's sign-in.

use rusty_inbox_ai::features::{read, Lang, MailForAi};
use rusty_inbox_ai::{chat, codex, Connection, MemorySecrets, ProviderKind, SecretStore};
use std::sync::atomic::AtomicBool;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&a[0]).expect("thread file")).unwrap();
    let me = v["me"].as_str().unwrap_or_default().to_string();
    let mails: Vec<MailForAi> = serde_json::from_value(v["mails"].clone()).unwrap();
    let secrets = MemorySecrets::default();
    secrets.set(codex::SECRET_KEY, &std::env::var("AI_PROBE_CODEX").expect("AI_PROBE_CODEX"));
    let conn = Connection::with_base_url(ProviderKind::Codex, "");
    for (name, mut req) in [
        ("summary", read::summarize_thread(&mails, &me, &Lang::Auto)),
        ("replies", read::smart_replies(&mails, &me, &Lang::Auto)),
    ] {
        req.model = a.get(1).cloned().unwrap_or_else(|| "gpt-6-luna".into());
        req.thinking = Some("low".into());
        match chat(&conn, &secrets, &req, &mut |_| {}, &AtomicBool::new(false)) {
            Ok(o) => println!("== {name}\n{}\n", o.text),
            Err(e) => println!("== {name}: error {e}\n"),
        }
    }
}
