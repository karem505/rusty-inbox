//! Live checks against the real services. `#[ignore]`d: run on purpose with
//! `cargo test -p rusty-inbox-ai --test live -- --ignored --nocapture --test-threads=1`.
//!
//! Read-only and careful with secrets: nothing is printed except model counts, ids and the answer.
//! - Codex: borrows the *access* token from `~/.hermes/auth.json` into memory, with an EMPTY refresh
//!   token so a refresh can never happen (refresh tokens are single-use; using Hermes's would log it out).
//! - Ollama cloud: key from the `OLLAMA_API_KEY=` line of `~/.hermes/.env`.

use rusty_inbox_ai::codex::{self, CodexTokens};
use rusty_inbox_ai::{
    chat, list_models, AiError, ChatRequest, Connection, MemorySecrets, Message, ProviderKind, SecretStore,
};
use std::sync::atomic::AtomicBool;

fn home_file(rel: &str) -> Option<String> {
    std::fs::read_to_string(format!("{}/{rel}", std::env::var("HOME").ok()?)).ok()
}

fn borrowed_codex_session() -> Option<MemorySecrets> {
    let v: serde_json::Value = serde_json::from_str(&home_file(".hermes/auth.json")?).ok()?;
    let access = v.pointer("/providers/openai-codex/tokens/access_token")?.as_str()?.to_string();
    let c = codex::parse_claims(&access)?;
    let t = CodexTokens {
        access_token: access,
        refresh_token: String::new(),
        account_id: c.account_id,
        residency: c.residency,
        email: c.email,
        plan: c.plan,
        expires_at: 4_000_000_000, // far in the future: never refresh
        ..Default::default()
    };
    let s = MemorySecrets::default();
    s.set(codex::SECRET_KEY, &serde_json::to_string(&t).unwrap());
    Some(s)
}

fn ping(conn: &Connection, s: &MemorySecrets, model: &str, thinking: Option<&str>) -> Result<String, AiError> {
    let req = ChatRequest {
        model: model.into(),
        thinking: thinking.map(str::to_string),
        system: "You are a test endpoint.".into(),
        messages: vec![Message::user("Reply with the single word: pong")],
        ..Default::default()
    };
    let mut streamed = String::new();
    let out = chat(conn, s, &req, &mut |d| streamed.push_str(d), &AtomicBool::new(false))?;
    assert_eq!(out.text, streamed);
    println!("  model {} usage {:?}", out.model, out.usage);
    Ok(out.text)
}

#[test]
#[ignore]
fn live_codex_models_and_chat() {
    let Some(s) = borrowed_codex_session() else { return println!("no Hermes Codex session on this PC: skipped") };
    let conn = Connection::new(ProviderKind::Codex);
    let models = list_models(&conn, &s).expect("codex model list");
    println!("codex: {} models", models.len());
    for m in models.iter().take(8) {
        println!("  {} | {} | ctx {:?} | {:?} | default {:?}", m.id, m.name, m.context, m.thinking, m.default_thinking);
    }
    assert!(!models.is_empty());
    let pick = models.iter().find(|m| m.thinking.iter().any(|l| l == "low")).unwrap_or(&models[0]).clone();
    println!("chatting with {} at effort low", pick.id);
    let t = std::time::Instant::now();
    let answer = ping(&conn, &s, &pick.id, Some("low")).expect("codex chat");
    println!("codex answer: {:?} in {:?}", answer.trim(), t.elapsed());
    assert!(answer.to_lowercase().contains("pong"));
    // the borrowed session must not have been rewritten (no refresh happened)
    assert_eq!(codex::load_tokens(&s).unwrap().refresh_token, "");
}

#[test]
#[ignore]
fn live_openrouter_models() {
    let models =
        list_models(&Connection::new(ProviderKind::OpenRouter), &MemorySecrets::default()).expect("openrouter list");
    let with_levels = models.iter().filter(|m| !m.thinking.is_empty()).count();
    println!("openrouter: {} models, {} with thinking levels", models.len(), with_levels);
    assert!(models.len() > 100);
}

#[test]
#[ignore]
fn live_ollama_cloud_models_and_quota_message() {
    let Some(env) = home_file(".hermes/.env") else { return println!("no ~/.hermes/.env: skipped") };
    let Some(key) = env.lines().find_map(|l| l.strip_prefix("OLLAMA_API_KEY=")) else {
        return println!("no OLLAMA_API_KEY: skipped");
    };
    let key = key.trim().trim_matches(|c| c == '"' || c == '\'').to_string();
    let s = MemorySecrets::default();
    s.set(&ProviderKind::Ollama.key_secret(), &key);
    let conn = Connection::with_base_url(ProviderKind::Ollama, "https://ollama.com");
    let models = list_models(&conn, &s).expect("ollama cloud list");
    let thinking = models.iter().filter(|m| !m.thinking.is_empty()).count();
    println!("ollama cloud: {} models, {} with thinking levels", models.len(), thinking);
    for m in models.iter().take(5) {
        println!("  {} | {} | {:?}", m.id, m.description, m.thinking);
    }
    assert!(!models.is_empty());
    let id = models.iter().find(|m| m.id.contains("gpt-oss")).unwrap_or(&models[0]).id.clone();
    match ping(&conn, &s, &id, None) {
        Ok(a) => println!("ollama cloud answered: {:?}", a.trim()),
        Err(e) => {
            // the weekly quota text must reach the user as the provider wrote it
            println!("ollama cloud chat error (shown to the user): {e}");
            assert!(matches!(e, AiError::RateLimited(_) | AiError::Auth(_) | AiError::Http { .. }));
        }
    }
}
