//! RFC 5322 / MIME parsing into store rows (mail-parser).

use crate::store::{NewAttachment, NewMessage};
use mail_parser::{Address, HeaderValue, MessageParser, MimeHeaders};
use serde_json::{json, Value};

pub fn addresses(a: Option<&Address<'_>>) -> Value {
    let Some(a) = a else { return json!([]) };
    Value::Array(
        a.iter()
            .filter_map(|x| {
                let mb = x.address.as_deref()?.trim().to_string();
                let name = x.name.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
                Some(json!({"name": name, "mailbox": mb}))
            })
            .collect(),
    )
}

fn ids(v: &HeaderValue<'_>) -> Vec<String> {
    let fmt = |s: &str| {
        let s = s.trim().trim_matches(|c| c == '<' || c == '>');
        format!("<{s}>")
    };
    match v {
        HeaderValue::Text(t) => vec![fmt(t)],
        HeaderValue::TextList(l) => l.iter().map(|t| fmt(t)).collect(),
        _ => vec![],
    }
}

/// Collapse whitespace and cut to a list preview.
pub fn preview_of(text: &str, max: usize) -> String {
    let mut out = String::new();
    let mut last_space = true;
    for c in text.chars() {
        if c.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(c);
            last_space = false;
        }
        if out.chars().count() >= max {
            break;
        }
    }
    out.trim().to_string()
}

/// Very small HTML-to-text for previews/search (bodies keep their HTML for display).
pub fn html_to_text(html: &str) -> String {
    let re_drop = regex::Regex::new(r"(?is)<(script|style|head)[^>]*>.*?</\s*(script|style|head)\s*>").unwrap();
    let re_br = regex::Regex::new(r"(?i)<\s*(br|/p|/div|/tr|/li|/h[1-6])[^>]*>").unwrap();
    let re_tag = regex::Regex::new(r"(?s)<[^>]*>").unwrap();
    let s = re_drop.replace_all(html, " ");
    let s = re_br.replace_all(&s, "\n");
    let s = re_tag.replace_all(&s, "");
    s.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

pub fn text_to_html(text: &str) -> String {
    let esc = text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    esc.replace("\r\n", "\n").replace('\n', "<br>")
}

/// Classify for Smart Inbox: personal | notifications | newsletters | invitations.
pub fn categorize(from: &str, has_list: bool, has_calendar: bool, precedence_bulk: bool) -> &'static str {
    if has_calendar {
        return "invitations";
    }
    if has_list {
        return "newsletters";
    }
    let local = from.split('@').next().unwrap_or("").to_lowercase();
    let automated = [
        "noreply",
        "no-reply",
        "donotreply",
        "do-not-reply",
        "notification",
        "notifications",
        "alerts",
        "mailer-daemon",
        "postmaster",
        "bounce",
        "system",
        "automated",
        "support",
        "billing",
        "info",
        "news",
    ];
    if precedence_bulk || automated.iter().any(|a| local == *a || local.contains(a)) {
        return "notifications";
    }
    "personal"
}

/// Parse a message (full RFC 822 or header-only). Account/folder/uid/flags are filled by the caller.
pub fn parse(raw: &[u8], full: bool) -> Option<NewMessage> {
    let msg = MessageParser::default().parse(raw)?;
    let from = msg.from().and_then(|a| a.first());
    let from_addr = from.and_then(|f| f.address.as_deref()).unwrap_or("").trim().to_string();
    let from_name = from.and_then(|f| f.name.as_deref()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);

    let mut has_calendar = false;
    let mut attachments = vec![];
    let (mut body_text, mut body_html) = (None, None);
    if full {
        body_text = msg.body_text(0).map(|s| s.into_owned());
        body_html = msg.body_html(0).map(|s| s.into_owned());
        // mail-parser synthesizes html from text and vice versa; keep html only when a real html part exists
        let has_real_html = msg.parts.iter().any(|p| p.is_content_type("text", "html"));
        if !has_real_html {
            body_html = None;
        }
        for (i, part) in msg.attachments().enumerate() {
            let ct = part.content_type();
            let mime = ct
                .map(|c| format!("{}/{}", c.c_type, c.c_subtype.as_deref().unwrap_or("octet-stream")))
                .unwrap_or_else(|| "application/octet-stream".into());
            if mime.eq_ignore_ascii_case("text/calendar") {
                has_calendar = true;
            }
            let inline = part.content_disposition().map(|d| d.c_type.eq_ignore_ascii_case("inline")).unwrap_or(false);
            attachments.push(NewAttachment {
                part: i.to_string(),
                filename: part.attachment_name().unwrap_or("attachment").to_string(),
                mime,
                size: part.len() as u64,
                content_id: part.content_id().map(str::to_string),
                inline: inline && part.content_id().is_some(),
            });
        }
    } else {
        // header-only: guess attachments from the top-level content type
        has_calendar = msg
            .content_type()
            .map(|c| c.c_subtype.as_deref().unwrap_or("").eq_ignore_ascii_case("calendar"))
            .unwrap_or(false);
    }
    let has_list = msg.header_raw("List-Unsubscribe").is_some() || msg.header_raw("List-Id").is_some();
    let bulk = msg
        .header_raw("Precedence")
        .map(|p| {
            let p = p.trim().to_lowercase();
            p == "bulk" || p == "list" || p == "junk"
        })
        .unwrap_or(false)
        || msg.header_raw("Auto-Submitted").map(|v| !v.trim().eq_ignore_ascii_case("no")).unwrap_or(false);

    let text_for_preview = match (&body_text, &body_html) {
        (Some(t), _) if !t.trim().is_empty() => t.clone(),
        (_, Some(h)) => html_to_text(h),
        _ => String::new(),
    };
    let has_mixed = !full
        && msg
            .content_type()
            .map(|c| c.c_subtype.as_deref().unwrap_or("").eq_ignore_ascii_case("mixed"))
            .unwrap_or(false);
    let real_attachments = attachments.iter().filter(|a| !a.inline).count() > 0;
    Some(NewMessage {
        message_id: msg.message_id().map(|s| format!("<{}>", s.trim_matches(|c| c == '<' || c == '>'))),
        in_reply_to: ids(msg.in_reply_to()).into_iter().next(),
        refs: ids(msg.references()),
        subject: msg.subject().unwrap_or("").to_string(),
        from_name,
        category: categorize(&from_addr, has_list, has_calendar, bulk).to_string(),
        from_addr,
        to: addresses(msg.to()),
        cc: addresses(msg.cc()),
        bcc: addresses(msg.bcc()),
        reply_to: addresses(msg.reply_to()),
        date_ms: msg.date().map(|d| d.to_timestamp() * 1000).unwrap_or(0),
        has_attachments: real_attachments || has_mixed,
        preview: preview_of(&text_for_preview, 240),
        body_text: if full { Some(body_text.unwrap_or_default()) } else { None },
        body_html,
        attachments,
        ..Default::default()
    })
}

/// Inline (Content-ID) parts of a full message: `(content id without <>, mime, bytes)`.
/// Parts count as inline whenever they carry a Content-ID, whatever their disposition — senders
/// often omit `Content-Disposition: inline` for `cid:` images.
pub fn inline_parts(raw: &[u8]) -> Vec<(String, String, Vec<u8>)> {
    let Some(msg) = MessageParser::default().parse(raw) else { return vec![] };
    msg.parts
        .iter()
        .filter_map(|part| {
            let cid = part.content_id()?.trim().trim_start_matches('<').trim_end_matches('>').to_string();
            if cid.is_empty() || part.is_content_type("multipart", "") {
                return None;
            }
            let mime = part
                .content_type()
                .map(|c| format!("{}/{}", c.c_type, c.c_subtype.as_deref().unwrap_or("octet-stream")))
                .unwrap_or_else(|| "application/octet-stream".into());
            Some((cid, mime, part.contents().to_vec()))
        })
        .collect()
}

/// Replace `cid:<id>` references (src/background/url()) with the given URLs. Matching is
/// case-insensitive on the id, and tolerates `cid:` ids written with surrounding `<>`.
pub fn rewrite_cids(html: &str, urls: &[(String, String)]) -> String {
    let mut out = String::with_capacity(html.len());
    let lower = html.to_ascii_lowercase();
    let mut i = 0;
    while let Some(off) = lower[i..].find("cid:") {
        let at = i + off;
        out.push_str(&html[i..at]);
        let rest = &html[at + 4..];
        let end = rest.find(['"', '\'', ')', ' ', '>', '\t', '\r', '\n']).unwrap_or(rest.len());
        let id = rest[..end].trim_start_matches("&lt;").trim_end_matches("&gt;").trim_matches(['<', '>']);
        let id = percent_encoding::percent_decode_str(id).decode_utf8_lossy();
        match urls.iter().find(|(c, _)| c.eq_ignore_ascii_case(&id)) {
            Some((_, url)) => {
                out.push_str(url);
                i = at + 4 + end;
            }
            None => {
                out.push_str("cid:");
                i = at + 4;
            }
        }
    }
    out.push_str(&html[i..]);
    out
}

/// Extract the raw bytes of attachment number `index` (as enumerated by [`parse`]).
pub fn attachment_bytes(raw: &[u8], index: usize) -> Option<(String, String, Vec<u8>)> {
    let msg = MessageParser::default().parse(raw)?;
    let part = msg.attachments().nth(index)?;
    let name = part.attachment_name().unwrap_or("attachment").to_string();
    let mime = part
        .content_type()
        .map(|c| format!("{}/{}", c.c_type, c.c_subtype.as_deref().unwrap_or("octet-stream")))
        .unwrap_or_else(|| "application/octet-stream".into());
    Some((name, mime, part.contents().to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &[u8] = b"From: =?UTF-8?B?2LnZhNin2YYg2KfZhNi02YfYsdmK?= <alan@example.com>\r\n\
To: User <user@example.com>, other@example.com\r\n\
Subject: =?UTF-8?B?2YXZhNin2K3YuNin2Ko=?=\r\n\
Message-ID: <abc@example.com>\r\n\
In-Reply-To: <parent@example.com>\r\n\
References: <root@example.com> <parent@example.com>\r\n\
Date: Fri, 09 Oct 2026 14:08:00 +0300\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"b1\"\r\n\
\r\n\
--b1\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Hello   world\r\nsecond line\r\n\
--b1\r\n\
Content-Type: application/pdf; name=\"report.pdf\"\r\n\
Content-Disposition: attachment; filename=\"report.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
JVBERi0xLjQK\r\n\
--b1--\r\n";

    #[test]
    fn parses_headers_body_attachments() {
        let m = parse(SAMPLE, true).unwrap();
        assert_eq!(m.subject, "ملاحظات");
        assert_eq!(m.from_addr, "alan@example.com");
        assert_eq!(m.from_name.as_deref(), Some("علان الشهري"));
        assert_eq!(m.message_id.as_deref(), Some("<abc@example.com>"));
        assert_eq!(m.in_reply_to.as_deref(), Some("<parent@example.com>"));
        assert_eq!(m.refs, vec!["<root@example.com>", "<parent@example.com>"]);
        assert_eq!(m.to.as_array().unwrap().len(), 2);
        assert_eq!(m.date_ms, 1_791_544_080_000);
        assert_eq!(m.preview, "Hello world second line");
        assert!(m.has_attachments);
        assert_eq!(m.attachments[0].filename, "report.pdf");
        assert_eq!(m.attachments[0].mime, "application/pdf");
        assert!(m.body_html.is_none());
        let (name, _, bytes) = attachment_bytes(SAMPLE, 0).unwrap();
        assert_eq!(name, "report.pdf");
        assert!(bytes.starts_with(b"%PDF"));
    }

    #[test]
    fn header_only_parse() {
        let head = SAMPLE.split(|_| false).next().unwrap();
        let end = head.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        let m = parse(&head[..end], false).unwrap();
        assert_eq!(m.subject, "ملاحظات");
        assert!(m.body_text.is_none());
        assert!(m.has_attachments, "multipart/mixed header implies attachments");
    }

    #[test]
    fn categories() {
        assert_eq!(categorize("noreply@x.com", false, false, false), "notifications");
        assert_eq!(categorize("bob@x.com", true, false, false), "newsletters");
        assert_eq!(categorize("bob@x.com", false, true, false), "invitations");
        assert_eq!(categorize("bob@x.com", false, false, false), "personal");
    }

    #[test]
    fn html_text() {
        assert_eq!(html_to_text("<style>x</style><p>a&amp;b</p><br>c").trim(), "a&b\n\nc");
        assert_eq!(text_to_html("a<b>\nc"), "a&lt;b&gt;<br>c");
    }

    const INLINE: &[u8] = b"From: a@example.com\r\n\
To: b@example.com\r\n\
Subject: logo\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/related; boundary=\"r1\"\r\n\
\r\n\
--r1\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<img src=\"cid:img1.png@example.com\" alt=\"logo\"><div style=\"background:url(CID:Img2.PNG@example.com)\"></div><img src=\"cid:missing@x\">\r\n\
--r1\r\n\
Content-Type: image/png\r\n\
Content-ID: <img1.png@example.com>\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
iVBORw0KGgo=\r\n\
--r1\r\n\
Content-Type: image/png\r\n\
Content-ID: <img2.png@example.com>\r\n\
Content-Disposition: attachment; filename=\"b.png\"\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
iVBORw0KGgo=\r\n\
--r1--\r\n";

    #[test]
    fn inline_images_are_extracted_and_rewritten() {
        let parts = inline_parts(INLINE);
        let ids: Vec<&str> = parts.iter().map(|p| p.0.as_str()).collect();
        assert_eq!(ids, ["img1.png@example.com", "img2.png@example.com"]);
        assert_eq!(parts[0].1, "image/png");
        assert_eq!(&parts[0].2[..4], b"\x89PNG");

        let html = parse(INLINE, true).unwrap().body_html.unwrap();
        let urls: Vec<(String, String)> =
            parts.iter().enumerate().map(|(n, p)| (p.0.clone(), format!("file:///c/{n}.png"))).collect();
        let out = rewrite_cids(&html, &urls);
        assert!(out.contains("src=\"file:///c/0.png\""), "{out}");
        assert!(out.contains("url(file:///c/1.png)"), "case-insensitive: {out}");
        assert!(out.contains("src=\"cid:missing@x\""), "unknown ids untouched: {out}");
    }
}
