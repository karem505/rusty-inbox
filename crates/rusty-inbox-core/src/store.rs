//! SQLite persistence.
//!
//! One database file (`<databases dir>/rusty-inbox.sqlite`) holding settings, accounts, folders,
//! messages and threads, plus an FTS5 index for search. Account secrets live in a separate
//! file (`secrets.sqlite`, mode 0600) so the main DB can be inspected/backed up without them.

use crate::CoreResult;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS accounts (
    email          TEXT PRIMARY KEY,
    name           TEXT NOT NULL DEFAULT '',
    account_type   TEXT NOT NULL,
    imap_host      TEXT NOT NULL,
    imap_port      INTEGER NOT NULL,
    imap_security  TEXT NOT NULL,          -- tls | starttls | none
    smtp_host      TEXT NOT NULL,
    smtp_port      INTEGER NOT NULL,
    smtp_security  TEXT NOT NULL,
    username       TEXT NOT NULL,
    created_ms     INTEGER NOT NULL,
    position       INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS folders (
    id            INTEGER PRIMARY KEY,
    account       TEXT NOT NULL REFERENCES accounts(email) ON DELETE CASCADE,
    path          TEXT NOT NULL,
    name          TEXT NOT NULL,
    delimiter     TEXT,
    role          TEXT,                     -- inbox | sent | drafts | trash | spam | archive | NULL
    selectable    INTEGER NOT NULL DEFAULT 1,
    uidvalidity   INTEGER,
    uidnext       INTEGER,
    synced_ms     INTEGER,
    UNIQUE(account, path)
);

CREATE TABLE IF NOT EXISTS threads (
    id           INTEGER PRIMARY KEY,
    account      TEXT NOT NULL,
    subject      TEXT NOT NULL DEFAULT '',
    last_ms      INTEGER NOT NULL DEFAULT 0,
    done         INTEGER NOT NULL DEFAULT 0,
    pinned       INTEGER NOT NULL DEFAULT 0,
    snoozed_until INTEGER
);
CREATE INDEX IF NOT EXISTS threads_last ON threads(last_ms DESC);

CREATE TABLE IF NOT EXISTS messages (
    id             INTEGER PRIMARY KEY,
    account        TEXT NOT NULL,
    folder_id      INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    uid            INTEGER NOT NULL,
    thread_id      INTEGER REFERENCES threads(id),
    message_id     TEXT,
    in_reply_to    TEXT,
    refs           TEXT,
    subject        TEXT NOT NULL DEFAULT '',
    from_name      TEXT,
    from_addr      TEXT NOT NULL DEFAULT '',
    to_json        TEXT NOT NULL DEFAULT '[]',
    cc_json        TEXT NOT NULL DEFAULT '[]',
    bcc_json       TEXT NOT NULL DEFAULT '[]',
    reply_to_json  TEXT NOT NULL DEFAULT '[]',
    date_ms        INTEGER NOT NULL DEFAULT 0,
    seen           INTEGER NOT NULL DEFAULT 0,
    flagged        INTEGER NOT NULL DEFAULT 0,
    answered       INTEGER NOT NULL DEFAULT 0,
    draft          INTEGER NOT NULL DEFAULT 0,
    size           INTEGER NOT NULL DEFAULT 0,
    has_attachments INTEGER NOT NULL DEFAULT 0,
    preview        TEXT NOT NULL DEFAULT '',
    body_text      TEXT,
    body_html      TEXT,
    body_fetched   INTEGER NOT NULL DEFAULT 0,
    category       TEXT NOT NULL DEFAULT 'personal',   -- personal | notifications | newsletters | invitations
    UNIQUE(folder_id, uid)
);
CREATE INDEX IF NOT EXISTS messages_thread ON messages(thread_id);
CREATE INDEX IF NOT EXISTS messages_msgid ON messages(message_id);
CREATE INDEX IF NOT EXISTS messages_date ON messages(date_ms DESC);

-- local drafts and outgoing mail: a draft becomes "queued" with a send time (undo send, send
-- later), then "sending"; a failed send goes back to "failed" with the error
-- drafts being written and mail waiting to go out; ids are never reused (undo refers to them)
CREATE TABLE IF NOT EXISTS outbox (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    account     TEXT NOT NULL,
    state       TEXT NOT NULL,              -- draft | queued | sending | failed
    draft       TEXT NOT NULL,              -- JSON (compose::Draft)
    send_at     INTEGER,
    error       TEXT,
    attempts    INTEGER NOT NULL DEFAULT 0,
    updated_ms  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS attachments (
    id          INTEGER PRIMARY KEY,
    message_id  INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    part        TEXT NOT NULL,
    filename    TEXT NOT NULL,
    mime        TEXT NOT NULL,
    size        INTEGER NOT NULL DEFAULT 0,
    content_id  TEXT,
    inline      INTEGER NOT NULL DEFAULT 0,
    path        TEXT
);

CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    subject, sender, body, content='', tokenize='unicode61 remove_diacritics 2'
);
"#;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Account {
    pub email: String,
    pub name: String,
    pub account_type: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub imap_security: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_security: String,
    pub username: String,
}

#[derive(Debug, Clone)]
pub struct Folder {
    pub id: i64,
    pub account: String,
    pub path: String,
    pub name: String,
    pub role: Option<String>,
    pub uidvalidity: Option<u32>,
    pub uidnext: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct NewMessage {
    pub account: String,
    pub folder_id: i64,
    pub uid: u32,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub refs: Vec<String>,
    pub subject: String,
    pub from_name: Option<String>,
    pub from_addr: String,
    pub to: Value,
    pub cc: Value,
    pub bcc: Value,
    pub reply_to: Value,
    pub date_ms: i64,
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
    pub draft: bool,
    pub size: u32,
    pub has_attachments: bool,
    pub preview: String,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub category: String,
    pub attachments: Vec<NewAttachment>,
}

#[derive(Debug, Clone, Default)]
pub struct NewAttachment {
    pub part: String,
    pub filename: String,
    pub mime: String,
    pub size: u64,
    pub content_id: Option<String>,
    pub inline: bool,
}

/// One row in the message list (a thread, newest message wins).
#[derive(Debug, Clone)]
pub struct ThreadRow {
    pub thread_id: i64,
    pub account: String,
    pub subject: String,
    pub last_ms: i64,
    pub message_count: i64,
    pub unseen: bool,
    pub flagged: bool,
    pub has_attachments: bool,
    pub pinned: bool,
    pub done: bool,
    pub preview: String,
    pub participants: Vec<(Option<String>, String)>,
    pub last_from: (Option<String>, String),
    pub category: String,
}

#[derive(Debug, Clone, Default)]
pub struct MessageRow {
    pub id: i64,
    pub account: String,
    pub folder_id: i64,
    pub folder_role: Option<String>,
    pub uid: u32,
    pub message_id: Option<String>,
    pub subject: String,
    pub from_name: Option<String>,
    pub from_addr: String,
    pub to: Value,
    pub cc: Value,
    pub bcc: Value,
    pub reply_to: Value,
    pub date_ms: i64,
    pub seen: bool,
    pub flagged: bool,
    pub preview: String,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub body_fetched: bool,
    pub attachments: Vec<(i64, String, String, i64, bool)>,
}

/// SQL: thread `t` is Done (flagged, or no message of it is left in an inbox folder).
const DONE_EXPR: &str = "(t.done=1 OR NOT EXISTS (SELECT 1 FROM messages i JOIN folders fi ON fi.id=i.folder_id \
     WHERE i.thread_id=t.id AND fi.role='inbox'))";

/// Which messages a list shows.
#[derive(Debug, Clone, PartialEq)]
pub enum ListFilter {
    /// inbox of one account or all (`None`); `done` selects Done threads (marked done, or moved
    /// out of the inbox to Archive) instead of the inbox. `with_done` shows both together.
    Inbox {
        account: Option<String>,
        done: bool,
        category: Option<String>,
        with_done: bool,
    },
    /// a role folder (sent/drafts/trash/spam/archive) across accounts or one account.
    Role {
        account: Option<String>,
        role: String,
    },
    /// a concrete folder by id.
    Folder(i64),
    Pinned,
    /// inbox threads hidden until a time (`threads.snoozed_until`), soonest first by date.
    Snoozed {
        account: Option<String>,
    },
    /// full-text search across everything.
    Search(String),
    /// structured search (`from:` / `in:` / `has:` / text …) from the search field.
    Query(crate::search::Query),
}

/// Counts behind the sidebar badges (see `Store::sidebar_counts`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SidebarCounts {
    pub inbox_unread: i64,
    /// category -> (unread threads, all threads)
    pub categories: HashMap<String, (i64, i64)>,
    pub account_unread: HashMap<String, i64>,
    pub pinned: i64,
    pub snoozed: i64,
    pub drafts: i64,
}

pub struct Store {
    db: Mutex<Connection>,
    secrets: Mutex<Connection>,
}

impl Store {
    /// Flush the WAL into the main database files (quit / termination).
    pub fn checkpoint(&self) -> CoreResult<()> {
        for c in [&self.db, &self.secrets] {
            c.lock().execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        }
        Ok(())
    }

    pub fn open(dir: &Path) -> CoreResult<Store> {
        std::fs::create_dir_all(dir).map_err(|e| crate::CoreError::Message(e.to_string()))?;
        let path = dir.join("rusty-inbox.sqlite");
        let db = Connection::open(&path)?;
        db.execute_batch(SCHEMA)?;
        let secrets_path = dir.join("secrets.sqlite");
        let secrets = Connection::open(&secrets_path)?;
        secrets.execute_batch("CREATE TABLE IF NOT EXISTS secrets (key TEXT PRIMARY KEY, value TEXT NOT NULL);")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&secrets_path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(Store { db: Mutex::new(db), secrets: Mutex::new(secrets) })
    }

    pub fn in_memory() -> CoreResult<Store> {
        let db = Connection::open_in_memory()?;
        db.execute_batch(SCHEMA)?;
        let secrets = Connection::open_in_memory()?;
        secrets.execute_batch("CREATE TABLE secrets (key TEXT PRIMARY KEY, value TEXT NOT NULL);")?;
        Ok(Store { db: Mutex::new(db), secrets: Mutex::new(secrets) })
    }

    // ---------- settings ----------

    pub fn setting(&self, key: &str) -> CoreResult<Option<Value>> {
        let db = self.db.lock();
        let v: Option<String> =
            db.query_row("SELECT value FROM settings WHERE key=?1", [key], |r| r.get(0)).optional()?;
        Ok(v.and_then(|s| serde_json::from_str(&s).ok()))
    }

    pub fn set_setting(&self, key: &str, value: &Value) -> CoreResult<()> {
        self.db.lock().execute(
            "INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value.to_string()],
        )?;
        Ok(())
    }

    // ---------- secrets ----------

    pub fn set_secret(&self, key: &str, value: &str) -> CoreResult<()> {
        self.secrets.lock().execute(
            "INSERT INTO secrets(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn secret(&self, key: &str) -> CoreResult<Option<String>> {
        Ok(self.secrets.lock().query_row("SELECT value FROM secrets WHERE key=?1", [key], |r| r.get(0)).optional()?)
    }

    pub fn delete_secret(&self, key: &str) -> CoreResult<()> {
        self.secrets.lock().execute("DELETE FROM secrets WHERE key=?1", [key])?;
        Ok(())
    }

    // ---------- accounts ----------

    pub fn upsert_account(&self, a: &Account) -> CoreResult<()> {
        let db = self.db.lock();
        let pos: i64 = db.query_row("SELECT COALESCE(MAX(position)+1,0) FROM accounts", [], |r| r.get(0))?;
        db.execute(
            "INSERT INTO accounts(email,name,account_type,imap_host,imap_port,imap_security,smtp_host,smtp_port,smtp_security,username,created_ms,position)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             ON CONFLICT(email) DO UPDATE SET name=excluded.name, account_type=excluded.account_type,
               imap_host=excluded.imap_host, imap_port=excluded.imap_port, imap_security=excluded.imap_security,
               smtp_host=excluded.smtp_host, smtp_port=excluded.smtp_port, smtp_security=excluded.smtp_security,
               username=excluded.username",
            params![
                a.email, a.name, a.account_type, a.imap_host, a.imap_port, a.imap_security, a.smtp_host,
                a.smtp_port, a.smtp_security, a.username, chrono::Utc::now().timestamp_millis(), pos
            ],
        )?;
        Ok(())
    }

    pub fn accounts(&self) -> CoreResult<Vec<Account>> {
        let db = self.db.lock();
        let mut st = db.prepare(
            "SELECT email,name,account_type,imap_host,imap_port,imap_security,smtp_host,smtp_port,smtp_security,username
             FROM accounts ORDER BY position",
        )?;
        let rows = st.query_map([], |r| {
            Ok(Account {
                email: r.get(0)?,
                name: r.get(1)?,
                account_type: r.get(2)?,
                imap_host: r.get(3)?,
                imap_port: r.get(4)?,
                imap_security: r.get(5)?,
                smtp_host: r.get(6)?,
                smtp_port: r.get(7)?,
                smtp_security: r.get(8)?,
                username: r.get(9)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn account(&self, email: &str) -> CoreResult<Option<Account>> {
        Ok(self.accounts()?.into_iter().find(|a| a.email.eq_ignore_ascii_case(email)))
    }

    pub fn remove_account(&self, email: &str) -> CoreResult<()> {
        let db = self.db.lock();
        db.execute("DELETE FROM messages WHERE account=?1", [email])?;
        db.execute("DELETE FROM threads WHERE account=?1", [email])?;
        db.execute("DELETE FROM accounts WHERE email=?1", [email])?;
        drop(db);
        self.delete_secret(&format!("password:{email}"))
    }

    // ---------- folders ----------

    pub fn upsert_folder(
        &self,
        account: &str,
        path: &str,
        name: &str,
        delimiter: Option<&str>,
        role: Option<&str>,
        selectable: bool,
    ) -> CoreResult<i64> {
        let db = self.db.lock();
        db.execute(
            "INSERT INTO folders(account,path,name,delimiter,role,selectable) VALUES(?1,?2,?3,?4,?5,?6)
             ON CONFLICT(account,path) DO UPDATE SET name=excluded.name, delimiter=excluded.delimiter,
               role=excluded.role, selectable=excluded.selectable",
            params![account, path, name, delimiter, role, selectable],
        )?;
        Ok(db.query_row("SELECT id FROM folders WHERE account=?1 AND path=?2", [account, path], |r| r.get(0))?)
    }

    pub fn folders(&self, account: Option<&str>) -> CoreResult<Vec<Folder>> {
        let db = self.db.lock();
        let mut st = db.prepare(
            "SELECT id,account,path,name,role,uidvalidity,uidnext FROM folders
             WHERE selectable=1 AND (?1 IS NULL OR account=?1) ORDER BY account, (role IS NULL), path",
        )?;
        let rows = st.query_map([account], |r| {
            Ok(Folder {
                id: r.get(0)?,
                account: r.get(1)?,
                path: r.get(2)?,
                name: r.get(3)?,
                role: r.get(4)?,
                uidvalidity: r.get(5)?,
                uidnext: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn folder_by_role(&self, account: &str, role: &str) -> CoreResult<Option<Folder>> {
        Ok(self.folders(Some(account))?.into_iter().find(|f| f.role.as_deref() == Some(role)))
    }

    pub fn folder(&self, id: i64) -> CoreResult<Option<Folder>> {
        Ok(self.folders(None)?.into_iter().find(|f| f.id == id))
    }

    /// Record sync position; a changed UIDVALIDITY wipes the folder's cached messages.
    pub fn set_folder_sync(&self, id: i64, uidvalidity: u32, uidnext: u32) -> CoreResult<bool> {
        let db = self.db.lock();
        let old: Option<Option<u32>> =
            db.query_row("SELECT uidvalidity FROM folders WHERE id=?1", [id], |r| r.get(0)).optional()?;
        let reset = matches!(old, Some(Some(v)) if v != uidvalidity);
        if reset {
            db.execute("DELETE FROM messages WHERE folder_id=?1", [id])?;
        }
        db.execute(
            "UPDATE folders SET uidvalidity=?2, uidnext=?3, synced_ms=?4 WHERE id=?1",
            params![id, uidvalidity, uidnext, chrono::Utc::now().timestamp_millis()],
        )?;
        Ok(reset)
    }

    pub fn max_uid(&self, folder_id: i64) -> CoreResult<u32> {
        Ok(self.db.lock().query_row(
            "SELECT COALESCE(MAX(uid),0) FROM messages WHERE folder_id=?1",
            [folder_id],
            |r| r.get(0),
        )?)
    }

    pub fn uids(&self, folder_id: i64) -> CoreResult<Vec<u32>> {
        let db = self.db.lock();
        let mut st = db.prepare("SELECT uid FROM messages WHERE folder_id=?1")?;
        let v = st.query_map([folder_id], |r| r.get(0))?.collect::<Result<_, _>>()?;
        Ok(v)
    }

    // ---------- messages / threading ----------

    /// Insert a message (idempotent on folder+uid) and attach it to a thread.
    /// Threading: References / In-Reply-To chain to an existing Message-ID, otherwise a new thread.
    pub fn insert_message(&self, m: &NewMessage) -> CoreResult<i64> {
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        if let Some(id) = tx
            .query_row("SELECT id FROM messages WHERE folder_id=?1 AND uid=?2", params![m.folder_id, m.uid], |r| {
                r.get::<_, i64>(0)
            })
            .optional()?
        {
            tx.execute(
                "UPDATE messages SET seen=?2, flagged=?3, answered=?4, draft=?5 WHERE id=?1",
                params![id, m.seen, m.flagged, m.answered, m.draft],
            )?;
            tx.commit()?;
            return Ok(id);
        }

        // find a thread through the reference chain (newest reference first)
        let mut thread: Option<i64> = None;
        let mut chain: Vec<&String> = m.refs.iter().rev().collect();
        if let Some(irt) = &m.in_reply_to {
            chain.insert(0, irt);
        }
        for r in chain {
            thread = tx
                .query_row(
                    "SELECT thread_id FROM messages WHERE message_id=?1 AND account=?2 AND thread_id IS NOT NULL LIMIT 1",
                    params![r, m.account],
                    |row| row.get(0),
                )
                .optional()?;
            if thread.is_some() {
                break;
            }
        }
        // the same Message-ID already stored in another folder (e.g. INBOX + Archive) shares its thread
        if thread.is_none() {
            if let Some(mid) = &m.message_id {
                thread = tx
                    .query_row(
                        "SELECT thread_id FROM messages WHERE message_id=?1 AND account=?2 LIMIT 1",
                        params![mid, m.account],
                        |row| row.get(0),
                    )
                    .optional()?;
            }
        }
        let thread_id = match thread {
            Some(t) => {
                tx.execute("UPDATE threads SET last_ms=MAX(last_ms,?2) WHERE id=?1", params![t, m.date_ms])?;
                t
            }
            None => {
                tx.execute(
                    "INSERT INTO threads(account,subject,last_ms) VALUES(?1,?2,?3)",
                    params![m.account, crate::mail::normalize_subject(&m.subject), m.date_ms],
                )?;
                tx.last_insert_rowid()
            }
        };
        tx.execute(
            "INSERT INTO messages(account,folder_id,uid,thread_id,message_id,in_reply_to,refs,subject,from_name,from_addr,
                to_json,cc_json,bcc_json,reply_to_json,date_ms,seen,flagged,answered,draft,size,has_attachments,preview,
                body_text,body_html,body_fetched,category)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26)",
            params![
                m.account,
                m.folder_id,
                m.uid,
                thread_id,
                m.message_id,
                m.in_reply_to,
                m.refs.join(" "),
                m.subject,
                m.from_name,
                m.from_addr,
                m.to.to_string(),
                m.cc.to_string(),
                m.bcc.to_string(),
                m.reply_to.to_string(),
                m.date_ms,
                m.seen,
                m.flagged,
                m.answered,
                m.draft,
                m.size,
                m.has_attachments,
                m.preview,
                m.body_text,
                m.body_html,
                m.body_text.is_some() || m.body_html.is_some(),
                m.category,
            ],
        )?;
        let id = tx.last_insert_rowid();
        for a in &m.attachments {
            tx.execute(
                "INSERT INTO attachments(message_id,part,filename,mime,size,content_id,inline) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![id, a.part, a.filename, a.mime, a.size as i64, a.content_id, a.inline],
            )?;
        }
        let sender = format!("{} {}", m.from_name.clone().unwrap_or_default(), m.from_addr);
        tx.execute(
            "INSERT INTO messages_fts(rowid,subject,sender,body) VALUES(?1,?2,?3,?4)",
            params![id, m.subject, sender, m.body_text.clone().unwrap_or_else(|| m.preview.clone())],
        )?;
        tx.commit()?;
        Ok(id)
    }

    pub fn delete_uids(&self, folder_id: i64, uids: &[u32]) -> CoreResult<usize> {
        let db = self.db.lock();
        let mut n = 0;
        for u in uids {
            n += db.execute("DELETE FROM messages WHERE folder_id=?1 AND uid=?2", params![folder_id, u])?;
        }
        Ok(n)
    }

    pub fn set_flags(&self, folder_id: i64, uid: u32, seen: bool, flagged: bool) -> CoreResult<()> {
        self.db.lock().execute(
            "UPDATE messages SET seen=?3, flagged=?4 WHERE folder_id=?1 AND uid=?2",
            params![folder_id, uid, seen, flagged],
        )?;
        Ok(())
    }

    pub fn thread_rows(&self, filter: &ListFilter, limit: usize) -> CoreResult<Vec<ThreadRow>> {
        let db = self.db.lock();
        let (cond, arg): (String, Option<String>) = match filter {
            ListFilter::Inbox { account, done, category, with_done } => {
                let mut c = if *with_done {
                    "f.role IN ('inbox','archive') AND t.snoozed_until IS NULL".to_string()
                } else if *done {
                    format!("f.role IN ('inbox','archive') AND {DONE_EXPR} AND t.snoozed_until IS NULL")
                } else {
                    "f.role='inbox' AND t.done=0 AND t.snoozed_until IS NULL".to_string()
                };
                if let Some(cat) = category {
                    c += &format!(" AND m.category='{}'", cat.replace('\'', ""));
                }
                (c, account.clone())
            }
            ListFilter::Role { account, role } => (format!("f.role='{}'", role.replace('\'', "")), account.clone()),
            ListFilter::Folder(id) => (format!("f.id={id}"), None),
            ListFilter::Pinned => ("t.pinned=1".to_string(), None),
            ListFilter::Snoozed { account } => ("t.snoozed_until IS NOT NULL".to_string(), account.clone()),
            ListFilter::Search(q) => {
                let q = fts_query(q);
                (format!("m.id IN (SELECT rowid FROM messages_fts WHERE messages_fts MATCH '{q}')"), None)
            }
            ListFilter::Query(q) => (query_condition(q), None),
        };
        // in inbox lists a conversation is Done when marked so or when none of it is in the inbox
        let done_col = if matches!(filter, ListFilter::Inbox { .. }) { DONE_EXPR } else { "t.done" };
        let sql = format!(
            "SELECT t.id, t.account, t.subject, MAX(m.date_ms),
                    (SELECT COUNT(DISTINCT COALESCE(q.message_id, q.id)) FROM messages q WHERE q.thread_id=t.id), MIN(m.seen), MAX(m.flagged),
                    MAX(m.has_attachments), t.pinned, {done_col},
                    (SELECT preview FROM messages x WHERE x.thread_id=t.id ORDER BY x.date_ms DESC LIMIT 1),
                    (SELECT json_group_array(json_array(from_name, from_addr)) FROM
                        (SELECT DISTINCT from_name, from_addr FROM messages y WHERE y.thread_id=t.id ORDER BY y.date_ms)),
                    (SELECT json_array(from_name, from_addr) FROM messages z WHERE z.thread_id=t.id ORDER BY z.date_ms DESC LIMIT 1),
                    (SELECT category FROM messages c WHERE c.thread_id=t.id ORDER BY c.date_ms DESC LIMIT 1)
             FROM threads t JOIN messages m ON m.thread_id=t.id JOIN folders f ON f.id=m.folder_id
             WHERE {cond} AND (?1 IS NULL OR t.account=?1)
             GROUP BY t.id ORDER BY MAX(m.date_ms) DESC LIMIT {limit}"
        );
        let mut st = db.prepare(&sql)?;
        let rows = st.query_map([arg], |r| {
            let parts: String = r.get(11)?;
            let last: String = r.get(12)?;
            let parse_pair = |v: &Value| -> (Option<String>, String) {
                (
                    v.get(0).and_then(|x| x.as_str()).filter(|s| !s.is_empty()).map(str::to_string),
                    v.get(1).and_then(|x| x.as_str()).unwrap_or_default().to_string(),
                )
            };
            let parts_v: Value = serde_json::from_str(&parts).unwrap_or(Value::Null);
            let last_v: Value = serde_json::from_str(&last).unwrap_or(Value::Null);
            Ok(ThreadRow {
                thread_id: r.get(0)?,
                account: r.get(1)?,
                subject: r.get(2)?,
                last_ms: r.get(3)?,
                message_count: r.get(4)?,
                unseen: r.get::<_, i64>(5)? == 0,
                flagged: r.get::<_, i64>(6)? != 0,
                has_attachments: r.get::<_, i64>(7)? != 0,
                pinned: r.get::<_, i64>(8)? != 0,
                done: r.get::<_, i64>(9)? != 0,
                preview: r.get::<_, Option<String>>(10)?.unwrap_or_default(),
                participants: parts_v.as_array().map(|a| a.iter().map(parse_pair).collect()).unwrap_or_default(),
                last_from: parse_pair(&last_v),
                category: r.get::<_, Option<String>>(13)?.unwrap_or_else(|| "personal".into()),
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn thread_messages(&self, thread_id: i64) -> CoreResult<Vec<MessageRow>> {
        let db = self.db.lock();
        let mut st = db.prepare(
            "SELECT m.id,m.account,m.folder_id,f.role,m.uid,m.message_id,m.subject,m.from_name,m.from_addr,m.to_json,
                    m.cc_json,m.bcc_json,m.reply_to_json,m.date_ms,m.seen,m.flagged,m.preview,m.body_text,m.body_html,
                    m.body_fetched
             FROM messages m JOIN folders f ON f.id=m.folder_id WHERE m.thread_id=?1 ORDER BY m.date_ms",
        )?;
        let mut rows: Vec<MessageRow> = st
            .query_map([thread_id], |r| {
                let j = |i: usize| -> rusqlite::Result<Value> {
                    Ok(serde_json::from_str(&r.get::<_, String>(i)?).unwrap_or(Value::Array(vec![])))
                };
                Ok(MessageRow {
                    id: r.get(0)?,
                    account: r.get(1)?,
                    folder_id: r.get(2)?,
                    folder_role: r.get(3)?,
                    uid: r.get(4)?,
                    message_id: r.get(5)?,
                    subject: r.get(6)?,
                    from_name: r.get(7)?,
                    from_addr: r.get(8)?,
                    to: j(9)?,
                    cc: j(10)?,
                    bcc: j(11)?,
                    reply_to: j(12)?,
                    date_ms: r.get(13)?,
                    seen: r.get(14)?,
                    flagged: r.get(15)?,
                    preview: r.get(16)?,
                    body_text: r.get(17)?,
                    body_html: r.get(18)?,
                    body_fetched: r.get(19)?,
                    attachments: vec![],
                })
            })?
            .collect::<Result<_, _>>()?;
        // de-duplicate copies of the same message living in several folders (keep the inbox copy)
        let mut seen_ids = std::collections::HashSet::new();
        rows.sort_by_key(|m| (m.date_ms, m.folder_role.as_deref() != Some("inbox")));
        rows.retain(|m| match &m.message_id {
            Some(id) => seen_ids.insert(id.clone()),
            None => true,
        });
        let mut ast = db.prepare("SELECT id,filename,mime,size,inline FROM attachments WHERE message_id=?1")?;
        for m in rows.iter_mut() {
            m.attachments = ast
                .query_map([m.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
                .collect::<Result<_, _>>()?;
        }
        Ok(rows)
    }

    pub fn set_body(&self, message_id: i64, text: Option<&str>, html: Option<&str>) -> CoreResult<()> {
        let db = self.db.lock();
        db.execute(
            "UPDATE messages SET body_text=?2, body_html=?3, body_fetched=1 WHERE id=?1",
            params![message_id, text, html],
        )?;
        Ok(())
    }

    pub fn set_thread_flag(&self, thread_id: i64, column: &str, value: bool) -> CoreResult<()> {
        let col = match column {
            "done" | "pinned" => column,
            _ => return Err(crate::CoreError::Message(format!("bad thread column {column}"))),
        };
        self.db.lock().execute(&format!("UPDATE threads SET {col}=?2 WHERE id=?1"), params![thread_id, value])?;
        Ok(())
    }

    /// Every sidebar count in a few aggregate queries (the list queries are far too slow for this).
    /// Unread follows `thread_rows`: a thread is unread when one of its matching messages is unseen;
    /// a thread belongs to every category one of its inbox messages has.
    pub fn sidebar_counts(&self) -> CoreResult<SidebarCounts> {
        let db = self.db.lock();
        let mut out = SidebarCounts::default();
        let mut st = db.prepare(
            "SELECT t.id, t.account, COALESCE(m.category,'personal'), MIN(m.seen)
             FROM threads t JOIN messages m ON m.thread_id=t.id JOIN folders f ON f.id=m.folder_id
             WHERE f.role='inbox' AND t.done=0 AND t.snoozed_until IS NULL
             GROUP BY t.id, COALESCE(m.category,'personal')",
        )?;
        let rows = st.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)? == 0))
        })?;
        let mut threads: HashMap<i64, (String, bool)> = HashMap::new();
        for row in rows {
            let (tid, account, category, unseen) = row?;
            let c = out.categories.entry(category).or_default();
            c.1 += 1;
            if unseen {
                c.0 += 1;
            }
            let e = threads.entry(tid).or_insert((account, false));
            e.1 |= unseen;
        }
        for (account, unseen) in threads.into_values() {
            if unseen {
                out.inbox_unread += 1;
                *out.account_unread.entry(account).or_default() += 1;
            }
        }
        let count = |sql: &str| -> CoreResult<i64> { Ok(db.query_row(sql, [], |r| r.get(0))?) };
        let has_msgs = "EXISTS (SELECT 1 FROM messages m WHERE m.thread_id=t.id)";
        out.pinned = count(&format!("SELECT COUNT(*) FROM threads t WHERE t.pinned=1 AND {has_msgs}"))?;
        out.snoozed =
            count(&format!("SELECT COUNT(*) FROM threads t WHERE t.snoozed_until IS NOT NULL AND {has_msgs}"))?;
        out.drafts = count(
            "SELECT COUNT(DISTINCT m.thread_id) FROM messages m JOIN folders f ON f.id=m.folder_id WHERE f.role='drafts'",
        )?;
        Ok(out)
    }

    /// Hide a thread from the inbox until `until_ms` (`None` brings it back now).
    pub fn set_snooze(&self, thread_id: i64, until_ms: Option<i64>) -> CoreResult<()> {
        self.db.lock().execute("UPDATE threads SET snoozed_until=?2 WHERE id=?1", params![thread_id, until_ms])?;
        Ok(())
    }

    /// Un-snooze every thread whose time has come; they return to the inbox unread. Returns their ids.
    pub fn wake_snoozed(&self, now_ms: i64) -> CoreResult<Vec<i64>> {
        let db = self.db.lock();
        let ids: Vec<i64> = db
            .prepare("SELECT id FROM threads WHERE snoozed_until IS NOT NULL AND snoozed_until<=?1")?
            .query_map([now_ms], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        for id in &ids {
            db.execute("UPDATE threads SET snoozed_until=NULL WHERE id=?1", [id])?;
            db.execute("UPDATE messages SET seen=0 WHERE id=(SELECT id FROM messages WHERE thread_id=?1 ORDER BY date_ms DESC LIMIT 1)", [id])?;
        }
        Ok(ids)
    }

    pub fn set_thread_seen(&self, thread_id: i64, seen: bool) -> CoreResult<Vec<(i64, u32)>> {
        let db = self.db.lock();
        db.execute("UPDATE messages SET seen=?2 WHERE thread_id=?1", params![thread_id, seen])?;
        let mut st = db.prepare("SELECT folder_id, uid FROM messages WHERE thread_id=?1")?;
        let v = st.query_map([thread_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        Ok(v)
    }

    pub fn thread_locations(&self, thread_id: i64) -> CoreResult<Vec<(i64, u32)>> {
        let db = self.db.lock();
        let mut st = db.prepare("SELECT folder_id, uid FROM messages WHERE thread_id=?1")?;
        let v = st.query_map([thread_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        Ok(v)
    }

    /// `(account, folder path, uid, part, filename, mime, cached path)` of an attachment.
    #[allow(clippy::type_complexity)]
    pub fn attachment_location(
        &self,
        attachment_id: i64,
    ) -> CoreResult<Option<(String, String, u32, String, String, String, Option<String>, i64)>> {
        let db = self.db.lock();
        Ok(db
            .query_row(
                "SELECT m.account, f.path, m.uid, a.part, a.filename, a.mime, a.path, m.id
                 FROM attachments a JOIN messages m ON m.id=a.message_id JOIN folders f ON f.id=m.folder_id
                 WHERE a.id=?1",
                [attachment_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)),
            )
            .optional()?)
    }

    pub fn set_attachment_path(&self, attachment_id: i64, path: &str) -> CoreResult<()> {
        self.db.lock().execute("UPDATE attachments SET path=?2 WHERE id=?1", params![attachment_id, path])?;
        Ok(())
    }

    /// Ordinal of an attachment among its message's attachments (matches `mime::attachment_bytes`).
    pub fn attachment_index(&self, attachment_id: i64) -> CoreResult<usize> {
        let db = self.db.lock();
        let n: i64 = db.query_row(
            "SELECT COUNT(*) FROM attachments b WHERE b.message_id=(SELECT message_id FROM attachments WHERE id=?1)
               AND b.id < ?1",
            [attachment_id],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }

    /// `(account, folder path, uid)` of a message.
    /// One message (with its thread siblings loaded through `thread_messages`).
    pub fn message(&self, message_id: i64) -> CoreResult<Option<(i64, MessageRow)>> {
        let tid: Option<i64> = {
            let db = self.db.lock();
            db.query_row("SELECT thread_id FROM messages WHERE id=?1", [message_id], |r| r.get(0)).optional()?
        };
        let Some(tid) = tid else { return Ok(None) };
        let msgs = self.thread_messages(tid)?;
        // the requested copy may have been de-duplicated in favour of another folder's copy
        let m = msgs.iter().find(|m| m.id == message_id).or(msgs.last()).cloned();
        Ok(m.map(|m| (tid, m)))
    }

    pub fn message_location(&self, message_id: i64) -> CoreResult<Option<(String, String, u32)>> {
        let db = self.db.lock();
        Ok(db
            .query_row(
                "SELECT m.account, f.path, m.uid FROM messages m JOIN folders f ON f.id=m.folder_id WHERE m.id=?1",
                [message_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?)
    }

    /// Thread locations grouped by folder path: `[(account, path, [uids])]`.
    pub fn thread_folder_uids(&self, thread_id: i64) -> CoreResult<Vec<(String, String, Vec<u32>)>> {
        let db = self.db.lock();
        let mut st = db.prepare(
            "SELECT m.account, f.path, m.uid FROM messages m JOIN folders f ON f.id=m.folder_id WHERE m.thread_id=?1
             ORDER BY f.path",
        )?;
        let rows: Vec<(String, String, u32)> =
            st.query_map([thread_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
        let mut out: Vec<(String, String, Vec<u32>)> = vec![];
        for (a, p, u) in rows {
            match out.last_mut() {
                Some((la, lp, v)) if *la == a && *lp == p => v.push(u),
                _ => out.push((a, p, vec![u])),
            }
        }
        Ok(out)
    }

    /// Remove every local copy of a thread (after a remote move/delete); returns removed rows.
    pub fn delete_thread_messages(&self, thread_id: i64) -> CoreResult<usize> {
        let db = self.db.lock();
        let n = db.execute("DELETE FROM messages WHERE thread_id=?1", [thread_id])?;
        db.execute("DELETE FROM threads WHERE id=?1", [thread_id])?;
        Ok(n)
    }

    pub fn count_unseen(&self, filter: &ListFilter) -> CoreResult<i64> {
        Ok(self.thread_rows(filter, 100_000)?.iter().filter(|t| t.unseen).count() as i64)
    }

    /// Thread-level state `(pinned, done, category)` for a thread id.
    pub fn thread_state(&self, thread_id: i64) -> CoreResult<Option<(bool, bool, String)>> {
        let db = self.db.lock();
        let r = db
            .query_row(
                "SELECT t.pinned, t.done,
                        COALESCE((SELECT m.category FROM messages m WHERE m.thread_id=t.id ORDER BY m.date_ms DESC LIMIT 1), 'personal')
                 FROM threads t WHERE t.id=?1",
                [thread_id],
                |r| Ok((r.get::<_, i64>(0)? != 0, r.get::<_, i64>(1)? != 0, r.get::<_, String>(2)?)),
            )
            .ok();
        Ok(r)
    }

    /// Most frequent inbox senders (excluding the user's own addresses), most frequent first:
    /// `(address, display name, category)`; used for the setup wizard's contact suggestions.
    pub fn top_senders(&self, exclude: &[String], limit: usize) -> CoreResult<Vec<(String, Option<String>, String)>> {
        let db = self.db.lock();
        let mut st = db.prepare(
            "SELECT lower(m.from_addr), max(m.from_name), max(m.category), count(*) AS n
             FROM messages m JOIN folders f ON f.id = m.folder_id
             WHERE f.role = 'inbox' AND m.from_addr <> ''
             GROUP BY lower(m.from_addr) ORDER BY n DESC, max(m.date_ms) DESC LIMIT ?1",
        )?;
        let rows = st.query_map([(limit + exclude.len()) as i64], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, String>(2)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (addr, name, cat) = row?;
            if exclude.iter().any(|e| e.eq_ignore_ascii_case(&addr)) {
                continue;
            }
            out.push((addr, name.filter(|n| !n.trim().is_empty()), cat));
            if out.len() == limit {
                break;
            }
        }
        Ok(out)
    }

    /// Address-book style lookup over everyone seen in mail (senders + recipients), most frequent first.
    pub fn suggest_contacts(&self, q: &str, limit: usize) -> CoreResult<Vec<(String, Option<String>)>> {
        let q = q.trim().to_lowercase();
        if q.is_empty() {
            return Ok(vec![]);
        }
        let db = self.db.lock();
        let mut st = db.prepare(
            "WITH people(addr, name, d) AS (
                SELECT lower(from_addr), from_name, date_ms FROM messages WHERE from_addr <> ''
                UNION ALL
                SELECT lower(json_extract(j.value,'$.mailbox')), json_extract(j.value,'$.displayName.value'), m.date_ms
                FROM messages m, json_each(CASE WHEN json_valid(m.to_json) THEN m.to_json ELSE '[]' END) j
                UNION ALL
                SELECT lower(json_extract(j.value,'$.mailbox')), json_extract(j.value,'$.displayName.value'), m.date_ms
                FROM messages m, json_each(CASE WHEN json_valid(m.cc_json) THEN m.cc_json ELSE '[]' END) j
             )
             SELECT addr, max(name), count(*) n, max(d) FROM people
             WHERE addr LIKE '%' || ?1 || '%' OR lower(coalesce(name,'')) LIKE '%' || ?1 || '%'
             GROUP BY addr ORDER BY (addr LIKE ?1 || '%') DESC, n DESC, max(d) DESC LIMIT ?2",
        )?;
        let rows = st.query_map(rusqlite::params![q, limit as i64], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })?;
        Ok(rows.filter_map(Result::ok).map(|(a, n)| (a, n.filter(|n| !n.trim().is_empty()))).collect())
    }

    pub fn counts(&self) -> CoreResult<(i64, i64)> {
        let db = self.db.lock();
        Ok(db.query_row("SELECT (SELECT COUNT(*) FROM messages), (SELECT COUNT(*) FROM threads)", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?)
    }
}

/// SQL literal (single quotes doubled) — values come from the user's search field.
fn lit(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// WHERE clause for a structured search. Every condition is per message (`m`), results are grouped
/// into threads by the caller.
fn query_condition(q: &crate::search::Query) -> String {
    let mut c: Vec<String> = vec![];
    let text: Vec<String> = q.text.iter().chain(&q.subject).cloned().collect();
    if !text.is_empty() {
        let fq = fts_query(&text.join(" "));
        if !fq.is_empty() {
            c.push(format!("m.id IN (SELECT rowid FROM messages_fts WHERE messages_fts MATCH {})", lit(&fq)));
        }
    }
    for s in &q.subject {
        c.push(format!("instr(lower(m.subject), lower({})) > 0", lit(s)));
    }
    for a in &q.from {
        c.push(format!(
            "(lower(m.from_addr) = {0} OR instr(lower(m.from_addr), {0}) > 0 OR instr(lower(coalesce(m.from_name,'')), {0}) > 0)",
            lit(a)
        ));
    }
    for (col, list) in [("to_json", &q.to), ("cc_json", &q.cc)] {
        for a in list {
            c.push(format!("instr(lower(m.{col}), {}) > 0", lit(a)));
        }
    }
    match q.folder.as_deref() {
        Some(r @ ("inbox" | "sent" | "drafts" | "trash" | "spam" | "archive")) => c.push(format!("f.role={}", lit(r))),
        Some(other) => c.push(format!("(lower(f.name)={0} OR lower(f.path)={0})", lit(other))),
        // a plain search skips Trash and Spam
        None => c.push("coalesce(f.role,'') NOT IN ('trash','spam')".into()),
    }
    if q.has_attachments {
        c.push("m.has_attachments=1".into());
    }
    match q.unread {
        Some(true) => c.push("m.seen=0".into()),
        Some(false) => c.push("m.seen=1".into()),
        None => {}
    }
    if q.pinned {
        c.push("t.pinned=1".into());
    }
    if let Some(ms) = q.since_ms {
        c.push(format!("m.date_ms >= {ms}"));
    }
    if q.is_empty() {
        return "0".into();
    }
    c.join(" AND ")
}

/// One row of the `outgoing` table.
/// `(Message-ID, In-Reply-To, References)` of a message.
pub type ThreadingHeaders = (Option<String>, Option<String>, Vec<String>);

#[derive(Debug, Clone, PartialEq)]
pub struct OutgoingRow {
    pub id: i64,
    pub account: String,
    pub state: String,
    pub draft: Value,
    pub send_at: Option<i64>,
    pub error: Option<String>,
    pub attempts: i64,
    pub updated_ms: i64,
}

impl Store {
    /// Insert (`id == 0`) or update an outgoing row; returns its id.
    pub fn put_outgoing(
        &self,
        id: i64,
        account: &str,
        state: &str,
        draft: &Value,
        send_at: Option<i64>,
        error: Option<&str>,
    ) -> CoreResult<i64> {
        let now = chrono::Utc::now().timestamp_millis();
        let db = self.db.lock();
        let json = draft.to_string();
        if id > 0 {
            let n = db.execute(
                "UPDATE outbox SET account=?2, state=?3, draft=?4, send_at=?5, error=?6, updated_ms=?7, attempts=0 WHERE id=?1",
                params![id, account, state, json, send_at, error, now],
            )?;
            if n > 0 {
                return Ok(id);
            }
        }
        db.execute(
            "INSERT INTO outbox(account,state,draft,send_at,error,updated_ms) VALUES(?1,?2,?3,?4,?5,?6)",
            params![account, state, json, send_at, error, now],
        )?;
        Ok(db.last_insert_rowid())
    }

    fn outgoing_rows(&self, cond: &str, p: &[&dyn rusqlite::ToSql]) -> CoreResult<Vec<OutgoingRow>> {
        let db = self.db.lock();
        let mut st = db.prepare(&format!(
            "SELECT id,account,state,draft,send_at,error,updated_ms,attempts FROM outbox WHERE {cond} ORDER BY COALESCE(send_at, updated_ms)"
        ))?;
        let rows = st
            .query_map(p, |r| {
                Ok(OutgoingRow {
                    id: r.get(0)?,
                    account: r.get(1)?,
                    state: r.get(2)?,
                    draft: serde_json::from_str(&r.get::<_, String>(3)?).unwrap_or(Value::Null),
                    send_at: r.get(4)?,
                    error: r.get(5)?,
                    updated_ms: r.get(6)?,
                    attempts: r.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn outgoing(&self, id: i64) -> CoreResult<Option<OutgoingRow>> {
        Ok(self.outgoing_rows("id=?1", &[&id])?.into_iter().next())
    }

    /// Rows in a state (`draft`, `queued`, `sending`, `failed`), oldest first.
    pub fn outgoing_in(&self, state: &str) -> CoreResult<Vec<OutgoingRow>> {
        self.outgoing_rows("state=?1", &[&state])
    }

    /// Queued rows whose send time has come.
    pub fn outgoing_due(&self, now_ms: i64) -> CoreResult<Vec<OutgoingRow>> {
        self.outgoing_rows("state='queued' AND COALESCE(send_at,0)<=?1", &[&now_ms])
    }

    /// Atomically claim a queued row for sending (false if it was cancelled or claimed already).
    pub fn claim_outgoing(&self, id: i64) -> CoreResult<bool> {
        let n = self.db.lock().execute("UPDATE outbox SET state='sending' WHERE id=?1 AND state='queued'", [id])?;
        Ok(n == 1)
    }

    /// A send attempt failed: back in the queue for `retry_at`, or `failed` when that is None.
    pub fn outgoing_failed(&self, id: i64, error: &str, retry_at: Option<i64>) -> CoreResult<()> {
        let now = chrono::Utc::now().timestamp_millis();
        let state = if retry_at.is_some() { "queued" } else { "failed" };
        self.db.lock().execute(
            "UPDATE outbox SET state=?2, error=?3, send_at=COALESCE(?4, send_at), attempts=attempts+1, updated_ms=?5 \
             WHERE id=?1",
            params![id, state, error, retry_at, now],
        )?;
        Ok(())
    }

    pub fn delete_outgoing(&self, id: i64) -> CoreResult<()> {
        self.db.lock().execute("DELETE FROM outbox WHERE id=?1", [id])?;
        Ok(())
    }

    /// `(Message-ID, In-Reply-To, References)` of a stored message.
    pub fn message_headers(&self, message_id: i64) -> CoreResult<Option<ThreadingHeaders>> {
        let db = self.db.lock();
        Ok(db
            .query_row("SELECT message_id, in_reply_to, refs FROM messages WHERE id=?1", [message_id], |r| {
                let refs: Option<String> = r.get(2)?;
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    refs.map(|s| s.split_whitespace().map(str::to_string).collect()).unwrap_or_default(),
                ))
            })
            .optional()?)
    }

    /// Mark a message answered locally (the reply was sent).
    pub fn set_answered(&self, message_id: i64) -> CoreResult<()> {
        self.db.lock().execute("UPDATE messages SET answered=1 WHERE id=?1", [message_id])?;
        Ok(())
    }

    /// Remove the local copies of a message in one folder by its Message-ID header (a sent or
    /// discarded draft leaves Drafts; copies in Sent or Inbox stay).
    pub fn delete_in_folder_by_message_id(&self, folder_id: i64, header_id: &str) -> CoreResult<usize> {
        let db = self.db.lock();
        let tids: Vec<i64> = {
            let mut st = db.prepare("SELECT DISTINCT thread_id FROM messages WHERE folder_id=?1 AND message_id=?2")?;
            let v = st
                .query_map(params![folder_id, header_id], |r| r.get::<_, Option<i64>>(0))?
                .filter_map(|r| r.ok().flatten())
                .collect();
            v
        };
        let n =
            db.execute("DELETE FROM messages WHERE folder_id=?1 AND message_id=?2", params![folder_id, header_id])?;
        for t in tids {
            db.execute(
                "DELETE FROM threads WHERE id=?1 AND NOT EXISTS (SELECT 1 FROM messages WHERE thread_id=?1)",
                [t],
            )?;
        }
        Ok(n)
    }

    /// Ids of the messages of a folder whose subject contains `needle` (newest first).
    pub fn folder_messages_matching(&self, folder_id: i64, needle: &str) -> CoreResult<Vec<i64>> {
        let db = self.db.lock();
        let mut st = db.prepare(
            "SELECT id FROM messages WHERE folder_id=?1 AND instr(lower(subject), lower(?2)) > 0 ORDER BY date_ms DESC",
        )?;
        let v = st.query_map(params![folder_id, needle], |r| r.get(0))?.filter_map(|r| r.ok()).collect();
        Ok(v)
    }

    /// People you have mail with, for address autocomplete: `(name, address, messages)`, most
    /// frequent first. `prefix` matches the start of the name, any word of it, or the address.
    pub fn contacts(&self, prefix: &str, limit: usize) -> CoreResult<Vec<(String, String, i64)>> {
        let p = prefix.trim().to_lowercase();
        let db = self.db.lock();
        let mut out: HashMap<String, (String, i64)> = HashMap::new();
        {
            let mut st = db.prepare(
                "SELECT from_name, from_addr, to_json, cc_json FROM messages ORDER BY date_ms DESC LIMIT 20000",
            )?;
            let rows = st.query_map([], |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?;
            for row in rows {
                let (fname, faddr, to, cc) = row?;
                let mut add = |name: Option<String>, addr: String| {
                    let addr = addr.trim().to_lowercase();
                    if addr.is_empty() || !addr.contains('@') {
                        return;
                    }
                    let e = out.entry(addr).or_insert((String::new(), 0));
                    e.1 += 1;
                    if e.0.is_empty() {
                        if let Some(n) = name.filter(|n| !n.trim().is_empty() && !n.contains('@')) {
                            e.0 = n.trim().to_string();
                        }
                    }
                };
                add(fname, faddr);
                for list in [to, cc] {
                    if let Ok(Value::Array(a)) = serde_json::from_str::<Value>(&list) {
                        for v in a {
                            let addr = v["address"].as_str().or(v["mailbox"].as_str()).unwrap_or("").to_string();
                            add(v["name"].as_str().map(str::to_string), addr);
                        }
                    }
                }
            }
        }
        let mut v: Vec<(String, String, i64)> = out
            .into_iter()
            .filter(|(addr, (name, _))| {
                p.is_empty()
                    || addr.starts_with(&p)
                    || name.to_lowercase().starts_with(&p)
                    || name.to_lowercase().split_whitespace().any(|w| w.starts_with(&p))
            })
            .map(|(addr, (name, n))| (name, addr, n))
            .collect();
        v.sort_by(|a, b| b.2.cmp(&a.2).then(a.1.cmp(&b.1)));
        v.truncate(limit);
        Ok(v)
    }
}

/// Make arbitrary user input safe for an FTS5 MATCH: quote each term, prefix-match the last one.
pub fn fts_query(q: &str) -> String {
    let terms: Vec<String> =
        q.split_whitespace().map(|t| t.replace(['"', '\''], "")).filter(|t| !t.is_empty()).collect();
    let n = terms.len();
    terms
        .into_iter()
        .enumerate()
        .map(|(i, t)| if i + 1 == n { format!("\"{t}\"*") } else { format!("\"{t}\"") })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn acct(s: &Store) {
        s.upsert_account(&Account {
            email: "user@example.com".into(),
            name: "User".into(),
            account_type: "customIMAP".into(),
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            imap_security: "tls".into(),
            smtp_host: "smtp.example.com".into(),
            smtp_port: 587,
            smtp_security: "starttls".into(),
            username: "user@example.com".into(),
        })
        .unwrap();
    }

    fn msg(folder: i64, uid: u32, mid: &str, irt: Option<&str>, subj: &str, date: i64) -> NewMessage {
        NewMessage {
            account: "user@example.com".into(),
            folder_id: folder,
            uid,
            message_id: Some(mid.into()),
            in_reply_to: irt.map(str::to_string),
            refs: irt.map(|s| vec![s.to_string()]).unwrap_or_default(),
            subject: subj.into(),
            from_addr: format!("sender{uid}@example.org"),
            from_name: Some(format!("Sender {uid}")),
            to: json!([]),
            cc: json!([]),
            bcc: json!([]),
            reply_to: json!([]),
            date_ms: date,
            preview: format!("preview {uid} quarterly report"),
            body_text: Some(format!("body {uid} quarterly report")),
            category: "personal".into(),
            ..Default::default()
        }
    }

    #[test]
    fn threading_and_listing() {
        let s = Store::in_memory().unwrap();
        acct(&s);
        let inbox = s.upsert_folder("user@example.com", "INBOX", "Inbox", Some("/"), Some("inbox"), true).unwrap();
        let sent = s.upsert_folder("user@example.com", "Sent", "Sent", Some("/"), Some("sent"), true).unwrap();
        s.insert_message(&msg(inbox, 1, "<a@x>", None, "Hello", 1000)).unwrap();
        s.insert_message(&msg(sent, 1, "<b@x>", Some("<a@x>"), "Re: Hello", 2000)).unwrap();
        s.insert_message(&msg(inbox, 2, "<c@x>", Some("<b@x>"), "Re: Hello", 3000)).unwrap();
        s.insert_message(&msg(inbox, 3, "<d@x>", None, "Other", 1500)).unwrap();
        // idempotent
        s.insert_message(&msg(inbox, 3, "<d@x>", None, "Other", 1500)).unwrap();

        let rows = s
            .thread_rows(&ListFilter::Inbox { account: None, done: false, category: None, with_done: false }, 50)
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].subject, "Hello");
        assert_eq!(rows[0].message_count, 3);
        assert_eq!(rows[0].last_ms, 3000);
        assert!(rows[0].unseen);
        assert_eq!(s.thread_messages(rows[0].thread_id).unwrap().len(), 3);

        s.set_thread_flag(rows[0].thread_id, "done", true).unwrap();
        let rows2 = s
            .thread_rows(&ListFilter::Inbox { account: None, done: false, category: None, with_done: false }, 50)
            .unwrap();
        assert_eq!(rows2.len(), 1);
        assert_eq!(rows2[0].subject, "Other");

        let found = s.thread_rows(&ListFilter::Search("quarter".into()), 50).unwrap();
        assert_eq!(found.len(), 2);
        let sent_rows = s.thread_rows(&ListFilter::Role { account: None, role: "sent".into() }, 50).unwrap();
        assert_eq!(sent_rows.len(), 1);
    }

    #[test]
    fn done_mail_in_archive_stays_reachable() {
        let s = Store::in_memory().unwrap();
        acct(&s);
        let inbox = s.upsert_folder("user@example.com", "INBOX", "Inbox", Some("/"), Some("inbox"), true).unwrap();
        let archive =
            s.upsert_folder("user@example.com", "Archive", "Archive", Some("/"), Some("archive"), true).unwrap();
        s.insert_message(&msg(inbox, 1, "<a@x>", None, "Still here", 1000)).unwrap();
        // marked Done here and moved to Archive by the server: only the archive copy is left
        s.insert_message(&msg(archive, 2, "<b@x>", None, "Handled", 2000)).unwrap();
        // a conversation with one message archived and a newer reply in the inbox is not Done
        s.insert_message(&msg(archive, 3, "<c@x>", None, "Ongoing", 500)).unwrap();
        s.insert_message(&msg(inbox, 4, "<d@x>", Some("<c@x>"), "Re: Ongoing", 3000)).unwrap();
        let f = |done, with_done| ListFilter::Inbox { account: None, done, category: None, with_done };
        let subjects = |rows: Vec<ThreadRow>| rows.into_iter().map(|r| (r.subject, r.done)).collect::<Vec<_>>();
        assert_eq!(
            subjects(s.thread_rows(&f(false, false), 50).unwrap()),
            vec![("Ongoing".to_string(), false), ("Still here".to_string(), false)]
        );
        assert_eq!(subjects(s.thread_rows(&f(true, false), 50).unwrap()), vec![("Handled".to_string(), true)]);
        assert_eq!(
            subjects(s.thread_rows(&f(false, true), 50).unwrap()),
            vec![("Ongoing".to_string(), false), ("Handled".to_string(), true), ("Still here".to_string(), false)]
        );
    }

    #[test]
    fn snooze_hides_until_due_then_returns_unread() {
        let s = Store::in_memory().unwrap();
        acct(&s);
        let inbox = s.upsert_folder("user@example.com", "INBOX", "Inbox", Some("/"), Some("inbox"), true).unwrap();
        s.insert_message(&msg(inbox, 1, "<a@x>", None, "Snooze me", 1000)).unwrap();
        s.insert_message(&msg(inbox, 2, "<b@x>", None, "Stay", 2000)).unwrap();
        let inbox_f = ListFilter::Inbox { account: None, done: false, category: None, with_done: false };
        let rows = s.thread_rows(&inbox_f, 50).unwrap();
        let tid = rows.iter().find(|r| r.subject == "Snooze me").unwrap().thread_id;
        s.set_thread_seen(tid, true).unwrap();

        s.set_snooze(tid, Some(10_000)).unwrap();
        assert_eq!(s.thread_rows(&inbox_f, 50).unwrap().len(), 1);
        let snoozed = s.thread_rows(&ListFilter::Snoozed { account: None }, 50).unwrap();
        assert_eq!(snoozed.len(), 1);
        assert_eq!(snoozed[0].thread_id, tid);

        assert!(s.wake_snoozed(9_999).unwrap().is_empty());
        assert_eq!(s.wake_snoozed(10_000).unwrap(), vec![tid]);
        let back = s.thread_rows(&inbox_f, 50).unwrap();
        assert_eq!(back.len(), 2);
        assert!(back.iter().find(|r| r.thread_id == tid).unwrap().unseen);
        assert!(s.thread_rows(&ListFilter::Snoozed { account: None }, 50).unwrap().is_empty());
    }

    #[test]
    fn sidebar_counts_match_list_queries() {
        let s = Store::in_memory().unwrap();
        acct(&s);
        let inbox = s.upsert_folder("user@example.com", "INBOX", "Inbox", Some("/"), Some("inbox"), true).unwrap();
        let drafts = s.upsert_folder("user@example.com", "Drafts", "Drafts", Some("/"), Some("drafts"), true).unwrap();
        for (uid, cat) in [(1, "personal"), (2, "notifications"), (3, "notifications"), (4, "newsletters")] {
            let mut m = msg(inbox, uid, &format!("<{uid}@x>"), None, &format!("S{uid}"), uid as i64 * 1000);
            m.category = cat.into();
            s.insert_message(&m).unwrap();
        }
        s.insert_message(&msg(drafts, 9, "<d@x>", None, "Draft", 9000)).unwrap();
        let rows = s
            .thread_rows(&ListFilter::Inbox { account: None, done: false, category: None, with_done: false }, 50)
            .unwrap();
        let by = |subj: &str| rows.iter().find(|r| r.subject == subj).unwrap().thread_id;
        s.set_thread_seen(by("S2"), true).unwrap();
        s.set_thread_flag(by("S1"), "pinned", true).unwrap();
        s.set_snooze(by("S4"), Some(99_999)).unwrap();

        let c = s.sidebar_counts().unwrap();
        let inbox_f = |category: Option<&str>| ListFilter::Inbox {
            account: None,
            done: false,
            category: category.map(str::to_string),
            with_done: false,
        };
        assert_eq!(c.inbox_unread, s.count_unseen(&inbox_f(None)).unwrap());
        assert_eq!(c.inbox_unread, 2);
        for cat in ["personal", "notifications", "newsletters"] {
            let (unread, total) = c.categories.get(cat).copied().unwrap_or_default();
            assert_eq!(unread, s.count_unseen(&inbox_f(Some(cat))).unwrap(), "{cat}");
            assert_eq!(total as usize, s.thread_rows(&inbox_f(Some(cat)), 50).unwrap().len(), "{cat}");
        }
        assert_eq!(c.account_unread.get("user@example.com"), Some(&2));
        assert_eq!(c.pinned, 1);
        assert_eq!(c.snoozed, 1);
        assert_eq!(c.drafts, 1);
    }

    #[test]
    fn uidvalidity_change_resets_folder() {
        let s = Store::in_memory().unwrap();
        acct(&s);
        let inbox = s.upsert_folder("user@example.com", "INBOX", "Inbox", None, Some("inbox"), true).unwrap();
        s.set_folder_sync(inbox, 7, 10).unwrap();
        s.insert_message(&msg(inbox, 5, "<z@x>", None, "Z", 1)).unwrap();
        assert!(!s.set_folder_sync(inbox, 7, 11).unwrap());
        assert_eq!(s.max_uid(inbox).unwrap(), 5);
        assert!(s.set_folder_sync(inbox, 8, 1).unwrap());
        assert_eq!(s.max_uid(inbox).unwrap(), 0);
    }

    #[test]
    fn settings_and_secrets() {
        let s = Store::in_memory().unwrap();
        assert_eq!(s.setting("themeMode").unwrap(), None);
        s.set_setting("themeMode", &json!("dark")).unwrap();
        assert_eq!(s.setting("themeMode").unwrap(), Some(json!("dark")));
        s.set_secret("password:a", "pw").unwrap();
        assert_eq!(s.secret("password:a").unwrap().as_deref(), Some("pw"));
    }

    #[test]
    fn fts_query_is_injection_safe() {
        assert_eq!(fts_query("hello wor"), "\"hello\" \"wor\"*");
        assert_eq!(fts_query("a' OR 1=1 --"), "\"a\" \"OR\" \"1=1\" \"--\"*");
    }
}
