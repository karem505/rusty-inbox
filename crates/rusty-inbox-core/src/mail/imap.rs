//! IMAP client: connect (implicit TLS / STARTTLS / plain), folder discovery, incremental sync,
//! flag/move operations and IDLE.

use crate::mail::{mime, tls};
use crate::store::{Account, Store};
use anyhow::{anyhow, Context, Result};
use async_imap::types::{Fetch, Flag, NameAttribute};
use futures::TryStreamExt;
use std::fmt::Debug;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

pub trait Io: AsyncRead + AsyncWrite + Unpin + Send + Debug {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + Debug> Io for T {}

pub type Session = async_imap::Session<Box<dyn Io>>;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const OP_TIMEOUT: Duration = Duration::from_secs(120);

/// How many of the newest messages per folder the first sync downloads.
pub const INITIAL_WINDOW: u32 = 300;
/// Messages bigger than this are stored header-only; the body is fetched when opened.
pub const FULL_FETCH_LIMIT: u32 = 3 * 1024 * 1024;

pub struct Connection {
    pub session: Session,
    /// account this connection is logged in to (the engine's connection pool key)
    pub account: String,
    /// when it was last handed back to the pool
    pub idle_since: std::time::Instant,
}

async fn timed<T>(what: &str, f: impl std::future::Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(OP_TIMEOUT, f).await.map_err(|_| anyhow!("IMAP {what} timed out"))?
}

impl Connection {
    pub async fn connect(acct: &Account, password: &str) -> Result<Connection> {
        let addr = (acct.imap_host.as_str(), acct.imap_port);
        let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
            .await
            .map_err(|_| anyhow!("connect to {}:{} timed out", acct.imap_host, acct.imap_port))?
            .with_context(|| format!("connect to {}:{}", acct.imap_host, acct.imap_port))?;
        tcp.set_nodelay(true).ok();
        let stream: Box<dyn Io> = match acct.imap_security.as_str() {
            "tls" => {
                let tls = tls::connector().connect(tls::server_name(&acct.imap_host)?, tcp).await?;
                Box::new(tls)
            }
            "starttls" => {
                let mut client = async_imap::Client::new(tcp);
                let _greeting = client.read_response().await?;
                client.run_command_and_check_ok("STARTTLS", None).await?;
                let tcp = client.into_inner();
                let tls = tls::connector().connect(tls::server_name(&acct.imap_host)?, tcp).await?;
                Box::new(tls)
            }
            _ => Box::new(tcp),
        };
        let mut client = async_imap::Client::new(stream);
        if acct.imap_security != "starttls" {
            let _greeting = timed("greeting", async { Ok(client.read_response().await?) }).await?;
        }
        let session = timed("login", async {
            client.login(&acct.username, password).await.map_err(|(e, _)| anyhow!("IMAP login failed: {e}"))
        })
        .await?;
        Ok(Connection { session, account: acct.email.clone(), idle_since: std::time::Instant::now() })
    }

    /// Cheap liveness check of a pooled connection.
    pub async fn noop(&mut self) -> bool {
        matches!(tokio::time::timeout(Duration::from_secs(8), self.session.noop()).await, Ok(Ok(_)))
    }

    pub async fn logout(&mut self) -> Result<()> {
        timed("logout", async { Ok(self.session.logout().await?) }).await
    }

    /// LIST all folders and classify their roles (RFC 6154 special-use first, names second).
    pub async fn list_folders(&mut self) -> Result<Vec<RemoteFolder>> {
        let names: Vec<_> =
            timed("list", async { Ok(self.session.list(Some(""), Some("*")).await?.try_collect::<Vec<_>>().await?) })
                .await?;
        let mut out = vec![];
        for n in &names {
            let attrs = n.attributes();
            let selectable = !attrs.iter().any(|a| {
                matches!(a, NameAttribute::NoSelect)
                    || matches!(a, NameAttribute::Extension(e) if e.eq_ignore_ascii_case("\\NonExistent"))
            });
            let mut role = attrs.iter().find_map(|a| match a {
                NameAttribute::Sent => Some("sent"),
                NameAttribute::Drafts => Some("drafts"),
                NameAttribute::Trash => Some("trash"),
                NameAttribute::Junk => Some("spam"),
                NameAttribute::Archive => Some("archive"),
                NameAttribute::All => Some("all"),
                NameAttribute::Flagged => Some("flagged"),
                _ => None,
            });
            if role.is_none() {
                role = role_from_name(n.name(), n.delimiter());
            }
            out.push(RemoteFolder {
                path: n.name().to_string(),
                name: display_name(n.name(), n.delimiter()),
                delimiter: n.delimiter().map(str::to_string),
                role: role.map(str::to_string),
                selectable,
            });
        }
        // only one folder per role (prefer special-use / shortest path)
        let mut seen = std::collections::HashSet::new();
        out.sort_by_key(|f| (f.role.is_none(), f.path.len()));
        for f in out.iter_mut() {
            if let Some(r) = &f.role {
                if !seen.insert(r.clone()) {
                    f.role = None;
                }
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    /// Incrementally sync one folder into the store. Returns the number of new messages.
    pub async fn sync_folder(&mut self, store: &Store, account: &str, folder_id: i64, path: &str) -> Result<SyncStats> {
        let mb = timed("select", async { Ok(self.session.select(path).await?) }).await?;
        let uidvalidity = mb.uid_validity.unwrap_or(0);
        let uidnext = mb.uid_next.unwrap_or(0);
        store.set_folder_sync(folder_id, uidvalidity, uidnext)?;
        let mut stats = SyncStats { exists: mb.exists, ..Default::default() };
        if mb.exists == 0 {
            let local = store.uids(folder_id)?;
            stats.removed = store.delete_uids(folder_id, &local)?;
            return Ok(stats);
        }

        // current UID set on the server (bounded to the window for big folders)
        let all: Vec<u32> = {
            let set = timed("search", async { Ok(self.session.uid_search("ALL").await?) }).await?;
            let mut v: Vec<u32> = set.into_iter().collect();
            v.sort_unstable();
            v
        };
        let window: Vec<u32> = all.iter().rev().take(INITIAL_WINDOW as usize).rev().copied().collect();
        let min_window = window.first().copied().unwrap_or(0);

        // removals (only within what we track)
        let server: std::collections::HashSet<u32> = all.iter().copied().collect();
        let local = store.uids(folder_id)?;
        let gone: Vec<u32> = local.iter().copied().filter(|u| !server.contains(u)).collect();
        stats.removed = store.delete_uids(folder_id, &gone)?;

        // new messages
        let have: std::collections::HashSet<u32> = local.into_iter().collect();
        let missing: Vec<u32> = window.iter().copied().filter(|u| !have.contains(u)).collect();
        if !missing.is_empty() {
            // sizes first so huge mails are fetched header-only
            let sizes = self.fetch_sizes(&missing).await?;
            let (small, big): (Vec<u32>, Vec<u32>) =
                missing.iter().partition(|u| sizes.get(u).copied().unwrap_or(0) <= FULL_FETCH_LIMIT);
            for chunk in small.chunks(25) {
                let fetches = self.uid_fetch(chunk, "(UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[])").await?;
                for f in &fetches {
                    if let Some(m) = to_message(f, account, folder_id, true) {
                        store.insert_message(&m)?;
                        stats.new += 1;
                    }
                }
            }
            for chunk in big.chunks(50) {
                let fetches = self.uid_fetch(chunk, "(UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[HEADER])").await?;
                for f in &fetches {
                    if let Some(m) = to_message(f, account, folder_id, false) {
                        store.insert_message(&m)?;
                        stats.new += 1;
                    }
                }
            }
        }

        // flag refresh for the tracked window
        if min_window > 0 {
            let fetches = self.uid_fetch_range(&format!("{min_window}:*"), "(UID FLAGS)").await?;
            for f in &fetches {
                if let Some(uid) = f.uid {
                    let (seen, flagged, ..) = flags_of(f);
                    store.set_flags(folder_id, uid, seen, flagged)?;
                }
            }
        }
        Ok(stats)
    }

    async fn fetch_sizes(&mut self, uids: &[u32]) -> Result<std::collections::HashMap<u32, u32>> {
        let mut out = std::collections::HashMap::new();
        for chunk in uids.chunks(500) {
            for f in self.uid_fetch(chunk, "(UID RFC822.SIZE)").await? {
                if let (Some(u), Some(s)) = (f.uid, f.size) {
                    out.insert(u, s);
                }
            }
        }
        Ok(out)
    }

    pub async fn uid_fetch(&mut self, uids: &[u32], query: &str) -> Result<Vec<Fetch>> {
        self.uid_fetch_range(&uid_set(uids), query).await
    }

    pub async fn uid_fetch_range(&mut self, set: &str, query: &str) -> Result<Vec<Fetch>> {
        timed("fetch", async { Ok(self.session.uid_fetch(set, query).await?.try_collect::<Vec<_>>().await?) }).await
    }

    /// Download one full message (used when opening a header-only message).
    pub async fn fetch_full(&mut self, path: &str, uid: u32) -> Result<Option<Vec<u8>>> {
        timed("select", async { Ok(self.session.select(path).await?) }).await?;
        let f = self.uid_fetch(&[uid], "(UID BODY.PEEK[])").await?;
        Ok(f.into_iter().find_map(|f| f.body().map(|b| b.to_vec())))
    }

    pub async fn store_flags(&mut self, path: &str, uids: &[u32], op: &str, flags: &str) -> Result<()> {
        if uids.is_empty() {
            return Ok(());
        }
        timed("select", async { Ok(self.session.select(path).await?) }).await?;
        let q = format!("{op}FLAGS.SILENT ({flags})");
        let set = uid_set(uids);
        timed("store", async {
            let _ = self.session.uid_store(&set, &q).await?.try_collect::<Vec<_>>().await?;
            Ok(())
        })
        .await
    }

    /// Move messages (MOVE when supported, otherwise COPY + \Deleted + UID EXPUNGE).
    pub async fn move_uids(&mut self, path: &str, uids: &[u32], dest: &str) -> Result<()> {
        if uids.is_empty() {
            return Ok(());
        }
        timed("select", async { Ok(self.session.select(path).await?) }).await?;
        let set = uid_set(uids);
        let caps = timed("capability", async { Ok(self.session.capabilities().await?) }).await?;
        if caps.has_str("MOVE") {
            timed("move", async { Ok(self.session.uid_mv(&set, dest).await?) }).await
        } else {
            timed("copy", async { Ok(self.session.uid_copy(&set, dest).await?) }).await?;
            timed("store", async {
                let _ =
                    self.session.uid_store(&set, "+FLAGS.SILENT (\\Deleted)").await?.try_collect::<Vec<_>>().await?;
                Ok(())
            })
            .await?;
            timed("expunge", async {
                let _ = self.session.uid_expunge(&set).await?.try_collect::<Vec<_>>().await?;
                Ok(())
            })
            .await
        }
    }

    pub async fn delete_permanently(&mut self, path: &str, uids: &[u32]) -> Result<()> {
        self.store_flags(path, uids, "+", "\\Deleted").await?;
        let set = uid_set(uids);
        timed("expunge", async {
            let _ = self.session.uid_expunge(&set).await?.try_collect::<Vec<_>>().await?;
            Ok(())
        })
        .await
    }

    pub async fn append(&mut self, path: &str, flags: &str, raw: &[u8]) -> Result<()> {
        timed("append", async { Ok(self.session.append(path, Some(flags), None, raw).await?) }).await
    }

    /// Permanently remove the message(s) carrying `message_id` (used to replace saved drafts).
    pub async fn delete_by_message_id(&mut self, path: &str, message_id: &str) -> Result<()> {
        timed("select", async { Ok(self.session.select(path).await?) }).await?;
        let q = format!("HEADER Message-ID \"{}\"", message_id.replace('"', ""));
        let found = timed("search", async { Ok(self.session.uid_search(&q).await?) }).await?;
        let uids: Vec<u32> = found.into_iter().collect();
        if uids.is_empty() {
            return Ok(());
        }
        self.delete_permanently(path, &uids).await
    }

    /// Server-side full-text search in one folder (`TEXT`), returns UIDs.
    pub async fn search_text(&mut self, path: &str, text: &str) -> Result<Vec<u32>> {
        timed("examine", async { Ok(self.session.examine(path).await?) }).await?;
        let q = format!("TEXT \"{}\"", text.replace(['"', '\\'], ""));
        let found = timed("search", async { Ok(self.session.uid_search(&q).await?) }).await?;
        Ok(found.into_iter().collect())
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SyncStats {
    pub exists: u32,
    pub new: usize,
    pub removed: usize,
}

#[derive(Debug, Clone)]
pub struct RemoteFolder {
    pub path: String,
    pub name: String,
    pub delimiter: Option<String>,
    pub role: Option<String>,
    pub selectable: bool,
}

pub fn role_from_name(path: &str, delim: Option<&str>) -> Option<&'static str> {
    if path.eq_ignore_ascii_case("INBOX") {
        return Some("inbox");
    }
    let leaf = display_name(path, delim).to_lowercase();
    match leaf.as_str() {
        "sent" | "sent items" | "sent messages" | "sent mail" | "المرسل" | "البريد المرسل" => {
            Some("sent")
        }
        "drafts" | "draft" | "المسودات" => Some("drafts"),
        "trash" | "deleted" | "deleted items" | "deleted messages" | "bin" | "المحذوفات" => Some("trash"),
        "spam" | "junk" | "junk e-mail" | "junk email" | "bulk mail" | "البريد العشوائي" => Some("spam"),
        "archive" | "archives" | "الأرشيف" => Some("archive"),
        _ => None,
    }
}

pub fn display_name(path: &str, delim: Option<&str>) -> String {
    let leaf = match delim {
        Some(d) if !d.is_empty() => path.rsplit(d).next().unwrap_or(path),
        _ => path,
    };
    if path.eq_ignore_ascii_case("INBOX") {
        "Inbox".into()
    } else {
        decode_utf7(leaf)
    }
}

/// Modified UTF-7 (RFC 3501 §5.1.3) mailbox-name decoding.
pub fn decode_utf7(s: &str) -> String {
    use base64::Engine;
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let Some(j) = after.find('-') else {
            out.push_str(&rest[i..]);
            return out;
        };
        let enc = &after[..j];
        if enc.is_empty() {
            out.push('&');
        } else {
            let b64 = enc.replace(',', "/");
            match base64::engine::general_purpose::STANDARD_NO_PAD.decode(b64.as_bytes()) {
                Ok(bytes) => {
                    let units: Vec<u16> =
                        bytes.chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                    out.push_str(&String::from_utf16_lossy(&units));
                }
                Err(_) => out.push_str(&rest[i..i + 2 + j]),
            }
        }
        rest = &after[j + 1..];
    }
    out.push_str(rest);
    out
}

pub fn uid_set(uids: &[u32]) -> String {
    let mut v = uids.to_vec();
    v.sort_unstable();
    v.dedup();
    let mut parts: Vec<String> = vec![];
    let mut i = 0;
    while i < v.len() {
        let start = v[i];
        let mut end = start;
        while i + 1 < v.len() && v[i + 1] == end + 1 {
            i += 1;
            end = v[i];
        }
        parts.push(if start == end { start.to_string() } else { format!("{start}:{end}") });
        i += 1;
    }
    parts.join(",")
}

fn flags_of(f: &Fetch) -> (bool, bool, bool, bool) {
    let (mut seen, mut flagged, mut answered, mut draft) = (false, false, false, false);
    for fl in f.flags() {
        match fl {
            Flag::Seen => seen = true,
            Flag::Flagged => flagged = true,
            Flag::Answered => answered = true,
            Flag::Draft => draft = true,
            _ => {}
        }
    }
    (seen, flagged, answered, draft)
}

fn to_message(f: &Fetch, account: &str, folder_id: i64, full: bool) -> Option<crate::store::NewMessage> {
    let uid = f.uid?;
    let raw = if full { f.body() } else { f.header() }?;
    let mut m = mime::parse(raw, full)?;
    let (seen, flagged, answered, draft) = flags_of(f);
    m.account = account.to_string();
    m.folder_id = folder_id;
    m.uid = uid;
    m.seen = seen;
    m.flagged = flagged;
    m.answered = answered;
    m.draft = draft;
    m.size = f.size.unwrap_or(raw.len() as u32);
    if m.date_ms == 0 {
        m.date_ms = f.internal_date().map(|d| d.timestamp_millis()).unwrap_or(0);
    }
    Some(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uid_sets_compress() {
        assert_eq!(uid_set(&[5, 1, 2, 3, 9, 10]), "1:3,5,9:10");
        assert_eq!(uid_set(&[7]), "7");
    }

    #[test]
    fn utf7_names() {
        assert_eq!(decode_utf7("INBOX.Sent"), "INBOX.Sent");
        assert_eq!(decode_utf7("&BB4EQgQ,BEAEMAQyBDsENQQ9BD0ESwQ1-"), "Отправленные");
        assert_eq!(decode_utf7("INBOX.&BCEEPwQwBDw-"), "INBOX.Спам");
        assert_eq!(decode_utf7("A&-B"), "A&B");
    }

    #[test]
    fn roles_from_names() {
        assert_eq!(role_from_name("INBOX", Some(".")), Some("inbox"));
        assert_eq!(role_from_name("INBOX.Sent", Some(".")), Some("sent"));
        assert_eq!(role_from_name("INBOX.Junk", Some(".")), Some("spam"));
        assert_eq!(role_from_name("[Gmail]/Sent Mail", Some("/")), Some("sent"));
        assert_eq!(role_from_name("Projects", Some("/")), None);
        assert_eq!(display_name("INBOX.Archive", Some(".")), "Archive");
    }
}
