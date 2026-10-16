//! Composing: drafts, reply / reply-all / forward templates, and turning a draft into a message.

use crate::mail::smtp::Outgoing;
use crate::store::{MessageRow, Store};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Addr {
    #[serde(default)]
    pub name: Option<String>,
    pub address: String,
}

/// A file to attach: a local file (`path`), or an attachment of a stored message
/// (`from_attachment`, used when forwarding).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftAttachment {
    pub name: String,
    #[serde(default)]
    pub mime: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub from_attachment: Option<i64>,
}

/// What the composer edits. `message_id` is fixed when the draft is created, so every saved copy
/// on the server (Drafts) and the final sent message share it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Draft {
    pub account: String,
    pub from_name: String,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub bcc: Vec<Addr>,
    pub subject: String,
    /// body as HTML (the editor's content, signature and quote included)
    pub html: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub attachments: Vec<DraftAttachment>,
    pub message_id: String,
    /// "new" | "reply" | "replyAll" | "forward" | "draft"
    pub kind: String,
    /// stored message this replies to / forwards (marked answered after sending)
    pub source_message: Option<i64>,
}

impl Draft {
    pub fn from_json(v: &Value) -> Draft {
        serde_json::from_value(v.clone()).unwrap_or_default()
    }

    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    pub fn recipients(&self) -> impl Iterator<Item = &Addr> {
        self.to.iter().chain(&self.cc).chain(&self.bcc)
    }

    /// A draft can be sent: an account, at least one recipient, and only valid addresses.
    pub fn check(&self) -> Result<(), String> {
        if self.account.is_empty() {
            return Err("Choose the account to send from".into());
        }
        if self.recipients().next().is_none() {
            return Err("Add at least one recipient".into());
        }
        if let Some(bad) = self.recipients().find(|a| !valid_address(&a.address)) {
            return Err(format!("\"{}\" is not a valid email address", bad.address));
        }
        Ok(())
    }
}

/// A fresh, unique Message-ID for a draft from `account`.
pub fn new_message_id(account: &str) -> String {
    let domain = account.rsplit('@').next().filter(|d| !d.is_empty()).unwrap_or("localhost");
    format!("<{}@{}>", uuid::Uuid::new_v4(), domain)
}

pub fn valid_address(a: &str) -> bool {
    let a = a.trim();
    match a.split_once('@') {
        Some((l, d)) => {
            !l.is_empty() && d.contains('.') && !d.starts_with('.') && !d.ends_with('.') && !a.contains(' ')
        }
        None => false,
    }
}

fn addrs(v: &Value) -> Vec<Addr> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    let address = x["mailbox"].as_str().or(x["address"].as_str())?.trim().to_string();
                    let name = x["name"].as_str().map(str::to_string).filter(|n| !n.trim().is_empty());
                    (!address.is_empty()).then_some(Addr { name, address })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn plain_to_html(t: &str) -> String {
    esc(t).replace("\r\n", "\n").replace('\n', "<br>")
}

fn prefixed(prefix: &str, subject: &str) -> String {
    let base = crate::mail::normalize_subject(subject);
    format!("{prefix} {base}").trim().to_string()
}

fn who(name: &Option<String>, addr: &str) -> String {
    match name {
        Some(n) if !n.trim().is_empty() => format!("{n} <{addr}>"),
        _ => addr.to_string(),
    }
}

fn date_line(ms: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|d| d.format("%a, %-d %b %Y at %H:%M").to_string())
        .unwrap_or_default()
}

fn original_body(m: &MessageRow) -> String {
    match (&m.body_html, &m.body_text) {
        (Some(h), _) if !h.trim().is_empty() => h.clone(),
        (_, Some(t)) => plain_to_html(t),
        _ => plain_to_html(&m.preview),
    }
}

/// Signature HTML block (empty when there is none).
pub fn signature_block(signature: &str) -> String {
    if signature.trim().is_empty() {
        String::new()
    } else {
        format!("<div class=\"ri-signature\"><br>-- <br>{}</div>", plain_to_html(signature.trim()))
    }
}

/// A new, empty draft from `account`.
pub fn new_draft(store: &Store, account: &str, signature: &str) -> Draft {
    let acc = store.account(account).ok().flatten();
    Draft {
        account: account.to_string(),
        from_name: acc.map(|a| a.name).unwrap_or_default(),
        html: format!("<p><br></p>{}", signature_block(signature)),
        message_id: new_message_id(account),
        kind: "new".into(),
        ..Default::default()
    }
}

/// Reply, reply-all or forward a stored message. `my_addresses` are left out of reply-all.
pub fn respond(store: &Store, message_id: i64, kind: &str, my_addresses: &[String], signature: &str) -> Option<Draft> {
    let (_tid, m) = store.message(message_id).ok().flatten()?;
    let (hdr_id, _irt, refs) = store.message_headers(m.id).ok().flatten().unwrap_or((None, None, vec![]));
    let account = m.account.clone();
    let mine = |a: &str| my_addresses.iter().any(|x| x.eq_ignore_ascii_case(a)) || a.eq_ignore_ascii_case(&account);
    let mut d = new_draft(store, &account, signature);
    d.kind = kind.to_string();
    d.source_message = Some(m.id);
    let quote_head;
    match kind {
        "forward" => {
            d.subject = prefixed("Fwd:", &m.subject);
            let to = addrs(&m.to).iter().map(|a| esc(&who(&a.name, &a.address))).collect::<Vec<_>>().join(", ");
            quote_head = format!(
                "<div>---------- Forwarded message ----------<br>From: {}<br>Date: {}<br>Subject: {}<br>To: {}</div><br>",
                esc(&who(&m.from_name, &m.from_addr)),
                esc(&date_line(m.date_ms)),
                esc(&m.subject),
                to
            );
            d.attachments = m
                .attachments
                .iter()
                .filter(|(_, _, _, _, inline)| !inline)
                .map(|(id, name, mime, size, _)| DraftAttachment {
                    name: name.clone(),
                    mime: mime.clone(),
                    size: (*size).max(0) as u64,
                    path: None,
                    from_attachment: Some(*id),
                })
                .collect();
        }
        _ => {
            d.subject = prefixed("Re:", &m.subject);
            // you replying to your own sent message writes to its recipients again
            let from_me = mine(&m.from_addr);
            let reply_to = addrs(&m.reply_to);
            let primary: Vec<Addr> = if from_me {
                addrs(&m.to)
            } else if !reply_to.is_empty() {
                reply_to
            } else {
                vec![Addr { name: m.from_name.clone(), address: m.from_addr.clone() }]
            };
            d.to = primary;
            if kind == "replyAll" {
                let mut seen: Vec<String> = d.to.iter().map(|a| a.address.to_lowercase()).collect();
                let others = addrs(&m.to).into_iter().chain(addrs(&m.cc));
                for a in others {
                    let key = a.address.to_lowercase();
                    if !mine(&a.address) && !seen.contains(&key) {
                        seen.push(key);
                        d.cc.push(a);
                    }
                }
            }
            d.to.retain(|a| !mine(&a.address) || from_me);
            d.in_reply_to = hdr_id.clone();
            let mut r = refs;
            if let Some(h) = &hdr_id {
                if !r.contains(h) {
                    r.push(h.clone());
                }
            }
            d.references = r;
            quote_head = format!(
                "<div>On {}, {} wrote:</div>",
                esc(&date_line(m.date_ms)),
                esc(&who(&m.from_name, &m.from_addr))
            );
        }
    }
    d.html = format!(
        "<p><br></p>{}<br>{}<blockquote class=\"ri-quote\" style=\"margin:0 0 0 .8ex;border-left:2px solid #ccc;padding-left:1ex\">{}</blockquote>",
        signature_block(signature),
        quote_head,
        original_body(&m)
    );
    Some(d)
}

/// Open a stored draft (a message in a Drafts folder) for editing.
pub fn from_stored_draft(store: &Store, message_id: i64) -> Option<Draft> {
    let (_tid, m) = store.message(message_id).ok().flatten()?;
    let (hdr_id, irt, refs) = store.message_headers(m.id).ok().flatten().unwrap_or((None, None, vec![]));
    Some(Draft {
        account: m.account.clone(),
        from_name: m.from_name.clone().unwrap_or_default(),
        to: addrs(&m.to),
        cc: addrs(&m.cc),
        bcc: addrs(&m.bcc),
        subject: m.subject.clone(),
        html: original_body(&m),
        in_reply_to: irt,
        references: refs,
        attachments: vec![],
        message_id: hdr_id.unwrap_or_else(|| new_message_id(&m.account)),
        kind: "draft".into(),
        source_message: None,
    })
}

/// Plain-text alternative of an HTML body (tags dropped, blocks become lines).
pub fn html_to_text(html: &str) -> String {
    let mut s = html.to_string();
    for (pat, rep) in [
        ("<br>", "\n"),
        ("<br/>", "\n"),
        ("<br />", "\n"),
        ("</p>", "\n\n"),
        ("</div>", "\n"),
        ("</li>", "\n"),
        ("</blockquote>", "\n"),
    ] {
        s = s.replace(pat, rep).replace(&pat.to_uppercase(), rep);
    }
    let mut out = String::new();
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&");
    let mut lines: Vec<&str> = out.lines().map(str::trim_end).collect();
    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    let mut res = vec![];
    let mut blank = 0;
    for l in lines {
        if l.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        res.push(l);
    }
    res.join("\n").trim_end().to_string()
}

/// The message to send / save for a draft. Attachments are read by the caller (`files`).
pub fn outgoing(d: &Draft, files: Vec<(String, String, Vec<u8>)>) -> Outgoing {
    let pair = |a: &Addr| (a.name.clone(), a.address.trim().to_string());
    Outgoing {
        from_name: Some(d.from_name.clone()).filter(|n| !n.is_empty()),
        from: d.account.clone(),
        to: d.to.iter().map(pair).collect(),
        cc: d.cc.iter().map(pair).collect(),
        bcc: d.bcc.iter().map(pair).collect(),
        subject: d.subject.clone(),
        text: html_to_text(&d.html),
        html: Some(format!("<html><body>{}</body></html>", d.html)),
        in_reply_to: d.in_reply_to.clone(),
        references: d.references.clone(),
        attachments: files,
        message_id: Some(d.message_id.clone()).filter(|m| !m.is_empty()),
    }
}

/// Best-effort MIME type from a file name.
pub fn mime_for(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "txt" | "log" | "md" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "zip" => "application/zip",
        "gz" | "tgz" => "application/gzip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "mp4" => "video/mp4",
        "mp3" => "audio/mpeg",
        "ics" => "text/calendar",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Account, NewMessage};
    use serde_json::json;

    fn store() -> (Store, i64) {
        let s = Store::in_memory().unwrap();
        s.upsert_account(&Account {
            email: "me@example.com".into(),
            name: "Me Myself".into(),
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
        let mid = s
            .insert_message(&NewMessage {
                account: "me@example.com".into(),
                folder_id: inbox,
                uid: 7,
                message_id: Some("<orig@example.org>".into()),
                refs: vec!["<root@example.org>".into()],
                subject: "Re: Quarterly plan".into(),
                from_name: Some("Ann Lee".into()),
                from_addr: "ann@example.org".into(),
                to: json!([{"name": "Me", "mailbox": "me@example.com"}, {"name": "Bob", "mailbox": "bob@example.org"}]),
                cc: json!([{"name": null, "mailbox": "carol@example.org"}, {"name": "Ann", "mailbox": "ann@example.org"}]),
                bcc: json!([]),
                reply_to: json!([]),
                date_ms: 1_700_000_000_000,
                body_html: Some("<p>Here is the plan.</p>".into()),
                category: "personal".into(),
                ..Default::default()
            })
            .unwrap();
        (s, mid)
    }

    #[test]
    fn reply_targets_sender_and_threads() {
        let (s, mid) = store();
        let d = respond(&s, mid, "reply", &[], "Me\nExample Inc.").unwrap();
        assert_eq!(d.subject, "Re: Quarterly plan");
        assert_eq!(d.to, vec![Addr { name: Some("Ann Lee".into()), address: "ann@example.org".into() }]);
        assert!(d.cc.is_empty());
        assert_eq!(d.in_reply_to.as_deref(), Some("<orig@example.org>"));
        assert_eq!(d.references, vec!["<root@example.org>", "<orig@example.org>"]);
        assert!(d.html.contains("Ann Lee &lt;ann@example.org&gt; wrote:"));
        assert!(d.html.contains("<p>Here is the plan.</p>"));
        assert!(d.html.contains("Me<br>Example Inc."), "signature: {}", d.html);
        assert!(d.message_id.ends_with("@example.com>"));
        assert_eq!(d.source_message, Some(mid));
    }

    #[test]
    fn reply_all_adds_everyone_but_me_once() {
        let (s, mid) = store();
        let d = respond(&s, mid, "replyAll", &[], "").unwrap();
        assert_eq!(d.to.len(), 1);
        let cc: Vec<&str> = d.cc.iter().map(|a| a.address.as_str()).collect();
        assert_eq!(cc, vec!["bob@example.org", "carol@example.org"]);
    }

    #[test]
    fn forward_has_header_block_and_no_recipients() {
        let (s, mid) = store();
        let d = respond(&s, mid, "forward", &[], "").unwrap();
        assert_eq!(d.subject, "Fwd: Quarterly plan");
        assert!(d.to.is_empty() && d.in_reply_to.is_none());
        assert!(d.html.contains("Forwarded message"));
        assert!(d.html.contains("Subject: Re: Quarterly plan"));
    }

    #[test]
    fn drafts_check_recipients() {
        let (s, _) = store();
        let mut d = new_draft(&s, "me@example.com", "");
        assert_eq!(d.from_name, "Me Myself");
        assert!(d.check().is_err());
        d.to.push(Addr { name: None, address: "not an address".into() });
        assert!(d.check().unwrap_err().contains("not a valid"));
        d.to[0].address = "x@example.org".into();
        assert!(d.check().is_ok());
        let back = Draft::from_json(&d.to_json());
        assert_eq!(back, d);
    }

    #[test]
    fn html_becomes_readable_text() {
        let t =
            html_to_text("<p>Hi <b>Ann</b>,</p><p>Line one<br>Line two</p><blockquote>quoted &amp; kept</blockquote>");
        assert_eq!(t, "Hi Ann,\n\nLine one\nLine two\n\nquoted & kept");
    }

    #[test]
    fn outgoing_carries_the_stable_message_id() {
        let (s, mid) = store();
        let d = respond(&s, mid, "reply", &[], "").unwrap();
        let o = outgoing(&d, vec![]);
        let (raw, id) = crate::mail::smtp::build(&o).unwrap();
        assert_eq!(id, d.message_id);
        let txt = String::from_utf8_lossy(&raw);
        assert!(txt.contains("In-Reply-To: <orig@example.org>"));
        assert_eq!(mime_for("Report.PDF"), "application/pdf");
    }

    #[test]
    fn outbox_rows_roundtrip() {
        let (s, _) = store();
        let d = new_draft(&s, "me@example.com", "");
        let id = s.put_outgoing(0, "me@example.com", "draft", &d.to_json(), None, None).unwrap();
        assert_eq!(s.outgoing_in("draft").unwrap().len(), 1);
        s.put_outgoing(id, "me@example.com", "queued", &d.to_json(), Some(10), None).unwrap();
        assert!(s.outgoing_due(5).unwrap().is_empty());
        assert_eq!(s.outgoing_due(10).unwrap().len(), 1);
        assert!(s.claim_outgoing(id).unwrap());
        assert!(!s.claim_outgoing(id).unwrap(), "claimed once");
        s.delete_outgoing(id).unwrap();
        assert!(s.outgoing(id).unwrap().is_none());
    }

    #[test]
    fn contacts_rank_by_frequency_and_match_prefixes() {
        let (s, _) = store();
        let c = s.contacts("", 10).unwrap();
        assert!(c.iter().any(|(_, a, _)| a == "ann@example.org"));
        let ann = s.contacts("lee", 10).unwrap();
        assert_eq!(ann.first().map(|x| x.1.as_str()), Some("ann@example.org"));
        assert!(s.contacts("zzz", 10).unwrap().is_empty());
    }
}
