//! SMTP sending (lettre) and outgoing message construction.

use crate::store::Account;
use anyhow::{Context, Result};
use lettre::message::{header, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct Outgoing {
    pub from_name: Option<String>,
    pub from: String,
    pub to: Vec<(Option<String>, String)>,
    pub cc: Vec<(Option<String>, String)>,
    pub bcc: Vec<(Option<String>, String)>,
    pub subject: String,
    pub text: String,
    pub html: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub attachments: Vec<(String, String, Vec<u8>)>,
    /// keep this Message-ID (drafts keep theirs across saves); a new one otherwise
    pub message_id: Option<String>,
}

fn mailbox(name: &Option<String>, addr: &str) -> Result<Mailbox> {
    Ok(Mailbox::new(name.clone(), addr.parse().with_context(|| format!("bad address {addr}"))?))
}

/// Build the RFC 822 message. Returns (raw bytes, Message-ID).
pub fn build(o: &Outgoing) -> Result<(Vec<u8>, String)> {
    let domain = o.from.rsplit('@').next().unwrap_or("localhost");
    let mid = o.message_id.clone().unwrap_or_else(|| format!("<{}@{}>", uuid::Uuid::new_v4(), domain));
    let mut b = Message::builder()
        .from(mailbox(&o.from_name, &o.from)?)
        .subject(o.subject.clone())
        .message_id(Some(mid.clone()));
    for (n, a) in &o.to {
        b = b.to(mailbox(n, a)?);
    }
    for (n, a) in &o.cc {
        b = b.cc(mailbox(n, a)?);
    }
    for (n, a) in &o.bcc {
        b = b.bcc(mailbox(n, a)?);
    }
    if let Some(irt) = &o.in_reply_to {
        b = b.in_reply_to(irt.clone());
    }
    if !o.references.is_empty() {
        b = b.references(o.references.join(" "));
    }
    let body = match &o.html {
        Some(h) => MultiPart::alternative_plain_html(o.text.clone(), h.clone()),
        None => MultiPart::mixed().singlepart(SinglePart::plain(o.text.clone())),
    };
    let msg = if o.attachments.is_empty() {
        b.multipart(body)?
    } else {
        let mut mixed = MultiPart::mixed().multipart(body);
        for (name, mime, bytes) in &o.attachments {
            let ct =
                header::ContentType::parse(mime).unwrap_or(header::ContentType::parse("application/octet-stream")?);
            mixed = mixed.singlepart(lettre::message::Attachment::new(name.clone()).body(bytes.clone(), ct));
        }
        b.multipart(mixed)?
    };
    Ok((local_date(msg.formatted()), mid))
}

/// lettre always stamps `Date:` in UTC; desktop clients use local time with its offset.
fn local_date(raw: Vec<u8>) -> Vec<u8> {
    let text = String::from_utf8_lossy(&raw);
    let Some(start) = text.find("Date: ") else { return raw };
    let Some(len) = text[start..].find("\r\n") else { return raw };
    let line = format!("Date: {}", chrono::Local::now().to_rfc2822());
    let mut out = Vec::with_capacity(raw.len() + 8);
    out.extend_from_slice(&raw[..start]);
    out.extend_from_slice(line.as_bytes());
    out.extend_from_slice(&raw[start + len..]);
    out
}

pub async fn send(acct: &Account, password: &str, o: &Outgoing) -> Result<(Vec<u8>, String)> {
    let (raw, mid) = build(o)?;
    let builder = match acct.smtp_security.as_str() {
        "tls" => AsyncSmtpTransport::<Tokio1Executor>::relay(&acct.smtp_host)?,
        "starttls" => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&acct.smtp_host)?,
        _ => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(acct.smtp_host.clone()),
    };
    let transport = builder
        .port(acct.smtp_port)
        .credentials(Credentials::new(acct.username.clone(), password.to_string()))
        .timeout(Some(Duration::from_secs(60)))
        .build();
    let mut rcpt: Vec<lettre::Address> = vec![];
    for (_, a) in o.to.iter().chain(&o.cc).chain(&o.bcc) {
        rcpt.push(a.parse()?);
    }
    let env = lettre::address::Envelope::new(Some(o.from.parse()?), rcpt)?;
    transport.send_raw(&env, &raw).await.context("SMTP send")?;
    Ok((raw, mid))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_reply_with_threading_headers() {
        let o = Outgoing {
            from_name: Some("User".into()),
            from: "user@example.com".into(),
            to: vec![(None, "bob@example.org".into())],
            subject: "Re: Hello".into(),
            text: "hi".into(),
            html: Some("<p>hi</p>".into()),
            in_reply_to: Some("<a@x>".into()),
            references: vec!["<a@x>".into()],
            attachments: vec![("a.txt".into(), "text/plain".into(), b"x".to_vec())],
            ..Default::default()
        };
        let (raw, mid) = build(&o).unwrap();
        let s = String::from_utf8_lossy(&raw);
        assert!(s.contains("In-Reply-To: <a@x>"));
        assert!(s.contains("References: <a@x>"));
        assert!(s.contains(&format!("Message-ID: {mid}")));
        let parsed = crate::mail::mime::parse(&raw, true).unwrap();
        assert_eq!(parsed.subject, "Re: Hello");
        assert_eq!(parsed.attachments.len(), 1);
        let offset = chrono::Local::now().format("%z").to_string();
        let date = s.lines().find(|l| l.starts_with("Date: ")).unwrap();
        assert!(date.ends_with(&offset), "{date} should carry local offset {offset}");
        assert_eq!(s.matches("\r\nDate: ").count(), 1);
    }
}
