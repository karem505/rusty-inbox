//! Try a provider from the command line: list models, or stream one answer.
//!
//! ```sh
//! cargo run -p rusty-inbox-ai --example ai_probe -- models custom http://127.0.0.1:18765/v1
//! cargo run -p rusty-inbox-ai --example ai_probe -- chat custom http://127.0.0.1:18765/v1 mock "Say hi"
//! cargo run -p rusty-inbox-ai --example ai_probe -- models ollama https://ollama.com   # AI_PROBE_KEY=...
//! cargo run -p rusty-inbox-ai --example ai_probe -- chat anthropic - claude-sonnet-4-5 "Say hi" high
//! ```
//! The ChatGPT subscription needs the app's sign-in (tokens in the secret store), so it is not probed here.
//! An API key, when the provider needs one, is read from `$AI_PROBE_KEY` (never from the command line).

use rusty_inbox_ai::{chat, list_models, ChatRequest, Connection, MemorySecrets, Message, ProviderKind, SecretStore};
use std::io::Write;
use std::sync::atomic::AtomicBool;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 2 {
        eprintln!("usage: ai_probe models|chat <provider> [base url|-] [model] [prompt] [thinking]");
        std::process::exit(2);
    }
    let kind = ProviderKind::from_id(&a[1]).expect("provider id");
    let base = a.get(2).filter(|b| *b != "-").cloned().unwrap_or_default();
    let conn = Connection::with_base_url(kind, &base);
    let secrets = MemorySecrets::default();
    if let Ok(k) = std::env::var("AI_PROBE_KEY") {
        secrets.set(&kind.key_secret(), &k);
    }
    match a[0].as_str() {
        "models" => match list_models(&conn, &secrets) {
            Ok(ms) => {
                for m in &ms {
                    println!("{}\t{}\tctx={:?}\tthinking={:?}", m.id, m.name, m.context, m.thinking);
                }
                println!("{} models", ms.len());
            }
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        },
        _ => {
            let req = ChatRequest {
                model: a.get(3).cloned().unwrap_or_default(),
                thinking: a.get(5).cloned(),
                system: "You are a helpful assistant.".into(),
                messages: vec![Message::user(a.get(4).cloned().unwrap_or_else(|| "Say hi".into()))],
                ..Default::default()
            };
            let t = std::time::Instant::now();
            let r = chat(
                &conn,
                &secrets,
                &req,
                &mut |d| {
                    print!("{d}");
                    let _ = std::io::stdout().flush();
                },
                &AtomicBool::new(false),
            );
            match r {
                Ok(o) => println!("\n-- model {} usage {:?} in {:?}", o.model, o.usage, t.elapsed()),
                Err(e) => {
                    eprintln!("\nerror: {e}");
                    std::process::exit(1);
                }
            }
        }
    }
}
