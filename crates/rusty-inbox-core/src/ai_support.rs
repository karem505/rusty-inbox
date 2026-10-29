//! Read-only helpers for the AI features (kept apart from the engine): links in an HTML body, the
//! security headers of a raw message and a body text without the quoted history.

use mail_parser::MessageParser;
use regex::Regex;
use std::sync::OnceLock;

/// Headers the phishing check looks at, from a raw RFC 822 message.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawHeaders {
    pub from: String,
    pub reply_to: String,
    pub return_path: String,
    /// all `Authentication-Results` headers, joined with newlines
    pub authentication_results: String,
}

/// Parse the headers out of a raw message (`None` when it cannot be parsed at all).
pub fn raw_headers(raw: &[u8]) -> Option<RawHeaders> {
    let msg = MessageParser::default().parse(raw)?;
    let one = |name: &str| {
        msg.header_raw(name).map(|v| v.split_whitespace().collect::<Vec<_>>().join(" ")).unwrap_or_default()
    };
    let auth: Vec<String> = msg
        .headers()
        .iter()
        .filter(|h| h.name().eq_ignore_ascii_case("authentication-results"))
        .filter_map(|h| msg.raw_message().get(h.offset_start() as usize..h.offset_end() as usize))
        .map(|b| String::from_utf8_lossy(b).split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    Some(RawHeaders {
        from: one("From"),
        reply_to: one("Reply-To"),
        return_path: one("Return-Path"),
        authentication_results: if auth.is_empty() { one("Authentication-Results") } else { auth.join("\n") },
    })
}

fn decode(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Every http(s) link of an HTML body as `(shown text, real URL)`, without duplicates, at most 60.
pub fn html_links(html: &str) -> Vec<(String, String)> {
    static RE: OnceLock<Regex> = OnceLock::new();
    static TAG: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"(?is)<a\s[^>]*?href\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))[^>]*>(.*?)</a>"#).unwrap()
    });
    let tag = TAG.get_or_init(|| Regex::new(r"(?s)<[^>]*>").unwrap());
    let mut out: Vec<(String, String)> = vec![];
    for c in re.captures_iter(html) {
        let href = decode(c.get(1).or(c.get(2)).or(c.get(3)).map(|m| m.as_str()).unwrap_or("").trim());
        if !(href.starts_with("http://") || href.starts_with("https://")) {
            continue;
        }
        let text = decode(&tag.replace_all(c.get(4).map(|m| m.as_str()).unwrap_or(""), " "));
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let item = (text, href);
        if !out.contains(&item) {
            out.push(item);
        }
        if out.len() == 60 {
            break;
        }
    }
    out
}

/// A message body for a prompt: the new text only. Quoted lines (`>`), the "On … wrote:" line with
/// everything after it, and an "-----Original Message-----" tail are dropped.
pub fn strip_quoted(text: &str) -> String {
    static WROTE: OnceLock<Regex> = OnceLock::new();
    let wrote = WROTE.get_or_init(|| {
        Regex::new(r"(?i)^\s*(on .{5,200} wrote:|le .{5,200} a écrit\s*:|am .{5,200} schrieb .*:|.{5,200} كتب:)\s*$")
            .unwrap()
    });
    let mut kept: Vec<&str> = vec![];
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("-----Original Message-----")
            || t.starts_with("________________________________")
            || wrote.is_match(line)
        {
            break;
        }
        if t.starts_with('>') {
            continue;
        }
        kept.push(line);
    }
    let out = kept.join("\n").trim().to_string();
    // a message that is only a quote keeps its text rather than becoming empty
    if out.is_empty() {
        text.trim().to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_with_text_and_target() {
        let html = r##"<p><a href="https://www.mybank.com/login" class=x>mybank.com</a> and
            <a href='http://203.0.113.9/a?x=1&amp;y=2'><b>Click</b> here</a> <a href=https://ok.example/>plain</a>
            <a href="mailto:a@b.c">mail</a> <a href="#top">top</a> <a href="https://www.mybank.com/login">mybank.com</a></p>"##;
        assert_eq!(
            html_links(html),
            vec![
                ("mybank.com".to_string(), "https://www.mybank.com/login".to_string()),
                ("Click here".to_string(), "http://203.0.113.9/a?x=1&y=2".to_string()),
                ("plain".to_string(), "https://ok.example/".to_string()),
            ]
        );
        assert!(html_links("no links").is_empty());
    }

    #[test]
    fn headers_of_a_raw_message() {
        let raw = b"Return-Path: <bounce@mailer.example>\r\nAuthentication-Results: mx.me.org;\r\n spf=pass smtp.mailfrom=a.example;\r\n dkim=fail header.d=a.example\r\nFrom: A <a@a.example>\r\nReply-To: r@other.example\r\nSubject: hi\r\n\r\nbody\r\n";
        let h = raw_headers(raw).unwrap();
        assert!(h.return_path.contains("bounce@mailer.example"));
        assert!(h.reply_to.contains("r@other.example"));
        assert!(h.from.contains("a@a.example"));
        assert!(
            h.authentication_results.contains("spf=pass") && h.authentication_results.contains("dkim=fail"),
            "{h:?}"
        );
    }

    #[test]
    fn quoted_history_is_dropped() {
        let t = "Sounds good, Friday works.\n\nOn Mon, 5 Oct 2026 at 10:00, Dana <d@x.org> wrote:\n> Can we meet Friday?\n> Please say.\n";
        assert_eq!(strip_quoted(t), "Sounds good, Friday works.");
        assert_eq!(strip_quoted("Hi\n> quoted\nthere\n-----Original Message-----\nold"), "Hi\nthere");
        assert_eq!(strip_quoted("> only quote"), "> only quote");
        assert_eq!(strip_quoted("plain"), "plain");
    }
}
