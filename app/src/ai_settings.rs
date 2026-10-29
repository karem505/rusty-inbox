//! QML object behind Settings → AI: providers, keys, ChatGPT sign-in, live model list, thinking levels.
//! WORKER B owns this file. See docs/ai/DESIGN.md for the contract.
//!
//! Everything the page shows is JSON built by the pure functions below (unit-tested). Network work
//! (model lists, "Test connection", ChatGPT sign-in) runs on `std::thread::spawn` and reports back with
//! `qt_thread().queue` + a signal. Keys go to the secret store only; JSON for QML carries at most the
//! last 4 characters of a key, and error text is scrubbed of the key before it is shown.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type AiSettings = super::AiSettingsRust;

        /// AI is configured and on.
        #[qinvokable]
        fn available(self: &AiSettings) -> bool;

        /// Everything the page shows (JSON): switches, chosen provider / model / thinking, the provider
        /// cards (connected, key hint = last 4 characters, base URL), the ChatGPT account, the chosen
        /// model's thinking levels.
        #[qinvokable]
        fn state_json(self: &AiSettings) -> QString;

        /// Store an API key (secret store only). Returns "" or an error message.
        #[qinvokable]
        fn set_key(self: &AiSettings, provider: &QString, key: &QString) -> QString;

        /// Forget a provider's API key.
        #[qinvokable]
        fn clear_key(self: &AiSettings, provider: &QString);

        /// Save the server address of Ollama or Custom. Returns "" or an error message.
        #[qinvokable]
        fn set_base_url(self: &AiSettings, provider: &QString, url: &QString) -> QString;

        /// Make `provider` the one the AI features use (the model resets when the provider changes).
        #[qinvokable]
        fn choose_provider(self: &AiSettings, provider: &QString) -> QString;

        /// Fetch the provider's live model list in the background; answers with `models_loaded`.
        #[qinvokable]
        fn start_model_list(self: &AiSettings, provider: &QString);

        /// The picker rows for `query` (JSON): fuzzy-ranked, with the matched letters, `limit` rows at most.
        #[qinvokable]
        fn search_models(self: &AiSettings, provider: &QString, query: &QString, limit: i32) -> QString;

        #[qinvokable]
        fn choose_model(self: &AiSettings, model: &QString) -> QString;

        /// `""` = the model's default.
        #[qinvokable]
        fn choose_thinking(self: &AiSettings, level: &QString) -> QString;

        /// Send one tiny chat to the chosen model; answers with `test_done`.
        #[qinvokable]
        fn test_connection(self: &AiSettings);

        /// Browser sign-in (falls back to a device code when the loopback port is busy).
        #[qinvokable]
        fn sign_in_chatgpt(self: &AiSettings);

        /// Device-code sign-in directly.
        #[qinvokable]
        fn use_device_code(self: &AiSettings);

        #[qinvokable]
        fn cancel_sign_in(self: &AiSettings);

        #[qinvokable]
        fn sign_out_chatgpt(self: &AiSettings);

        /// Models of `provider` are in (`error` empty; `json` = `{"count": n}`) or failed.
        #[qsignal]
        fn models_loaded(self: Pin<&mut AiSettings>, provider: QString, json: QString, error: QString);

        /// "Test connection" finished: the model's reply and the time it took, or the error.
        #[qsignal]
        fn test_done(self: Pin<&mut AiSettings>, ok: bool, reply: QString, millis: i64, error: QString);

        /// Browser sign-in: open this address.
        #[qsignal]
        fn sign_in_url(self: Pin<&mut AiSettings>, url: QString);

        /// Device-code sign-in: open `url` and type `code`.
        #[qsignal]
        fn sign_in_code(self: Pin<&mut AiSettings>, url: QString, code: QString);

        /// Signed in (`account_json` = `{email, plan}`).
        #[qsignal]
        fn sign_in_done(self: Pin<&mut AiSettings>, account_json: QString);

        #[qsignal]
        fn sign_in_failed(self: Pin<&mut AiSettings>, message: QString);
    }

    impl cxx_qt::Threading for AiSettings {}
}

use crate::ai::{self, StoreSecrets};
use crate::engine::core;
use crate::settings;
use cxx_qt::Threading;
use cxx_qt_lib::QString;
use rusty_inbox_ai::codex::{self, Account};
use rusty_inbox_ai::{AuthKind, ChatRequest, ModelInfo, ProviderKind, SecretStore};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct AiSettingsRust {}

/// Last model list per provider (in memory only).
static MODELS: Mutex<Option<HashMap<String, Vec<ModelInfo>>>> = Mutex::new(None);
/// Cancel flag of the sign-in in progress.
static SIGN_IN: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

fn cached(provider: &str) -> Option<Vec<ModelInfo>> {
    MODELS.lock().ok()?.as_ref()?.get(provider).cloned()
}

fn cache_put(provider: &str, models: Vec<ModelInfo>) {
    if let Ok(mut g) = MODELS.lock() {
        g.get_or_insert_with(HashMap::new).insert(provider.to_string(), models);
    }
}

fn cache_drop(provider: &str) {
    if let Ok(mut g) = MODELS.lock() {
        if let Some(m) = g.as_mut() {
            m.remove(provider);
        }
    }
}

// ---- pure helpers (unit-tested) -----------------------------------------------------------------

/// The only part of a key the UI ever sees: its last 4 characters, and only when the key is long
/// enough that 4 characters reveal little (8+).
pub fn key_hint(key: &str) -> String {
    let k = key.trim();
    let n = k.chars().count();
    if n < 8 {
        return String::new();
    }
    k.chars().skip(n - 4).collect()
}

/// Reject keys that cannot be real before they are stored.
pub fn validate_key(key: &str) -> Result<String, String> {
    let k = key.trim();
    if k.is_empty() {
        return Err("Paste the key first.".into());
    }
    if k.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("A key has no spaces or line breaks. Paste it again.".into());
    }
    if k.len() > 400 {
        return Err("That is too long to be a key.".into());
    }
    Ok(k.to_string())
}

/// A server address: http(s)://host[:port][/path]. Empty is allowed (Custom: not set yet).
pub fn validate_url(url: &str) -> Result<String, String> {
    let u = url.trim().trim_end_matches('/');
    if u.is_empty() {
        return Ok(String::new());
    }
    let rest = u.strip_prefix("https://").or_else(|| u.strip_prefix("http://"));
    match rest {
        Some(r) if !r.is_empty() && !r.starts_with('/') && !u.chars().any(|c| c.is_whitespace() || c.is_control()) => {
            Ok(u.to_string())
        }
        _ => Err("The address must look like http://localhost:1234/v1 or https://example.com/v1.".into()),
    }
}

/// Replace every secret in `msg` (providers sometimes echo the key in an error).
pub fn scrub(msg: &str, secrets: &[String]) -> String {
    let mut out = msg.to_string();
    for s in secrets.iter().filter(|s| s.trim().len() >= 6) {
        out = out.replace(s.trim(), "[REDACTED]");
    }
    out
}

/// "128K", "1M", "" for an unknown size.
pub fn context_label(ctx: Option<u64>) -> String {
    match ctx {
        None | Some(0) => String::new(),
        Some(n) if n >= 1_000_000 => {
            let m = n as f64 / 1_000_000.0;
            if (m - m.round()).abs() < 0.05 {
                format!("{}M", m.round() as u64)
            } else {
                format!("{m:.1}M")
            }
        }
        Some(n) if n >= 1000 => format!("{}K", (n + 500) / 1000),
        Some(n) => n.to_string(),
    }
}

fn is_local(url: &str) -> bool {
    let u = url.to_lowercase();
    ["localhost", "127.0.0.1", "[::1]", "0.0.0.0"].iter().any(|h| u.contains(h))
}

/// A provider is "configured" when the credentials it needs are in place: a model list can be asked for.
pub fn is_configured(kind: ProviderKind, has_key: bool, base_url: &str, account: bool) -> bool {
    match kind {
        ProviderKind::Codex => account,
        ProviderKind::Ollama => !base_url.trim().is_empty() && (is_local(base_url) || has_key),
        ProviderKind::Custom => !base_url.trim().is_empty(),
        _ => has_key,
    }
}

/// What the card says: "Connected" needs credentials, and for Ollama on this computer (which needs
/// none) a model list that actually came back, so a server that is not running is not "connected".
pub fn is_connected(kind: ProviderKind, has_key: bool, base_url: &str, account: bool, listed: bool) -> bool {
    let configured = is_configured(kind, has_key, base_url, account);
    if kind == ProviderKind::Ollama && is_local(base_url) {
        return configured && listed;
    }
    configured
}

/// One provider card for the page. `key` is only read to build the hint, never copied into the JSON.
pub fn provider_entry(
    kind: ProviderKind,
    key: Option<&str>,
    base_url: &str,
    account: Option<&Account>,
    listed: bool,
) -> Value {
    let has_key = key.is_some_and(|k| !k.trim().is_empty());
    json!({
        "id": kind.id(),
        "label": kind.label(),
        "blurb": kind.blurb(),
        "auth": match kind.auth() { AuthKind::ApiKey => "key", AuthKind::OptionalKey => "optionalKey", AuthKind::ChatGpt => "chatgpt" },
        "configured": is_configured(kind, has_key, base_url, account.is_some()),
        "connected": is_connected(kind, has_key, base_url, account.is_some(), listed),
        "hasKey": has_key,
        "keyHint": key.map(key_hint).unwrap_or_default(),
        "baseUrl": base_url,
        "baseEditable": kind.base_url_editable(),
        "defaultBase": kind.default_base_url(),
    })
}

/// The list with the saved model kept in it even when the provider no longer lists it.
pub fn merge_saved(mut models: Vec<ModelInfo>, saved: &str) -> Vec<(ModelInfo, bool)> {
    let saved = saved.trim();
    let mut out: Vec<(ModelInfo, bool)> = vec![];
    if !saved.is_empty() && !models.iter().any(|m| m.id == saved) {
        out.push((ModelInfo { id: saved.to_string(), name: saved.to_string(), ..Default::default() }, false));
    }
    out.extend(models.drain(..).map(|m| (m, true)));
    out
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

fn word_start(t: &[char], i: usize) -> bool {
    i == 0 || !t[i - 1].is_alphanumeric() || (t[i - 1].is_lowercase() && t[i].is_uppercase())
}

/// Fuzzy match of `query` (spaces ignored) in `text`: the letters must appear in order. Returns the
/// score and the matched char positions. Consecutive letters and letters at the start of a word score
/// more; skipped letters and long texts cost a little.
pub fn fuzzy(query: &str, text: &str) -> Option<(i32, Vec<usize>)> {
    let q: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).map(lower).collect();
    let t: Vec<char> = text.chars().collect();
    let tl: Vec<char> = t.iter().map(|c| lower(*c)).collect();
    if q.is_empty() {
        return Some((0, vec![]));
    }
    let mut best: Option<(i32, Vec<usize>)> = None;
    for start in 0..tl.len() {
        if tl[start] != q[0] {
            continue;
        }
        let mut pos = vec![start];
        let mut ok = true;
        for &qc in &q[1..] {
            let last = pos[pos.len() - 1];
            if tl.get(last + 1) == Some(&qc) {
                pos.push(last + 1);
                continue;
            }
            // else the next start of a word that has the letter, else the next hit anywhere
            let next = (last + 1..tl.len())
                .find(|&j| tl[j] == qc && word_start(&t, j))
                .or_else(|| (last + 1..tl.len()).find(|&j| tl[j] == qc));
            match next {
                Some(j) => pos.push(j),
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            continue;
        }
        let mut score = 0i32;
        for (k, &j) in pos.iter().enumerate() {
            score += 10;
            if word_start(&t, j) {
                score += 12;
            }
            if k > 0 {
                let gap = j - pos[k - 1] - 1;
                if gap == 0 {
                    score += 16;
                } else {
                    score -= gap.min(8) as i32;
                }
            }
        }
        if pos[0] == 0 {
            score += 8;
        }
        score -= (t.len() / 8) as i32;
        if best.as_ref().is_none_or(|(b, _)| score > *b) {
            best = Some((score, pos));
        }
    }
    best
}

/// One picker row.
#[derive(Debug, PartialEq)]
pub struct Hit {
    pub index: usize,
    pub score: i32,
    pub name_hl: Vec<usize>,
    pub id_hl: Vec<usize>,
}

/// Rank `models` for `query`: every whitespace-separated term must match the name or the id. An empty
/// query keeps the provider's own order.
pub fn rank(models: &[ModelInfo], query: &str) -> Vec<Hit> {
    let terms: Vec<&str> = query.split_whitespace().collect();
    let mut hits = vec![];
    'models: for (index, m) in models.iter().enumerate() {
        let (mut score, mut name_hl, mut id_hl) = (0, vec![], vec![]);
        for term in &terms {
            let n = fuzzy(term, &m.name);
            let i = fuzzy(term, &m.id);
            if n.is_none() && i.is_none() {
                continue 'models;
            }
            score += n.as_ref().map(|x| x.0).max(i.as_ref().map(|x| x.0)).unwrap_or(0);
            if let Some((_, p)) = n {
                name_hl.extend(p);
            }
            if let Some((_, p)) = i {
                id_hl.extend(p);
            }
        }
        name_hl.sort_unstable();
        name_hl.dedup();
        id_hl.sort_unstable();
        id_hl.dedup();
        hits.push(Hit { index, score, name_hl, id_hl });
    }
    if !terms.is_empty() {
        hits.sort_by(|a, b| b.score.cmp(&a.score).then(a.index.cmp(&b.index)));
    }
    hits
}

/// The picker rows as JSON: `{rows, total, shown, exact}`; `exact` = the typed text is a model id.
pub fn rows_json(models: Vec<ModelInfo>, saved: &str, query: &str, limit: usize) -> Value {
    let merged = merge_saved(models, saved);
    let list: Vec<ModelInfo> = merged.iter().map(|(m, _)| m.clone()).collect();
    let hits = rank(&list, query);
    let q = query.trim();
    let rows: Vec<Value> = hits
        .iter()
        .take(limit)
        .map(|h| {
            let (m, listed) = &merged[h.index];
            json!({
                "id": m.id, "name": m.name, "description": m.description,
                "context": context_label(m.context), "thinking": m.thinking,
                "defaultThinking": m.default_thinking, "listed": listed,
                "current": m.id == saved.trim(), "nameHl": h.name_hl, "idHl": h.id_hl,
            })
        })
        .collect();
    json!({
        "rows": rows, "total": list.len(), "shown": rows.len(),
        "exact": !q.is_empty() && list.iter().any(|m| m.id == q),
    })
}

// ---- glue ---------------------------------------------------------------------------------------

fn provider_label(id: &str) -> &'static str {
    ProviderKind::from_id(id).map(ProviderKind::label).unwrap_or("")
}

fn s(q: &QString) -> String {
    q.to_string()
}

fn pref(key: &str) -> Value {
    settings::get(core().store(), key)
}

fn pref_str(key: &str) -> String {
    pref(key).as_str().unwrap_or_default().to_string()
}

fn save(key: &str, value: Value) -> String {
    settings::set(core().store(), key, &value).err().unwrap_or_default()
}

fn base_of(kind: ProviderKind) -> String {
    match kind {
        ProviderKind::Ollama => pref_str("aiOllamaUrl"),
        ProviderKind::Custom => pref_str("aiCustomUrl"),
        _ => String::new(),
    }
}

fn state() -> Value {
    let acc = codex::account(&StoreSecrets);
    let providers: Vec<Value> = ProviderKind::ALL
        .iter()
        .map(|k| {
            let key = StoreSecrets.get(&k.key_secret());
            provider_entry(*k, key.as_deref(), &base_of(*k), acc.as_ref(), cached(k.id()).is_some())
        })
        .collect();
    let provider = pref_str("aiProvider");
    let model = pref_str("aiModel");
    let list = cached(&provider);
    let info = list.as_ref().and_then(|l| l.iter().find(|m| m.id == model));
    json!({
        "enabled": pref("aiEnabled") == json!(true),
        "provider": provider,
        "providerLabel": provider_label(&provider),
        "model": model,
        "thinking": pref("aiThinking"),
        "providers": providers,
        "account": acc.map(|a| json!({"email": a.email, "plan": a.plan})),
        "testHook": ai::test_hook(),
        "modelsLoaded": list.is_some(),
        "modelName": info.map(|m| m.name.clone()).unwrap_or_else(|| model.clone()),
        "modelThinking": info.map(|m| m.thinking.clone()).unwrap_or_default(),
        "modelDefaultThinking": info.and_then(|m| m.default_thinking.clone()),
    })
}

fn all_secrets() -> Vec<String> {
    ProviderKind::ALL.iter().filter_map(|k| StoreSecrets.get(&k.key_secret())).collect()
}

fn port_busy(e: &rusty_inbox_ai::AiError) -> bool {
    let m = e.to_string().to_lowercase();
    ["busy", "in use", "1455", "address"].iter().any(|w| m.contains(w))
}

fn queue<F>(qt: &cxx_qt::CxxQtThread<qobject::AiSettings>, f: F)
where
    F: FnOnce(Pin<&mut qobject::AiSettings>) + Send + 'static,
{
    let _ = qt.queue(f);
}

impl qobject::AiSettings {
    fn available(&self) -> bool {
        ai::available()
    }

    fn state_json(&self) -> QString {
        QString::from(&state().to_string())
    }

    fn set_key(&self, provider: &QString, key: &QString) -> QString {
        let Some(kind) = ProviderKind::from_id(&s(provider)) else { return QString::from("Unknown provider.") };
        if kind.auth() == AuthKind::ChatGpt {
            return QString::from("This provider signs in with your ChatGPT account; it has no key.");
        }
        match validate_key(&s(key)) {
            Ok(k) => {
                StoreSecrets.set(&kind.key_secret(), &k);
                cache_drop(kind.id());
                QString::default()
            }
            Err(e) => QString::from(&e),
        }
    }

    fn clear_key(&self, provider: &QString) {
        if let Some(kind) = ProviderKind::from_id(&s(provider)) {
            StoreSecrets.delete(&kind.key_secret());
            cache_drop(kind.id());
        }
    }

    fn set_base_url(&self, provider: &QString, url: &QString) -> QString {
        let (key, kind) = match s(provider).as_str() {
            "ollama" => ("aiOllamaUrl", ProviderKind::Ollama),
            "custom" => ("aiCustomUrl", ProviderKind::Custom),
            _ => return QString::from("This provider has a fixed address."),
        };
        match validate_url(&s(url)) {
            Ok(u) => {
                let u = if u.is_empty() && kind == ProviderKind::Ollama { "http://localhost:11434".into() } else { u };
                cache_drop(kind.id());
                QString::from(&save(key, json!(u)))
            }
            Err(e) => QString::from(&e),
        }
    }

    fn choose_provider(&self, provider: &QString) -> QString {
        let id = s(provider);
        if id != pref_str("aiProvider") {
            let e = save("aiProvider", json!(id));
            if !e.is_empty() {
                return QString::from(&e);
            }
            save("aiModel", json!(""));
            save("aiThinking", json!(""));
        }
        QString::default()
    }

    fn start_model_list(&self, provider: &QString) {
        let qt = self.qt_thread();
        let id = s(provider);
        std::thread::spawn(move || {
            let result = match ProviderKind::from_id(&id) {
                Some(kind) => {
                    rusty_inbox_ai::list_models(&ai::connection(kind), &StoreSecrets).map_err(|e| e.to_string())
                }
                None => Err("Unknown provider.".to_string()),
            };
            let (json, err) = match result {
                Ok(list) => {
                    let n = list.len();
                    cache_put(&id, list);
                    (json!({"count": n}).to_string(), String::new())
                }
                Err(e) => (json!({"count": 0}).to_string(), scrub(&e, &all_secrets())),
            };
            queue(&qt, move |mut o| {
                o.as_mut().models_loaded(QString::from(&id), QString::from(&json), QString::from(&err))
            });
        });
    }

    fn search_models(&self, provider: &QString, query: &QString, limit: i32) -> QString {
        let id = s(provider);
        let saved = if pref_str("aiProvider") == id { pref_str("aiModel") } else { String::new() };
        let v = rows_json(cached(&id).unwrap_or_default(), &saved, &s(query), limit.max(1) as usize);
        QString::from(&v.to_string())
    }

    fn choose_model(&self, model: &QString) -> QString {
        let m = s(model).trim().to_string();
        if m.is_empty() || m.len() > 200 || m.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return QString::from("That is not a model id.");
        }
        let e = save("aiModel", json!(m));
        if !e.is_empty() {
            return QString::from(&e);
        }
        // keep the level only when the new model accepts it
        let cur = pref_str("aiThinking");
        let accepts = cached(&pref_str("aiProvider"))
            .and_then(|l| l.into_iter().find(|x| x.id == m))
            .is_some_and(|x| x.thinking.contains(&cur));
        if !cur.is_empty() && !accepts {
            save("aiThinking", json!(""));
        }
        QString::default()
    }

    fn choose_thinking(&self, level: &QString) -> QString {
        let l = s(level);
        if !l.is_empty() {
            let model = pref_str("aiModel");
            let known = cached(&pref_str("aiProvider"))
                .and_then(|v| v.into_iter().find(|x| x.id == model))
                .is_some_and(|x| x.thinking.contains(&l));
            if !known {
                return QString::from("This model has no such thinking level.");
            }
        }
        QString::from(&save("aiThinking", json!(l)))
    }

    fn test_connection(&self) {
        let qt = self.qt_thread();
        std::thread::spawn(move || {
            let started = Instant::now();
            let result = ai::selected().and_then(|a| {
                let req = ChatRequest {
                    model: a.model,
                    thinking: a.thinking,
                    system: "You are a connection test. Answer with the single word OK.".into(),
                    messages: vec![rusty_inbox_ai::Message::user("Reply with the single word OK")],
                    ..Default::default()
                };
                let cancel = AtomicBool::new(false);
                rusty_inbox_ai::chat(&a.conn, &StoreSecrets, &req, &mut |_| {}, &cancel).map_err(|e| e.to_string())
            });
            let millis = started.elapsed().as_millis() as i64;
            let (ok, reply, err) = match result {
                Ok(out) => (true, out.text.trim().chars().take(160).collect::<String>(), String::new()),
                Err(e) => (false, String::new(), scrub(&e, &all_secrets())),
            };
            queue(&qt, move |mut o| o.as_mut().test_done(ok, QString::from(&reply), millis, QString::from(&err)));
        });
    }

    fn sign_in_chatgpt(&self) {
        self.sign_in(false);
    }

    fn use_device_code(&self) {
        self.sign_in(true);
    }

    fn cancel_sign_in(&self) {
        if let Ok(mut g) = SIGN_IN.lock() {
            if let Some(c) = g.take() {
                c.store(true, Ordering::SeqCst);
            }
        }
    }

    fn sign_out_chatgpt(&self) {
        self.cancel_sign_in();
        codex::sign_out(&StoreSecrets);
        cache_drop("codex");
    }

    fn sign_in(&self, device: bool) {
        let cancel = Arc::new(AtomicBool::new(false));
        if let Ok(mut g) = SIGN_IN.lock() {
            if let Some(old) = g.replace(cancel.clone()) {
                old.store(true, Ordering::SeqCst);
            }
        }
        let qt = self.qt_thread();
        std::thread::spawn(move || {
            let result: Result<Account, String> = (|| {
                if !device {
                    match codex::start_browser_login() {
                        Ok(login) => {
                            let url = login.url.clone();
                            queue(&qt, move |mut o| o.as_mut().sign_in_url(QString::from(&url)));
                            return login
                                .wait(&StoreSecrets, Duration::from_secs(600), &cancel)
                                .map_err(|e| e.to_string());
                        }
                        Err(e) if port_busy(&e) => {} // fall back to the device code
                        Err(e) => return Err(e.to_string()),
                    }
                }
                let d = codex::start_device_login().map_err(|e| e.to_string())?;
                let (url, code) = (d.verification_url.clone(), d.user_code.clone());
                queue(&qt, move |mut o| o.as_mut().sign_in_code(QString::from(&url), QString::from(&code)));
                d.wait(&StoreSecrets, &cancel).map_err(|e| e.to_string())
            })();
            if cancel.load(Ordering::SeqCst) {
                return; // the user cancelled: the page already moved on
            }
            if let Ok(mut g) = SIGN_IN.lock() {
                if g.as_ref().is_some_and(|c| Arc::ptr_eq(c, &cancel)) {
                    *g = None;
                }
            }
            cache_drop("codex");
            match result {
                Ok(a) => {
                    let j = json!({"email": a.email, "plan": a.plan}).to_string();
                    queue(&qt, move |mut o| o.as_mut().sign_in_done(QString::from(&j)));
                }
                Err(e) => {
                    let e = scrub(&e, &all_secrets());
                    queue(&qt, move |mut o| o.as_mut().sign_in_failed(QString::from(&e)));
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: &str, name: &str) -> ModelInfo {
        ModelInfo { id: id.into(), name: name.into(), ..Default::default() }
    }

    #[test]
    fn key_hint_never_shows_more_than_four_characters() {
        assert_eq!(key_hint("sk-abcdefgh1234"), "1234");
        assert_eq!(key_hint("  sk-abcdefgh1234 \n"), "1234");
        assert_eq!(key_hint("short"), "", "short keys show nothing");
        assert_eq!(key_hint(""), "");
        for k in ["sk-ant-api03-ZZZZZZZZZZZZZZZZ1a2b", "12345678", "ключ-секретный-ключ"] {
            assert!(key_hint(k).chars().count() <= 4);
        }
    }

    #[test]
    fn provider_json_carries_only_the_hint() {
        let key = "sk-live-VERYSECRETVALUE9876";
        let v = provider_entry(ProviderKind::OpenAI, Some(key), "", None, false);
        let text = v.to_string();
        assert!(!text.contains("VERYSECRET"), "{text}");
        assert_eq!(v["keyHint"], "9876");
        assert_eq!(v["connected"], true);
        assert_eq!(v["auth"], "key");
        let none = provider_entry(ProviderKind::Anthropic, None, "", None, false);
        assert_eq!(none["connected"], false);
        assert_eq!(none["keyHint"], "");
    }

    #[test]
    fn connected_rules_per_provider() {
        let c = |k, key, base, acc, listed| is_connected(k, key, base, acc, listed);
        assert!(!c(ProviderKind::Codex, true, "", false, false));
        assert!(c(ProviderKind::Codex, false, "", true, false));
        assert!(!c(ProviderKind::Ollama, false, "https://ollama.com", false, true));
        assert!(c(ProviderKind::Ollama, true, "https://ollama.com", false, false));
        assert!(!c(ProviderKind::Custom, true, "", false, false));
        assert!(c(ProviderKind::Custom, false, "http://127.0.0.1:1234/v1", false, false));
        assert!(!c(ProviderKind::OpenRouter, false, "", false, false));
        // Ollama on this computer needs no credentials, but is only "connected" once it answered
        assert!(is_configured(ProviderKind::Ollama, false, "http://localhost:11434", false));
        assert!(!c(ProviderKind::Ollama, false, "http://localhost:11434", false, false));
        assert!(c(ProviderKind::Ollama, false, "http://localhost:11434", false, true));
        let v = provider_entry(ProviderKind::Ollama, None, "http://localhost:11434", None, false);
        assert_eq!((v["configured"].as_bool(), v["connected"].as_bool()), (Some(true), Some(false)));
    }

    #[test]
    fn key_and_url_validation() {
        assert!(validate_key("").is_err());
        assert!(validate_key("sk abc").is_err());
        assert!(validate_key("sk-abc\n").is_ok());
        assert_eq!(validate_key("  sk-abc ").unwrap(), "sk-abc");
        assert_eq!(validate_url("http://localhost:1234/v1/").unwrap(), "http://localhost:1234/v1");
        assert_eq!(validate_url("").unwrap(), "");
        assert!(validate_url("localhost:1234").is_err());
        assert!(validate_url("ftp://x").is_err());
        assert!(validate_url("http://").is_err());
        assert!(validate_url("http://a b").is_err());
    }

    #[test]
    fn errors_are_scrubbed_of_keys() {
        let msg = "401: invalid key sk-live-ABCDEF123456 for this project";
        let out = scrub(msg, &["sk-live-ABCDEF123456".to_string(), "".to_string()]);
        assert!(!out.contains("ABCDEF"));
        assert!(out.contains("[REDACTED]"));
    }

    #[test]
    fn context_labels() {
        assert_eq!(context_label(None), "");
        assert_eq!(context_label(Some(128_000)), "128K");
        assert_eq!(context_label(Some(131_072)), "131K");
        assert_eq!(context_label(Some(200_000)), "200K");
        assert_eq!(context_label(Some(1_000_000)), "1M");
        assert_eq!(context_label(Some(1_048_576)), "1M");
        assert_eq!(context_label(Some(2_000_000)), "2M");
        assert_eq!(context_label(Some(512)), "512");
    }

    #[test]
    fn saved_model_stays_in_the_list() {
        let list = vec![m("a", "A"), m("b", "B")];
        let merged = merge_saved(list.clone(), "gone-model");
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].0.id, "gone-model");
        assert!(!merged[0].1, "marked as not listed");
        assert!(merged[1].1);
        assert_eq!(merge_saved(list.clone(), "b").len(), 2);
        assert_eq!(merge_saved(list, "").len(), 2);
        let v = rows_json(vec![m("a", "A")], "gone", "", 10);
        assert_eq!(v["rows"][0]["id"], "gone");
        assert_eq!(v["rows"][0]["listed"], false);
        assert_eq!(v["rows"][0]["current"], true);
    }

    #[test]
    fn fuzzy_matches_in_order_and_prefers_word_starts_and_runs() {
        assert!(fuzzy("xyz", "claude-sonnet").is_none());
        assert!(fuzzy("tnos", "claude-sonnet").is_none(), "order matters");
        let (_, p) = fuzzy("son", "claude-sonnet-4").unwrap();
        assert_eq!(p, vec![7, 8, 9]);
        let (a, _) = fuzzy("gpt", "openai/gpt-5").unwrap();
        let (b, _) = fuzzy("gpt", "gxpxt-model").unwrap();
        assert!(a > b, "consecutive beats scattered: {a} vs {b}");
        let (w, _) = fuzzy("cs", "claude-sonnet").unwrap();
        let (n, _) = fuzzy("cs", "acsb").unwrap();
        assert!(w > n, "word starts beat mid-word hits");
        let (_, p) = fuzzy("CS", "Claude Sonnet").unwrap();
        assert_eq!(p, vec![0, 7], "case-insensitive, picks the word starts");
        assert_eq!(fuzzy("", "x").unwrap().1, Vec::<usize>::new());
    }

    #[test]
    fn rank_orders_by_score_and_keeps_provider_order_without_a_query() {
        let list =
            vec![m("x/gpt-oss", "GPT OSS"), m("a/gemma", "Gemma"), m("openai/gpt-5", "GPT-5"), m("b/other", "Other")];
        assert_eq!(rank(&list, "").iter().map(|h| h.index).collect::<Vec<_>>(), vec![0, 1, 2, 3]);
        let hits = rank(&list, "gpt");
        assert_eq!(hits.len(), 2);
        assert!(!hits[0].name_hl.is_empty());
        let two = rank(&list, "gpt 5");
        assert_eq!(two.len(), 1, "every term must match");
        assert_eq!(two[0].index, 2);
        assert!(rank(&list, "zzz").is_empty());
    }

    #[test]
    fn rows_report_an_exact_id_and_limit() {
        let list: Vec<ModelInfo> = (0..50).map(|i| m(&format!("model-{i}"), &format!("Model {i}"))).collect();
        let v = rows_json(list.clone(), "", "model", 10);
        assert_eq!(v["rows"].as_array().unwrap().len(), 10);
        assert_eq!(v["total"], 50);
        assert_eq!(v["exact"], false);
        assert_eq!(rows_json(list.clone(), "", "model-7", 5)["exact"], true);
        assert_eq!(rows_json(list, "", "", 5)["exact"], false);
    }
}
