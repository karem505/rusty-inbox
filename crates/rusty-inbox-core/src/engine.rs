//! The engine: account sync, thread actions, on-demand bodies and new-mail callbacks.
//!
//! Local state changes first (the UI reads the [`Store`] directly), then the change is mirrored to
//! the IMAP server. Without network (`EngineConfig::network = false`) every action stays local.

use crate::compose::{self, Draft};
use crate::mail::{self, imap};
use crate::store::{ListFilter, Store};
use crate::CoreResult;
use chrono::Utc;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Where the engine keeps its files, and whether it may touch the network.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub databases_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub network: bool,
}

impl EngineConfig {
    /// `<dir>/databases` for SQLite, `<dir>/cache` for downloaded parts.
    pub fn for_dir(dir: &Path) -> EngineConfig {
        EngineConfig { databases_dir: dir.join("databases"), cache_dir: dir.join("cache"), network: true }
    }
}

/// A conversation that received new unread mail during a sync pass.
#[derive(Debug, Clone, PartialEq)]
pub struct NewMail {
    pub thread_id: i64,
    pub account: String,
    pub from: String,
    pub subject: String,
    pub preview: String,
}

type NewMailFn = dyn Fn(NewMail) + Send + Sync;

/// Automatic retries of a message whose sending hit a network error.
pub const MAX_SEND_RETRIES: i64 = 3;

/// Whether a send error is worth retrying by itself (connection trouble, not a rejection).
pub fn transient(error: &str) -> bool {
    let e = error.to_ascii_lowercase();
    ["timed out", "timeout", "connection", "connect", "temporarily", "try again", "network", "dns", "broken pipe"]
        .iter()
        .any(|k| e.contains(k))
        && !e.contains("authentication")
}

struct Inner {
    cfg: EngineConfig,
    store: Store,
    /// newest message time seen per account; mail newer than this after a pass is "new"
    high_water: Mutex<HashMap<String, i64>>,
    on_new_mail: Mutex<Option<Arc<NewMailFn>>>,
    /// open IMAP connections per account, reused between operations: mail servers (cPanel's
    /// firewall in particular) drop clients that open several connections in a short time
    pool: Mutex<HashMap<String, Vec<imap::Connection>>>,
}

#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

impl Engine {
    pub fn new(cfg: EngineConfig) -> CoreResult<Engine> {
        std::fs::create_dir_all(&cfg.cache_dir).map_err(|e| crate::CoreError::Message(e.to_string()))?;
        let store = Store::open(&cfg.databases_dir)?;
        Ok(Engine {
            inner: Arc::new(Inner {
                cfg,
                store,
                high_water: Mutex::new(HashMap::new()),
                on_new_mail: Mutex::new(None),
                pool: Mutex::new(HashMap::new()),
            }),
        })
    }

    pub fn store(&self) -> &Store {
        &self.inner.store
    }

    pub fn network(&self) -> bool {
        self.inner.cfg.network
    }

    /// Called (on an engine thread) for each conversation with new unread mail after a sync pass.
    /// The first pass of an account after start-up never reports, so old mail does not ring.
    pub fn on_new_mail(&self, f: impl Fn(NewMail) + Send + Sync + 'static) {
        *self.inner.on_new_mail.lock() = Some(Arc::new(f));
    }

    // ---------------------------------------------------------------------------------------
    // thread actions
    // ---------------------------------------------------------------------------------------

    /// Apply an action to threads: `markAsSeen`, `markAsUnseen`, `pin`, `unpin`, `done`,
    /// `undone`, `snooze` (needs `until_ms`), `unsnooze`, `archive`, `moveToTrash`, `markAsSpam`,
    /// `moveToInbox`, `permanentDelete`. Returns false for an unknown action.
    pub async fn thread_action(&self, action: &str, tids: &[i64], until_ms: Option<i64>) -> bool {
        let store = &self.inner.store;
        match action {
            "markAsSeen" => self.set_seen(tids, true).await,
            "markAsUnseen" => self.set_seen(tids, false).await,
            "pin" | "unpin" => {
                let on = action == "pin";
                for t in tids {
                    let _ = store.set_thread_flag(*t, "pinned", on);
                }
                self.remote_flags(tids, if on { "+" } else { "-" }, "\\Flagged").await;
            }
            "done" | "undone" => {
                // On IMAP, Done archives the conversation; Undone brings it back to the inbox.
                for t in tids {
                    let _ = store.set_thread_flag(*t, "done", action == "done");
                }
                self.move_threads(tids, if action == "done" { "archive" } else { "inbox" }).await;
            }
            "snooze" | "unsnooze" => {
                // local only: IMAP has no snooze, the conversation stays in the server inbox
                let until = if action == "snooze" { until_ms } else { None };
                if action == "snooze" && until.is_none() {
                    return false;
                }
                for t in tids {
                    let _ = store.set_snooze(*t, until);
                }
            }
            "archive" => self.move_threads(tids, "archive").await,
            "moveToTrash" => self.move_threads(tids, "trash").await,
            "markAsSpam" => self.move_threads(tids, "spam").await,
            "moveToInbox" => self.move_threads(tids, "inbox").await,
            "permanentDelete" => {
                for t in tids {
                    for (acc, path, uids) in store.thread_folder_uids(*t).unwrap_or_default() {
                        if let Some(mut c) = self.connect(&acc).await {
                            let _ = c.delete_permanently(&path, &uids).await;
                            self.release(c).await;
                        }
                    }
                    let _ = store.delete_thread_messages(*t);
                }
            }
            _ => return false,
        }
        true
    }

    async fn connect(&self, email: &str) -> Option<imap::Connection> {
        match self.checkout(email).await {
            Ok(c) => Some(c),
            Err(e) => {
                tracing::warn!("connect {email}: {e:#}");
                None
            }
        }
    }

    /// An IMAP connection for `email`: a pooled one that still answers, else a new login.
    async fn checkout(&self, email: &str) -> anyhow::Result<imap::Connection> {
        if !self.inner.cfg.network {
            anyhow::bail!("offline");
        }
        loop {
            let pooled = self.inner.pool.lock().get_mut(email).and_then(|v| v.pop());
            let Some(mut c) = pooled else { break };
            // servers close idle sessions after ~30 minutes
            if c.idle_since.elapsed() < std::time::Duration::from_secs(20 * 60) && c.noop().await {
                return Ok(c);
            }
        }
        let store = &self.inner.store;
        let acct = store.account(email)?.ok_or_else(|| anyhow::anyhow!("no account {email}"))?;
        let pw = store.secret(&format!("password:{email}"))?.ok_or_else(|| anyhow::anyhow!("no password"))?;
        imap::Connection::connect(&acct, &pw).await
    }

    /// Hand a connection back for reuse (at most two idle per account; extra ones log out).
    async fn release(&self, mut c: imap::Connection) {
        c.idle_since = std::time::Instant::now();
        let extra = {
            let mut pool = self.inner.pool.lock();
            let v = pool.entry(c.account.clone()).or_default();
            if v.len() < 2 {
                v.push(c);
                None
            } else {
                Some(c)
            }
        };
        if let Some(mut c) = extra {
            let _ = c.logout().await;
        }
    }

    /// Log out of every pooled connection (account removed, app quitting).
    pub async fn close_connections(&self, email: Option<&str>) {
        let conns: Vec<imap::Connection> = {
            let mut pool = self.inner.pool.lock();
            match email {
                Some(e) => pool.remove(e).unwrap_or_default(),
                None => pool.drain().flat_map(|(_, v)| v).collect(),
            }
        };
        for mut c in conns {
            let _ = c.logout().await;
        }
    }

    async fn set_seen(&self, tids: &[i64], seen: bool) {
        let store = &self.inner.store;
        let mut changed = false;
        for t in tids {
            if store.thread_messages(*t).map(|m| m.iter().any(|x| x.seen != seen)).unwrap_or(false) {
                let _ = store.set_thread_seen(*t, seen);
                changed = true;
            }
        }
        if changed {
            self.remote_flags(tids, if seen { "+" } else { "-" }, "\\Seen").await;
        }
    }

    async fn remote_flags(&self, tids: &[i64], op: &str, flag: &str) {
        let mut by_acc: HashMap<String, Vec<(String, Vec<u32>)>> = HashMap::new();
        for t in tids {
            for (acc, path, uids) in self.inner.store.thread_folder_uids(*t).unwrap_or_default() {
                by_acc.entry(acc).or_default().push((path, uids));
            }
        }
        for (acc, list) in by_acc {
            if let Some(mut c) = self.connect(&acc).await {
                for (path, uids) in list {
                    if let Err(e) = c.store_flags(&path, &uids, op, flag).await {
                        tracing::warn!("store flags {path}: {e:#}");
                    }
                }
                self.release(c).await;
            }
        }
    }

    /// Move threads to the account folder with `role`. Locally the messages leave at once; the
    /// next sync pass brings them back from their new folder.
    async fn move_threads(&self, tids: &[i64], role: &str) {
        let store = &self.inner.store;
        for t in tids {
            for (acc, path, uids) in store.thread_folder_uids(*t).unwrap_or_default() {
                let Some(dest) = store.folder_by_role(&acc, role).ok().flatten() else { continue };
                if dest.path == path {
                    continue;
                }
                if let Some(mut c) = self.connect(&acc).await {
                    if let Err(e) = c.move_uids(&path, &uids, &dest.path).await {
                        tracing::warn!("move {path} -> {}: {e:#}", dest.path);
                    }
                    self.release(c).await;
                }
            }
            let _ = store.delete_thread_messages(*t);
        }
    }

    // ---------------------------------------------------------------------------------------
    // bodies
    // ---------------------------------------------------------------------------------------

    /// Download full bodies for the thread's messages that were synced header-only (or whose
    /// stored html still has unresolved `cid:` images). Returns whether anything was missing.
    pub async fn fetch_missing_bodies(&self, tid: i64) -> bool {
        let store = &self.inner.store;
        let msgs = store.thread_messages(tid).unwrap_or_default();
        let missing: Vec<i64> = msgs
            .iter()
            .filter(|m| !m.body_fetched || m.body_html.as_deref().is_some_and(has_cid_refs))
            .map(|m| m.id)
            .collect();
        if missing.is_empty() {
            return false;
        }
        for mid in missing {
            let Some((acc, path, uid)) = store.message_location(mid).ok().flatten() else { continue };
            let Some(mut c) = self.connect(&acc).await else { continue };
            if let Ok(Some(raw)) = c.fetch_full(&path, uid).await {
                if let Some(parsed) = mail::mime::parse(&raw, true) {
                    let html = parsed.body_html.as_deref().map(|h| self.resolve_inline_images(mid, h, &raw));
                    let _ = store.set_body(mid, parsed.body_text.as_deref(), html.as_deref());
                }
            }
            self.release(c).await;
        }
        true
    }

    /// Replace the html's `cid:` references with `data:` URLs of the message's inline parts
    /// (the parts are also written to the cache).
    fn resolve_inline_images(&self, mid: i64, html: &str, raw: &[u8]) -> String {
        use base64::Engine as _;
        if !has_cid_refs(html) {
            return html.to_string();
        }
        let dir = self.inner.cfg.cache_dir.join("messagesData").join(mid.to_string()).join("inline");
        let mut urls = vec![];
        for (n, (cid, mime, bytes)) in mail::mime::inline_parts(raw).into_iter().enumerate() {
            let ext = mime.rsplit('/').next().unwrap_or("bin").split(['+', ';']).next().unwrap_or("bin").to_string();
            if std::fs::create_dir_all(&dir).is_ok() {
                let _ = std::fs::write(dir.join(format!("{n}.{ext}")), &bytes);
            }
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            urls.push((cid, format!("data:{mime};base64,{b64}")));
        }
        mail::mime::rewrite_cids(html, &urls)
    }

    // ---------------------------------------------------------------------------------------
    // composing and sending
    // ---------------------------------------------------------------------------------------

    /// Read a draft's attachments: local files, and attachments of stored messages (forwarding),
    /// which are downloaded from the server.
    async fn attachment_files(&self, d: &Draft) -> anyhow::Result<Vec<(String, String, Vec<u8>)>> {
        let mut out = vec![];
        for a in &d.attachments {
            let mime = if a.mime.is_empty() { compose::mime_for(&a.name).to_string() } else { a.mime.clone() };
            if let Some(path) = &a.path {
                let bytes = std::fs::read(path).map_err(|e| anyhow::anyhow!("{}: {e}", a.name))?;
                out.push((a.name.clone(), mime, bytes));
            } else if let Some(aid) = a.from_attachment {
                let store = &self.inner.store;
                let loc = store.attachment_location(aid).ok().flatten();
                let Some((acc, path, uid, ..)) = loc else { anyhow::bail!("{}: original message is gone", a.name) };
                let index = store.attachment_index(aid).unwrap_or(0);
                let mut c = self.connect(&acc).await.ok_or_else(|| anyhow::anyhow!("{}: offline", a.name))?;
                let raw =
                    c.fetch_full(&path, uid).await?.ok_or_else(|| anyhow::anyhow!("{}: not on the server", a.name))?;
                self.release(c).await;
                let (_n, m, bytes) = mail::mime::attachment_bytes(&raw, index)
                    .ok_or_else(|| anyhow::anyhow!("{}: part not found", a.name))?;
                out.push((a.name.clone(), if m.is_empty() { mime } else { m }, bytes));
            }
        }
        Ok(out)
    }

    /// Send a draft now: SMTP, then a copy in Sent (Gmail files it itself), the server copy of
    /// the draft removed, and the replied message marked answered.
    pub async fn send(&self, d: &Draft) -> anyhow::Result<()> {
        d.check().map_err(|e| anyhow::anyhow!(e))?;
        if !self.inner.cfg.network {
            anyhow::bail!("offline");
        }
        let store = &self.inner.store;
        let acct = store.account(&d.account)?.ok_or_else(|| anyhow::anyhow!("no account {}", d.account))?;
        let pw = store.secret(&format!("password:{}", d.account))?.ok_or_else(|| anyhow::anyhow!("no password"))?;
        let files = self.attachment_files(d).await?;
        let (raw, mid) = mail::smtp::send(&acct, &pw, &compose::outgoing(d, files)).await?;
        tracing::info!("sent {mid} from {}", d.account);
        // everything below is bookkeeping: the mail is out, so failures only log
        if let Some(mut c) = self.connect(&d.account).await {
            if acct.account_type != "gmail" {
                if let Ok(Some(sent)) = store.folder_by_role(&d.account, "sent") {
                    if let Err(e) = c.append(&sent.path, "(\\Seen)", &raw).await {
                        tracing::warn!("save to {}: {e:#}", sent.path);
                    }
                }
            }
            if let Ok(Some(drafts)) = store.folder_by_role(&d.account, "drafts") {
                let _ = c.delete_by_message_id(&drafts.path, &d.message_id).await;
                let _ = store.delete_in_folder_by_message_id(drafts.id, &d.message_id);
            }
            if let Some(src) = d.source_message.filter(|_| d.kind != "forward") {
                if let Some((_, path, uid)) = store.message_location(src).ok().flatten() {
                    let _ = c.store_flags(&path, &[uid], "+", "\\Answered").await;
                }
            }
            self.release(c).await;
        }
        if let Some(src) = d.source_message {
            let _ = store.set_answered(src);
        }
        Ok(())
    }

    /// Save a draft locally (`id == 0` creates it) and return its outbox id.
    pub fn save_draft(&self, id: i64, d: &Draft) -> CoreResult<i64> {
        self.inner.store.put_outgoing(id, &d.account, "draft", &d.to_json(), None, None)
    }

    /// Copy a draft to the account's Drafts folder on the server, replacing the earlier copy
    /// (same Message-ID). Returns false when offline or the account has no Drafts folder.
    pub async fn upload_draft(&self, d: &Draft) -> bool {
        let store = &self.inner.store;
        let Ok(Some(drafts)) = store.folder_by_role(&d.account, "drafts") else { return false };
        let Some(mut c) = self.connect(&d.account).await else { return false };
        // attachments of local files go with the draft; forwarded ones are added when sending
        let files: Vec<_> = d
            .attachments
            .iter()
            .filter_map(|a| {
                let p = a.path.as_ref()?;
                Some((a.name.clone(), compose::mime_for(&a.name).to_string(), std::fs::read(p).ok()?))
            })
            .collect();
        let ok = match mail::smtp::build(&compose::outgoing(d, files)) {
            Ok((raw, _)) => {
                let _ = c.delete_by_message_id(&drafts.path, &d.message_id).await;
                c.append(&drafts.path, "(\\Seen \\Draft)", &raw).await.is_ok()
            }
            Err(e) => {
                tracing::warn!("draft: {e:#}");
                false
            }
        };
        self.release(c).await;
        ok
    }

    /// Throw a draft away, locally and on the server.
    pub async fn discard_draft(&self, id: i64, d: &Draft) {
        let store = &self.inner.store;
        if id > 0 {
            let _ = store.delete_outgoing(id);
        }
        if d.message_id.is_empty() {
            return;
        }
        if let Ok(Some(drafts)) = store.folder_by_role(&d.account, "drafts") {
            let _ = store.delete_in_folder_by_message_id(drafts.id, &d.message_id);
            if let Some(mut c) = self.connect(&d.account).await {
                let _ = c.delete_by_message_id(&drafts.path, &d.message_id).await;
                self.release(c).await;
            }
        }
    }

    /// Queue a draft for sending at `send_at_ms` (now + undo time, or a Send Later time).
    pub fn queue(&self, id: i64, d: &Draft, send_at_ms: i64) -> CoreResult<i64> {
        d.check().map_err(crate::CoreError::Message)?;
        self.inner.store.put_outgoing(id, &d.account, "queued", &d.to_json(), Some(send_at_ms), None)
    }

    /// Take a queued message back before it goes out (Undo Send, cancel a Send Later). Returns
    /// the draft, now a draft again, or None if it is already being sent.
    pub fn unqueue(&self, id: i64) -> Option<Draft> {
        let store = &self.inner.store;
        let row = store.outgoing(id).ok().flatten()?;
        if row.state != "queued" && row.state != "failed" {
            return None;
        }
        let d = Draft::from_json(&row.draft);
        store.put_outgoing(id, &row.account, "draft", &row.draft, None, None).ok()?;
        Some(d)
    }

    /// Send every queued message that is due. Returns `(outbox id, Ok | error)` per message.
    pub async fn process_outbox(&self) -> Vec<(i64, Result<(), String>)> {
        let store = &self.inner.store;
        let now = Utc::now().timestamp_millis();
        let mut done = vec![];
        for row in store.outgoing_due(now).unwrap_or_default() {
            if !store.claim_outgoing(row.id).unwrap_or(false) {
                continue;
            }
            let d = Draft::from_json(&row.draft);
            match self.send(&d).await {
                Ok(()) => {
                    let _ = store.delete_outgoing(row.id);
                    done.push((row.id, Ok(())));
                }
                Err(e) => {
                    let msg = format!("{e:#}");
                    // network trouble (timeouts, refused or throttled connections) is retried a
                    // few times with growing pauses; anything else waits for the user
                    let retry = self.inner.cfg.network && row.attempts < MAX_SEND_RETRIES && transient(&msg);
                    let retry_at = retry.then(|| now + 30_000 * (row.attempts + 1));
                    tracing::warn!("send {} (attempt {}): {msg}; retry: {retry}", row.id, row.attempts + 1);
                    let _ = store.outgoing_failed(row.id, &msg, retry_at);
                    done.push((row.id, Err(msg)));
                }
            }
        }
        done
    }

    // ---------------------------------------------------------------------------------------
    // sync
    // ---------------------------------------------------------------------------------------

    /// One sync pass of an account's system folders (wakes due snoozes first). Returns the number
    /// of new messages.
    pub async fn sync_account(&self, email: &str) -> anyhow::Result<usize> {
        let store = &self.inner.store;
        let _ = store.wake_snoozed(Utc::now().timestamp_millis());
        let mut c = self.checkout(email).await?;
        let remote = c.list_folders().await?;
        let mut targets: Vec<(i64, String, String)> = vec![];
        for f in &remote {
            let id = store.upsert_folder(
                email,
                &f.path,
                &f.name,
                f.delimiter.as_deref(),
                f.role.as_deref(),
                f.selectable,
            )?;
            if let (Some(role), true) = (&f.role, f.selectable) {
                targets.push((id, f.path.clone(), role.clone()));
            }
        }
        let order = ["inbox", "sent", "drafts", "archive", "trash", "spam"];
        targets.sort_by_key(|t| order.iter().position(|r| *r == t.2).unwrap_or(99));
        let before = self.inner.high_water.lock().get(email).copied();
        let mut total = 0;
        for (id, path, _role) in &targets {
            total += c.sync_folder(store, email, *id, path).await?.new;
        }
        self.release(c).await;
        self.report_new_mail(email, before);
        Ok(total)
    }

    fn report_new_mail(&self, email: &str, before: Option<i64>) {
        let filter = ListFilter::Inbox { account: Some(email.to_string()), done: false, category: None };
        let rows = self.inner.store.thread_rows(&filter, 20).unwrap_or_default();
        let newest = rows.iter().map(|r| r.last_ms).max().unwrap_or(0);
        self.inner.high_water.lock().insert(email.to_string(), newest.max(before.unwrap_or(0)));
        let Some(hw) = before else { return };
        let Some(cb) = self.inner.on_new_mail.lock().clone() else { return };
        for r in rows.iter().filter(|r| r.last_ms > hw && r.unseen).take(5) {
            cb(NewMail {
                thread_id: r.thread_id,
                account: email.to_string(),
                from: r.last_from.0.clone().unwrap_or_else(|| r.last_from.1.clone()),
                subject: r.subject.clone(),
                preview: r.preview.clone(),
            });
        }
    }
}

/// Html still referencing `cid:` parts (as an attribute value or css url()).
fn has_cid_refs(html: &str) -> bool {
    let l = html.to_ascii_lowercase();
    ["\"cid:", "'cid:", "(cid:", "=cid:"].iter().any(|p| l.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Account, NewMessage};
    use serde_json::json;

    fn engine() -> (Engine, tempfile::TempDir, Vec<i64>) {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = EngineConfig::for_dir(dir.path());
        cfg.network = false;
        let e = Engine::new(cfg).unwrap();
        let s = e.store();
        s.upsert_account(&Account {
            email: "me@example.com".into(),
            name: "Me".into(),
            account_type: "customIMAP".into(),
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            imap_security: "tls".into(),
            smtp_host: "smtp.example.com".into(),
            smtp_port: 465,
            smtp_security: "tls".into(),
            username: "me@example.com".into(),
        })
        .unwrap();
        let inbox = s.upsert_folder("me@example.com", "INBOX", "Inbox", Some("/"), Some("inbox"), true).unwrap();
        for uid in 1..=3u32 {
            s.insert_message(&NewMessage {
                account: "me@example.com".into(),
                folder_id: inbox,
                uid,
                message_id: Some(format!("<{uid}@example.com>")),
                subject: format!("Subject {uid}"),
                from_addr: format!("sender{uid}@example.org"),
                to: json!([]),
                cc: json!([]),
                bcc: json!([]),
                reply_to: json!([]),
                date_ms: uid as i64 * 1000,
                category: "personal".into(),
                ..Default::default()
            })
            .unwrap();
        }
        let f = ListFilter::Inbox { account: None, done: false, category: None };
        let ids = s.thread_rows(&f, 10).unwrap().iter().map(|r| r.thread_id).collect();
        (e, dir, ids)
    }

    fn inbox(e: &Engine) -> Vec<i64> {
        let f = ListFilter::Inbox { account: None, done: false, category: None };
        e.store().thread_rows(&f, 10).unwrap().iter().map(|r| r.thread_id).collect()
    }

    #[tokio::test]
    async fn offline_actions_change_local_state() {
        let (e, _d, ids) = engine();
        assert_eq!(ids.len(), 3);
        assert!(e.thread_action("pin", &ids[..1], None).await);
        assert!(e.store().thread_rows(&ListFilter::Pinned, 10).unwrap().iter().any(|r| r.thread_id == ids[0]));
        // done leaves the inbox at once (the move to Archive lands on the next sync pass)
        assert!(e.thread_action("done", &ids[1..2], None).await);
        assert!(!inbox(&e).contains(&ids[1]));
        assert!(!e.thread_action("snooze", &ids[2..], None).await, "snooze needs a time");
        assert!(e.thread_action("snooze", &ids[2..], Some(i64::MAX / 2)).await);
        assert!(!inbox(&e).contains(&ids[2]));
        assert!(e.thread_action("unsnooze", &ids[2..], None).await);
        assert!(inbox(&e).contains(&ids[2]));
        assert!(!e.thread_action("nonsense", &ids, None).await);
    }

    #[tokio::test]
    async fn seen_flags_and_bodies_offline() {
        let (e, _d, ids) = engine();
        assert!(e.thread_action("markAsSeen", &ids, None).await);
        assert!(e.store().thread_messages(ids[0]).unwrap().iter().all(|m| m.seen));
        // header-only messages report missing bodies; offline nothing can be fetched
        assert!(e.fetch_missing_bodies(ids[0]).await);
    }

    #[tokio::test]
    async fn outbox_queue_undo_and_offline_failure() {
        let (e, _d, _ids) = engine();
        let mut d = compose::new_draft(e.store(), "me@example.com", "");
        let id = e.save_draft(0, &d).unwrap();
        assert!(e.queue(id, &d, 0).is_err(), "no recipients yet");
        d.to.push(compose::Addr { name: None, address: "x@example.org".into() });
        assert_eq!(e.queue(id, &d, i64::MAX).unwrap(), id);
        assert!(e.process_outbox().await.is_empty(), "not due");
        // undo send: back to a draft, nothing sent
        assert_eq!(e.unqueue(id).unwrap(), d);
        assert_eq!(e.store().outgoing(id).unwrap().unwrap().state, "draft");
        // due now, but offline: the send fails and the message is kept
        e.queue(id, &d, 0).unwrap();
        let r = e.process_outbox().await;
        assert_eq!(r.len(), 1);
        assert!(r[0].1.as_ref().unwrap_err().contains("offline"));
        let row = e.store().outgoing(id).unwrap().unwrap();
        assert_eq!((row.state.as_str(), row.error.as_deref()), ("failed", Some("offline")));
        assert!(e.unqueue(id).is_some(), "a failed message can be edited again");
        e.discard_draft(id, &d).await;
        assert!(e.store().outgoing(id).unwrap().is_none());
    }

    #[test]
    fn transient_errors() {
        assert!(transient("SMTP send: Connection error: connection timed out"));
        assert!(transient("connect to mail.example.com:993 timed out"));
        assert!(!transient("SMTP send: permanent error (550): mailbox unavailable"));
        assert!(!transient("SMTP send: Connection error: authentication failed"));
    }

    #[test]
    fn cid_detection() {
        assert!(has_cid_refs(r#"<img src="cid:logo">"#));
        assert!(has_cid_refs("background:url(cid:x)"));
        assert!(!has_cid_refs("<p>no images</p>"));
    }
}
