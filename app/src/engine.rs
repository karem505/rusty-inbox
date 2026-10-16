//! Process-wide engine: one `rusty_inbox_core::Engine`, one tokio runtime, and a sync loop that
//! notifies the UI after every pass. JSON shaping for QML lives here as plain functions so it is
//! unit-testable without Qt.

use rusty_inbox_core::compose::Draft;
use rusty_inbox_core::store::{Account, ListFilter, MessageRow, Store, ThreadRow};
use rusty_inbox_core::{Engine, EngineConfig};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

pub fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("rusty-inbox-engine")
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

/// `$RUSTY_INBOX_DATA_DIR`, else the platform data dir: `$XDG_DATA_HOME/rusty-inbox/core-data`
/// on Linux, `~/Library/Application Support/rusty-inbox/core-data` on macOS,
/// `%APPDATA%\\rusty-inbox\\core-data` on Windows.
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("RUSTY_INBOX_DATA_DIR") {
        return PathBuf::from(d);
    }
    let base = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share"));
    base.join("rusty-inbox/core-data")
}

pub fn core() -> &'static Engine {
    static CORE: OnceLock<Engine> = OnceLock::new();
    CORE.get_or_init(|| {
        let mut cfg = EngineConfig::for_dir(&data_dir());
        cfg.network = std::env::var_os("RUSTY_INBOX_OFFLINE").is_none();
        Engine::new(cfg).expect("engine start")
    })
}

fn wake() -> &'static tokio::sync::Notify {
    static WAKE: OnceLock<tokio::sync::Notify> = OnceLock::new();
    WAKE.get_or_init(tokio::sync::Notify::new)
}

/// Cut the current wait short and sync now ("Check for new mail").
pub fn sync_now() {
    wake().notify_one();
}

fn outbox_wake() -> &'static tokio::sync::Notify {
    static WAKE: OnceLock<tokio::sync::Notify> = OnceLock::new();
    WAKE.get_or_init(tokio::sync::Notify::new)
}

/// Check the outbox now (a message was just queued).
pub fn wake_outbox() {
    outbox_wake().notify_one();
}

/// Send due outgoing mail (undo-send delays, Send Later) every second or when woken, reporting
/// each result as `(outbox id, subject, Ok | error)`. A sent message triggers a sync pass so it
/// shows up in Sent.
pub fn on_outbox(report: impl Fn(i64, String, Result<(), String>) + Send + Sync + 'static) {
    runtime().spawn(async move {
        loop {
            let subjects: std::collections::HashMap<i64, String> = core()
                .store()
                .outgoing_due(chrono_now_ms())
                .unwrap_or_default()
                .into_iter()
                .map(|r| (r.id, r.draft["subject"].as_str().unwrap_or_default().to_string()))
                .collect();
            let results = core().process_outbox().await;
            let sent_any = results.iter().any(|(_, r)| r.is_ok());
            for (id, r) in results {
                report(id, subjects.get(&id).cloned().unwrap_or_default(), r);
            }
            if sent_any {
                sync_now();
            }
            let _ = tokio::time::timeout(Duration::from_secs(1), outbox_wake().notified()).await;
        }
    });
}

/// Run a sync pass over every account now and then every `poll`, calling `notify` after each pass.
/// With `RUSTY_INBOX_OFFLINE` set, only the snooze clock runs (no network).
pub fn on_synced(notify: impl Fn() + Send + Sync + 'static) {
    let offline = std::env::var_os("RUSTY_INBOX_OFFLINE").is_some();
    runtime().spawn(async move {
        loop {
            if offline {
                let now = chrono_now_ms();
                if !core().store().wake_snoozed(now).unwrap_or_default().is_empty() {
                    notify();
                }
            } else {
                let accounts = core().store().accounts().unwrap_or_default();
                for a in accounts {
                    match core().sync_account(&a.email).await {
                        Ok(n) => tracing::info!("synced {}: {n} new", a.email),
                        Err(e) => tracing::warn!("sync {}: {e:#}", a.email),
                    }
                }
                notify();
            }
            let minutes = crate::settings::get(core().store(), "syncMinutes").as_u64().unwrap_or(1).max(1);
            let poll = Duration::from_secs(if offline { 60 } else { minutes * 60 });
            let _ = tokio::time::timeout(poll, wake().notified()).await;
        }
    });
}

fn chrono_now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

// ---- accounts -----------------------------------------------------------------------------

/// Accounts with their server settings (never the password), for the Settings window.
pub fn account_details(store: &Store) -> Value {
    let list: Vec<Value> = store
        .accounts()
        .unwrap_or_default()
        .into_iter()
        .map(|a| {
            json!({
                "email": a.email, "name": a.name, "type": a.account_type, "username": a.username,
                "imapHost": a.imap_host, "imapPort": a.imap_port, "imapSecurity": a.imap_security,
                "smtpHost": a.smtp_host, "smtpPort": a.smtp_port, "smtpSecurity": a.smtp_security,
            })
        })
        .collect();
    Value::Array(list)
}

fn security_of(v: &Value, default: &str) -> Result<String, String> {
    match v.as_str().unwrap_or(default) {
        s @ ("tls" | "starttls" | "none") => Ok(s.to_string()),
        other => Err(format!("unknown security \"{other}\"")),
    }
}

fn port_of(v: &Value, default: u16) -> Result<u16, String> {
    match v {
        Value::Null => Ok(default),
        Value::Number(n) => n.as_u64().and_then(|p| u16::try_from(p).ok()).filter(|p| *p > 0).ok_or("bad port".into()),
        Value::String(s) if s.trim().is_empty() => Ok(default),
        Value::String(s) => s.trim().parse::<u16>().ok().filter(|p| *p > 0).ok_or(format!("bad port \"{s}\"")),
        _ => Err("bad port".into()),
    }
}

/// Check an "add account" request. Returns (email, name, password, manual servers if given).
/// `req`: `{email, name, password, imapHost?, imapPort?, imapSecurity?, smtpHost?, smtpPort?,
/// smtpSecurity?, username?}`; with no `imapHost` the servers are detected automatically.
pub fn parse_new_account(req: &Value) -> Result<(String, String, String, Option<Account>), String> {
    let text = |k: &str| req[k].as_str().unwrap_or("").trim().to_string();
    let email = text("email").to_lowercase();
    let (local, domain) = email.split_once('@').unwrap_or(("", ""));
    if local.is_empty() || !domain.contains('.') || email.contains(' ') {
        return Err("Enter a valid email address".into());
    }
    let password = req["password"].as_str().unwrap_or("").to_string();
    if password.is_empty() {
        return Err("Enter the password".into());
    }
    let name = text("name");
    let imap_host = text("imapHost");
    if imap_host.is_empty() {
        return Ok((email, name, password, None));
    }
    let imap_security = security_of(&req["imapSecurity"], "tls")?;
    let smtp_security = security_of(&req["smtpSecurity"], "tls")?;
    let smtp_host = Some(text("smtpHost")).filter(|h| !h.is_empty()).unwrap_or_else(|| imap_host.clone());
    let account = Account {
        email: email.clone(),
        name: name.clone(),
        account_type: rusty_inbox_core::mail::detect_account_type(&email).to_string(),
        imap_port: port_of(&req["imapPort"], if imap_security == "tls" { 993 } else { 143 })?,
        imap_host,
        imap_security,
        smtp_port: port_of(&req["smtpPort"], if smtp_security == "tls" { 465 } else { 587 })?,
        smtp_host,
        smtp_security,
        username: Some(text("username")).filter(|u| !u.is_empty()).unwrap_or_else(|| email.clone()),
    };
    Ok((email, name, password, Some(account)))
}

/// Verify the login against the server and save the account; the next sync pass fetches its mail.
pub async fn add_account(req: Value) -> Result<String, String> {
    let (email, name, password, manual) = parse_new_account(&req)?;
    if std::env::var_os("RUSTY_INBOX_OFFLINE").is_some() {
        return Err("Offline mode: accounts cannot be verified".into());
    }
    let friendly = |m: String| {
        tracing::warn!("add account {email}: {m}");
        if m.contains("AUTHENTICATIONFAILED")
            || m.to_lowercase().contains("login failed")
            || m.contains("Invalid credentials")
        {
            "The server rejected the email or password".to_string()
        } else {
            format!("Could not connect: {m}")
        }
    };
    let account = match manual {
        Some(a) => {
            let mut c = rusty_inbox_core::mail::imap::Connection::connect(&a, &password)
                .await
                .map_err(|e| friendly(format!("{e:#}")))?;
            let _ = c.logout().await;
            a
        }
        None => rusty_inbox_core::mail::autodetect(&email, &name, &password)
            .await
            .map_err(|e| friendly(format!("{e:#}")))?,
    };
    let store = core().store();
    store.upsert_account(&account).map_err(|e| e.to_string())?;
    store.set_secret(&format!("password:{email}"), &password).map_err(|e| e.to_string())?;
    sync_now();
    Ok(email)
}

/// Accounts found in Thunderbird profiles (no passwords), each with `exists` (already added here)
/// and `hasPassword` (Thunderbird's saved password can be brought over).
pub fn thunderbird_of(store: &Store) -> Value {
    let have: Vec<String> = store.accounts().unwrap_or_default().into_iter().map(|a| a.email).collect();
    let list: Vec<Value> = rusty_inbox_core::thunderbird::find_accounts()
        .into_iter()
        .map(|a| {
            let mut v = serde_json::to_value(&a).unwrap_or(Value::Null);
            v["exists"] = json!(have.contains(&a.email));
            v
        })
        .collect();
    Value::Array(list)
}

/// The add-account request for a Thunderbird account (servers as Thunderbird has them).
pub fn thunderbird_request(a: &rusty_inbox_core::thunderbird::TbAccount, password: &str) -> Value {
    json!({
        "email": a.email, "name": a.name, "password": password, "username": a.username,
        "imapHost": a.imap_host, "imapPort": a.imap_port, "imapSecurity": a.imap_security,
        "smtpHost": a.smtp_host, "smtpPort": a.smtp_port, "smtpSecurity": a.smtp_security,
    })
}

/// Import the chosen Thunderbird accounts one after another (mail servers throttle bursts of
/// logins), with Thunderbird's saved password or the one typed in `passwords`.
pub async fn import_thunderbird(
    emails: Vec<String>,
    passwords: std::collections::HashMap<String, String>,
    report: impl Fn(String, Result<(), String>),
) {
    let found = rusty_inbox_core::thunderbird::find_accounts();
    for (i, email) in emails.iter().enumerate() {
        let Some(a) = found.iter().find(|a| &a.email == email) else {
            report(email.clone(), Err("not found in Thunderbird".into()));
            continue;
        };
        let typed = passwords.get(email).filter(|p| !p.is_empty()).cloned();
        let Some(pw) = typed.or_else(|| a.password.clone()) else {
            report(
                email.clone(),
                Err(if a.oauth {
                    "Thunderbird signs in with Google or Microsoft here. Enter an app password.".into()
                } else {
                    "Enter the password".into()
                }),
            );
            continue;
        };
        if i > 0 {
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
        let r = add_account(thunderbird_request(a, &pw)).await.map(|_| ());
        report(email.clone(), r);
    }
}

pub fn remove_account(email: &str) -> Result<(), String> {
    core().store().remove_account(email).map_err(|e| e.to_string())
}

pub fn rename_account(email: &str, name: &str) -> Result<(), String> {
    let store = core().store();
    let mut a = store.account(email).map_err(|e| e.to_string())?.ok_or("no such account")?;
    a.name = name.trim().to_string();
    store.upsert_account(&a).map_err(|e| e.to_string())
}

// ---- new-mail notifications ---------------------------------------------------------------

/// A desktop notification for new mail.
#[derive(Debug, Clone, PartialEq)]
pub struct NewMail {
    pub thread_id: i64,
    pub title: String,
    pub subtitle: String,
    pub body: String,
}

impl From<rusty_inbox_core::NewMail> for NewMail {
    fn from(n: rusty_inbox_core::NewMail) -> Self {
        NewMail { thread_id: n.thread_id, title: n.from, subtitle: n.subject, body: n.preview }
    }
}

/// Route the engine's new-mail reports to `show` (called on an engine thread).
pub fn on_new_mail(show: impl Fn(NewMail) + Send + Sync + 'static) {
    core().on_new_mail(move |n| show(n.into()));
}

// ---- actions ------------------------------------------------------------------------------

/// Actions that move mail out of the current list. The UI hides the rows at once and commits
/// after the undo window, because once moved the messages are gone from the local store until
/// the destination folder syncs (so an inverse action could not bring them back right away).
pub fn is_deferred(action: &str) -> bool {
    matches!(action, "done" | "archive" | "moveToTrash" | "markAsSpam" | "permanentDelete")
}

/// Inverse of an immediate action, for undo. Deferred actions are undone by cancelling them.
pub fn inverse_of(action: &str) -> Option<&'static str> {
    Some(match action {
        "pin" => "unpin",
        "unpin" => "pin",
        "markAsSeen" => "markAsUnseen",
        "markAsUnseen" => "markAsSeen",
        "snooze" => "unsnooze",
        "unsnooze" => "snooze",
        "undone" => "done",
        "moveToInbox" => "archive",
        _ => return None,
    })
}

// ---- lists --------------------------------------------------------------------------------

/// What the thread list shows, as sent from QML:
/// `{"list":"inbox","account":"a@x"|null,"category":"personal"|null,"folder":12|null}`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ListSpec {
    pub list: String,
    pub account: Option<String>,
    pub category: Option<String>,
    pub folder: Option<i64>,
}

impl ListSpec {
    pub fn parse(json: &str) -> ListSpec {
        let v: Value = serde_json::from_str(json).unwrap_or(Value::Null);
        // plain list names ("inbox") are accepted too
        if let Some(s) = v.as_str().or_else(|| if v.is_null() { Some(json) } else { None }) {
            return ListSpec { list: s.to_string(), ..Default::default() };
        }
        let opt = |k: &str| v[k].as_str().filter(|s| !s.is_empty()).map(str::to_string);
        ListSpec {
            list: opt("list").unwrap_or_else(|| "inbox".into()),
            account: opt("account"),
            category: opt("category"),
            folder: v["folder"].as_i64(),
        }
    }

    pub fn filter(&self) -> ListFilter {
        if let Some(id) = self.folder {
            return ListFilter::Folder(id);
        }
        let account = self.account.clone();
        match self.list.as_str() {
            "done" => ListFilter::Inbox { account, done: true, category: None },
            "pinned" => ListFilter::Pinned,
            "snoozed" => ListFilter::Snoozed { account },
            "sent" | "drafts" | "trash" | "spam" | "archive" => ListFilter::Role { account, role: self.list.clone() },
            _ => ListFilter::Inbox { account, done: false, category: self.category.clone() },
        }
    }
}

pub fn filter_for(list: &str) -> ListFilter {
    ListSpec::parse(list).filter()
}

pub fn accounts_of(store: &Store) -> Value {
    let list: Vec<Value> =
        store.accounts().unwrap_or_default().into_iter().map(|a| json!({ "email": a.email, "name": a.name })).collect();
    Value::Array(list)
}

/// Smart-inbox categories in display order (engine names).
pub const CATEGORIES: [&str; 4] = ["personal", "notifications", "newsletters", "invitations"];

/// Everything the sidebar shows: unread counts per list and category, and each account with its
/// own (non-system) folders.
pub fn sidebar_of(store: &Store) -> Value {
    let c = store.sidebar_counts().unwrap_or_default();
    let categories: Vec<Value> = CATEGORIES
        .iter()
        .map(|k| {
            let (unread, total) = c.categories.get(*k).copied().unwrap_or_default();
            json!({ "key": k, "unread": unread, "total": total })
        })
        .collect();
    let accounts: Vec<Value> = store
        .accounts()
        .unwrap_or_default()
        .into_iter()
        .map(|a| {
            let mut folders: Vec<Value> = store
                .folders(Some(&a.email))
                .unwrap_or_default()
                .into_iter()
                .filter(|f| f.role.is_none())
                .map(|f| json!({ "id": f.id, "name": f.name, "path": f.path }))
                .collect();
            folders.sort_by_key(|f| f["path"].as_str().unwrap_or_default().to_lowercase());
            json!({
                "email": a.email,
                "name": a.name,
                "unread": c.account_unread.get(&a.email).copied().unwrap_or(0),
                "folders": folders,
            })
        })
        .collect();
    json!({
        "inbox": c.inbox_unread,
        "pinned": c.pinned,
        "snoozed": c.snoozed,
        "drafts": c.drafts,
        "scheduled": store.outgoing_in("queued").map(|v| v.len()).unwrap_or(0)
            + store.outgoing_in("failed").map(|v| v.len()).unwrap_or(0),
        "categories": categories,
        "accounts": accounts,
    })
}

pub fn thread_json(t: &ThreadRow) -> Value {
    let (name, addr) = &t.last_from;
    json!({
        "id": t.thread_id,
        "account": t.account,
        "subject": t.subject,
        "preview": t.preview,
        "date": t.last_ms,
        "count": t.message_count,
        "unread": t.unseen,
        "flagged": t.flagged,
        "pinned": t.pinned,
        "done": t.done,
        "attachments": t.has_attachments,
        "category": t.category,
        "from": name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| addr.clone()),
        "fromAddr": addr,
    })
}

pub fn threads_of(store: &Store, spec: &str, limit: usize) -> Value {
    if ListSpec::parse(spec).list == "scheduled" {
        return scheduled_of(store, ListSpec::parse(spec).account.as_deref());
    }
    let rows = store.thread_rows(&filter_for(spec), limit).unwrap_or_default();
    Value::Array(rows.iter().map(thread_json).collect())
}

/// Messages waiting to go out (Send Later) or that failed to send, shaped like list rows. Their
/// ids are negative (`-outbox id`) so the list can tell them from conversations.
pub fn scheduled_of(store: &Store, account: Option<&str>) -> Value {
    let mut rows = store.outgoing_in("queued").unwrap_or_default();
    rows.extend(store.outgoing_in("failed").unwrap_or_default());
    let list: Vec<Value> = rows
        .into_iter()
        .filter(|r| account.is_none_or(|a| a == r.account))
        .map(|r| {
            let d = Draft::from_json(&r.draft);
            let to: Vec<String> = d
                .recipients()
                .map(|a| a.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| a.address.clone()))
                .collect();
            let preview =
                rusty_inbox_core::compose::html_to_text(&d.html).split_whitespace().collect::<Vec<_>>().join(" ");
            json!({
                "id": -r.id,
                "account": r.account,
                "subject": d.subject,
                "preview": match (&r.error, r.state.as_str()) {
                    (Some(e), "failed") => format!("Not sent: {e}"),
                    (Some(e), _) => format!("Retrying soon ({e})"),
                    _ => preview.chars().take(160).collect(),
                },
                "date": r.send_at.unwrap_or(r.updated_ms),
                "count": 1,
                "unread": r.state == "failed",
                "flagged": false,
                "pinned": false,
                "done": false,
                "attachments": !d.attachments.is_empty(),
                "category": "personal",
                "from": format!("To: {}", if to.is_empty() { "(no recipients)".to_string() } else { to.join(", ") }),
                "fromAddr": d.to.first().map(|a| a.address.clone()).unwrap_or_default(),
                "scheduled": true,
                "failed": r.state == "failed",
            })
        })
        .collect();
    Value::Array(list)
}

/// A composer draft as JSON: `kind` is `new`, `reply`, `replyAll`, `forward` (of `message_id`),
/// or `draft` (edit the stored draft `message_id`). `account` picks the sender of a new message.
pub fn compose_of(store: &Store, kind: &str, message_id: i64, account: &str) -> Value {
    let accounts = store.accounts().unwrap_or_default();
    let mine: Vec<String> = accounts.iter().map(|a| a.email.clone()).collect();
    let pick = |a: &str| -> String {
        if mine.iter().any(|m| m == a) {
            a.to_string()
        } else {
            mine.first().cloned().unwrap_or_default()
        }
    };
    if kind.get(..7).is_some_and(|p| p.eq_ignore_ascii_case("mailto:")) {
        let acc = pick(account);
        let mut d = rusty_inbox_core::compose::new_draft(store, &acc, &crate::settings::signature(store, &acc));
        rusty_inbox_core::compose::apply_mailto(&mut d, kind);
        return d.to_json();
    }
    let d = match kind {
        "reply" | "replyAll" | "forward" => {
            let acc = store.message(message_id).ok().flatten().map(|(_, m)| m.account).unwrap_or_default();
            let sig = crate::settings::signature(store, &acc);
            rusty_inbox_core::compose::respond(store, message_id, kind, &mine, &sig)
        }
        "draft" => rusty_inbox_core::compose::from_stored_draft(store, message_id),
        _ => None,
    };
    let d = d.unwrap_or_else(|| {
        let acc = pick(account);
        rusty_inbox_core::compose::new_draft(store, &acc, &crate::settings::signature(store, &acc))
    });
    d.to_json()
}

/// Address suggestions for the recipient fields.
pub fn contacts_of(store: &Store, prefix: &str) -> Value {
    let v: Vec<Value> = store
        .contacts(prefix, 8)
        .unwrap_or_default()
        .into_iter()
        .map(|(name, address, n)| json!({ "name": name, "address": address, "count": n }))
        .collect();
    Value::Array(v)
}

/// `{name, mime, size, path}` of a local file picked as an attachment (`file://` URL or path).
pub fn file_info(url: &str) -> Value {
    let path = url.strip_prefix("file://").unwrap_or(url);
    let path = percent_decode(path);
    match std::fs::metadata(&path) {
        Ok(m) if m.is_file() => {
            let name =
                std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            json!({ "name": name, "mime": rusty_inbox_core::compose::mime_for(&name), "size": m.len(), "path": path })
        }
        _ => Value::Null,
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

pub fn message_json(m: &MessageRow) -> Value {
    json!({
        "id": m.id,
        "subject": m.subject,
        "from": m.from_name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| m.from_addr.clone()),
        "fromAddr": m.from_addr,
        "to": m.to,
        "cc": m.cc,
        "date": m.date_ms,
        "html": m.body_html,
        "text": m.body_text,
        "fetched": m.body_fetched,
        "folderRole": m.folder_role,
        "attachments": m.attachments.iter().map(|(id, name, mime, size, inline)| {
            json!({ "id": id, "name": name, "mime": mime, "size": size, "inline": inline })
        }).collect::<Vec<_>>(),
    })
}

pub fn messages_of(store: &Store, thread_id: i64) -> Value {
    let msgs = store.thread_messages(thread_id).unwrap_or_default();
    Value::Array(msgs.iter().map(message_json).collect())
}

/// Parse a JSON array of thread ids sent from QML.
pub fn ids_of(json: &str) -> Vec<i64> {
    serde_json::from_str::<Vec<i64>>(json).unwrap_or_default()
}

// ---- live wrappers ------------------------------------------------------------------------

pub fn sidebar_json() -> Value {
    sidebar_of(core().store())
}

pub fn threads_json(spec: &str, limit: usize) -> Value {
    threads_of(core().store(), spec, limit)
}

pub fn messages_json(thread_id: i64) -> Value {
    messages_of(core().store(), thread_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusty_inbox_core::store::NewMessage;

    fn seeded() -> Store {
        let s = Store::in_memory().unwrap();
        s.upsert_account(&Account {
            email: "u@example.com".into(),
            name: "U".into(),
            account_type: "customIMAP".into(),
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            imap_security: "tls".into(),
            smtp_host: "smtp.example.com".into(),
            smtp_port: 587,
            smtp_security: "starttls".into(),
            username: "u@example.com".into(),
        })
        .unwrap();
        let inbox = s.upsert_folder("u@example.com", "INBOX", "Inbox", Some("/"), Some("inbox"), true).unwrap();
        s.upsert_folder("u@example.com", "Projects/Alpha", "Alpha", Some("/"), None, true).unwrap();
        for (uid, cat) in [(1u32, "personal"), (2, "notifications"), (3, "notifications")] {
            s.insert_message(&NewMessage {
                account: "u@example.com".into(),
                folder_id: inbox,
                uid,
                message_id: Some(format!("<{uid}@x>")),
                subject: format!("S{uid}"),
                from_addr: format!("s{uid}@example.org"),
                to: json!([]),
                cc: json!([]),
                bcc: json!([]),
                reply_to: json!([]),
                date_ms: uid as i64 * 1000,
                category: cat.into(),
                ..Default::default()
            })
            .unwrap();
        }
        s
    }

    #[test]
    fn list_specs_map_to_store_filters() {
        assert!(matches!(filter_for("inbox"), ListFilter::Inbox { done: false, .. }));
        assert!(matches!(filter_for("done"), ListFilter::Inbox { done: true, .. }));
        assert!(matches!(filter_for("pinned"), ListFilter::Pinned));
        assert!(matches!(filter_for("snoozed"), ListFilter::Snoozed { account: None }));
        assert!(matches!(filter_for("sent"), ListFilter::Role { ref role, .. } if role == "sent"));
        assert!(matches!(filter_for("whatever"), ListFilter::Inbox { done: false, .. }));
        let f = filter_for(r#"{"list":"inbox","account":"u@example.com","category":"notifications"}"#);
        assert!(matches!(f, ListFilter::Inbox { account: Some(ref a), category: Some(ref c), done: false }
            if a == "u@example.com" && c == "notifications"));
        assert!(matches!(filter_for(r#"{"list":"inbox","folder":12}"#), ListFilter::Folder(12)));
        assert!(matches!(filter_for(r#"{"list":"sent","account":""}"#), ListFilter::Role { account: None, .. }));
    }

    #[test]
    fn categories_filter_threads() {
        let s = seeded();
        assert_eq!(threads_of(&s, "inbox", 50).as_array().unwrap().len(), 3);
        assert_eq!(threads_of(&s, r#"{"list":"inbox","category":"notifications"}"#, 50).as_array().unwrap().len(), 2);
        assert_eq!(threads_of(&s, r#"{"list":"inbox","category":"personal"}"#, 50).as_array().unwrap().len(), 1);
    }

    #[test]
    fn sidebar_counts_and_custom_folders() {
        let s = seeded();
        let v = sidebar_of(&s);
        assert_eq!(v["inbox"], 3);
        assert_eq!(v["snoozed"], 0);
        let notif = v["categories"].as_array().unwrap().iter().find(|c| c["key"] == "notifications").unwrap();
        assert_eq!(notif["unread"], 2);
        let acc = &v["accounts"][0];
        assert_eq!(acc["email"], "u@example.com");
        assert_eq!(acc["unread"], 3);
        let folders = acc["folders"].as_array().unwrap();
        assert_eq!(folders.len(), 1, "system folders are not listed as custom folders");
        assert_eq!(folders[0]["name"], "Alpha");
    }

    #[test]
    fn undo_rules() {
        for a in ["done", "archive", "moveToTrash", "markAsSpam", "permanentDelete"] {
            assert!(is_deferred(a), "{a}");
            assert_eq!(inverse_of(a), None, "{a}");
        }
        for (a, b) in [("pin", "unpin"), ("markAsSeen", "markAsUnseen"), ("snooze", "unsnooze")] {
            assert!(!is_deferred(a));
            assert_eq!(inverse_of(a), Some(b));
            assert_eq!(inverse_of(b), Some(a));
        }
    }

    #[test]
    fn thunderbird_requests_are_valid_add_requests() {
        let a = rusty_inbox_core::thunderbird::TbAccount {
            email: "ann@example.com".into(),
            name: "Ann".into(),
            username: "ann".into(),
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            imap_security: "tls".into(),
            smtp_host: "smtp.example.com".into(),
            smtp_port: 587,
            smtp_security: "starttls".into(),
            oauth: false,
            password: None,
            has_password: false,
            profile: "p".into(),
        };
        let (email, _, pw, acct) = parse_new_account(&thunderbird_request(&a, "pw")).unwrap();
        let acct = acct.unwrap();
        assert_eq!((email.as_str(), pw.as_str(), acct.username.as_str()), ("ann@example.com", "pw", "ann"));
        assert_eq!((acct.smtp_port, acct.smtp_security.as_str()), (587, "starttls"));
    }

    #[test]
    fn compose_and_scheduled_rows() {
        let s = seeded();
        let d = Draft::from_json(&compose_of(&s, "new", 0, "nobody@example.com"));
        assert_eq!(d.account, "u@example.com", "falls back to the first account");
        assert!(threads_of(&s, r#"{"list":"scheduled"}"#, 50).as_array().unwrap().is_empty());
        let mut d2 = d.clone();
        d2.to.push(rusty_inbox_core::compose::Addr { name: Some("Bo".into()), address: "bo@example.org".into() });
        d2.subject = "Later".into();
        let id = s.put_outgoing(0, &d2.account, "queued", &d2.to_json(), Some(99), None).unwrap();
        let rows = threads_of(&s, r#"{"list":"scheduled"}"#, 50);
        assert_eq!(rows[0]["id"], -id);
        assert_eq!(rows[0]["from"], "To: Bo");
        assert_eq!(sidebar_of(&s)["scheduled"], 1);
        assert_eq!(file_info("/definitely/not/here"), Value::Null);
        let m = Draft::from_json(&compose_of(&s, "mailto:x@example.org?subject=Hey", 0, ""));
        assert_eq!((m.to[0].address.as_str(), m.subject.as_str(), m.kind.as_str()), ("x@example.org", "Hey", "new"));
        let f = file_info(&format!("file://{}", env!("CARGO_MANIFEST_DIR").replace(' ', "%20") + "/Cargo.toml"));
        assert_eq!(f["name"], "Cargo.toml");
    }

    #[test]
    fn new_mail_becomes_a_notification() {
        let n: NewMail = rusty_inbox_core::NewMail {
            thread_id: 42,
            account: "me@example.com".into(),
            from: "Alice".into(),
            subject: "Hello".into(),
            preview: "Body text".into(),
        }
        .into();
        assert_eq!(
            n,
            NewMail { thread_id: 42, title: "Alice".into(), subtitle: "Hello".into(), body: "Body text".into() }
        );
    }

    #[test]
    fn new_account_requests_are_checked() {
        let ok = parse_new_account(&json!({"email": " Me@Example.com ", "password": "pw", "name": "Me"})).unwrap();
        assert_eq!((ok.0.as_str(), ok.1.as_str(), ok.3.is_none()), ("me@example.com", "Me", true));
        assert!(parse_new_account(&json!({"email": "nope", "password": "pw"})).is_err());
        assert!(parse_new_account(&json!({"email": "a@b.com", "password": ""})).is_err());
        let m = parse_new_account(&json!({
            "email": "a@b.com", "password": "pw", "imapHost": "mail.b.com", "imapSecurity": "starttls",
            "smtpPort": "587", "smtpSecurity": "starttls"
        }))
        .unwrap()
        .3
        .unwrap();
        assert_eq!((m.imap_host.as_str(), m.imap_port, m.imap_security.as_str()), ("mail.b.com", 143, "starttls"));
        assert_eq!((m.smtp_host.as_str(), m.smtp_port, m.username.as_str()), ("mail.b.com", 587, "a@b.com"));
        assert!(
            parse_new_account(&json!({"email": "a@b.com", "password": "p", "imapHost": "h", "imapPort": "x"})).is_err()
        );
        assert!(parse_new_account(
            &json!({"email": "a@b.com", "password": "p", "imapHost": "h", "imapSecurity": "ssl3"})
        )
        .is_err());
    }

    #[test]
    fn account_details_never_include_secrets() {
        let s = seeded();
        s.set_secret("password:u@example.com", "hunter2").unwrap();
        let v = account_details(&s);
        assert_eq!(v[0]["imapHost"], "imap.example.com");
        assert!(!v.to_string().contains("hunter2"));
    }

    #[test]
    fn ids_parse_leniently() {
        assert_eq!(ids_of("[1,2,3]"), vec![1, 2, 3]);
        assert_eq!(ids_of("nonsense"), Vec::<i64>::new());
    }

    #[test]
    fn thread_sender_falls_back_to_address() {
        let t = ThreadRow {
            thread_id: 7,
            account: "a@x".into(),
            subject: "Hi".into(),
            last_ms: 1,
            message_count: 2,
            unseen: true,
            flagged: false,
            has_attachments: false,
            pinned: false,
            done: false,
            preview: "p".into(),
            participants: vec![],
            last_from: (Some("  ".into()), "b@y".into()),
            category: "people".into(),
        };
        let v = thread_json(&t);
        assert_eq!(v["from"], "b@y");
        assert_eq!(v["id"], 7);
        assert_eq!(v["unread"], true);
    }

    #[test]
    fn empty_store_gives_empty_lists() {
        let s = Store::in_memory().unwrap();
        assert_eq!(accounts_of(&s), json!([]));
        assert_eq!(threads_of(&s, "inbox", 50), json!([]));
        assert_eq!(messages_of(&s, 1), json!([]));
        assert_eq!(sidebar_of(&s)["accounts"], json!([]));
    }
}
