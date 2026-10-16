//! The search query language: `"text" from:"Name <a@b>" to:… subject:… has:attachments in:sent
//! is:unread is:pinned since:2026-01-01`.

/// A parsed query: free-text terms plus structured filters.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Query {
    pub text: Vec<String>,
    pub from: Vec<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: Vec<String>,
    pub folder: Option<String>,
    pub has_attachments: bool,
    pub unread: Option<bool>,
    pub pinned: bool,
    pub since_ms: Option<i64>,
}

impl Query {
    pub fn is_empty(&self) -> bool {
        *self == Query::default()
    }

    /// Words worth highlighting in results.
    pub fn highlight_terms(&self) -> Vec<String> {
        self.text.iter().chain(&self.subject).flat_map(|t| t.split_whitespace().map(str::to_string)).collect()
    }
}

/// Extract the address out of `Name <addr>` / `addr` / `Name`.
fn address_of(v: &str) -> String {
    match (v.rfind('<'), v.rfind('>')) {
        (Some(a), Some(b)) if b > a => v[a + 1..b].trim().to_lowercase(),
        _ => v.trim().to_lowercase(),
    }
}

/// Parse the query string (`"text" from:"Name <a@b>" has:attachments in:sent is:unread …`). Bare words
/// typed by hand (`from: alex`) work too.
pub fn parse(q: &str, now_ms: i64) -> Query {
    let mut out = Query::default();
    let chars: Vec<char> = q.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        // optional `key:` prefix
        let start = i;
        let mut key = String::new();
        while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
            i += 1;
        }
        if i < chars.len() && chars[i] == ':' && i > start {
            key = chars[start..i].iter().collect::<String>().to_lowercase();
            i += 1;
            while i < chars.len() && chars[i] == ' ' {
                i += 1;
            }
        } else {
            i = start;
        }
        // value: quoted or up to whitespace
        let value: String = if i < chars.len() && chars[i] == '"' {
            i += 1;
            let s = i;
            while i < chars.len() && chars[i] != '"' {
                i += 1;
            }
            let v: String = chars[s..i].iter().collect();
            i += 1;
            v
        } else {
            let s = i;
            while i < chars.len() && !chars[i].is_whitespace() {
                i += 1;
            }
            chars[s..i].iter().collect()
        };
        let value = value.trim().to_string();
        match key.as_str() {
            "from" if !value.is_empty() => out.from.push(address_of(&value)),
            "to" if !value.is_empty() => out.to.push(address_of(&value)),
            "cc" if !value.is_empty() => out.cc.push(address_of(&value)),
            "subject" if !value.is_empty() => out.subject.push(value),
            "in" => out.folder = Some(value.to_lowercase()),
            "has" | "with" => out.has_attachments = true,
            "is" => match value.to_lowercase().as_str() {
                "unread" => out.unread = Some(true),
                "read" => out.unread = Some(false),
                "pinned" => out.pinned = true,
                _ => {}
            },
            "date" | "since" => out.since_ms = relative_since(&value, now_ms),
            _ => {
                let v = if key.is_empty() { value } else { format!("{key}:{value}") };
                if !v.is_empty() {
                    out.text.push(v);
                }
            }
        }
    }
    out
}

fn relative_since(v: &str, now_ms: i64) -> Option<i64> {
    const DAY: i64 = 86_400_000;
    let days = match v {
        "today" => 0,
        "yesterday" | "lastTwoDays" => 1,
        "thisWeek" => 6,
        "lastWeek" => 13,
        "lastTwoWeeks" => 14,
        "thisMonth" => 30,
        "lastMonth" | "lastTwoMonth" => 61,
        "thisYear" => 365,
        "lastYear" => 730,
        _ => {
            return chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d")
                .ok()
                .and_then(|d| d.and_hms_opt(0, 0, 0))
                .map(|d| d.and_utc().timestamp_millis())
        }
    };
    let start_of_today = now_ms - now_ms.rem_euclid(DAY);
    Some(start_of_today - days * DAY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tokens_and_text() {
        let q = parse("from:\"Jane Doe <Jane@Example.org>\" \"grant\" has:attachments in:sent is:unread", 0);
        assert_eq!(q.from, vec!["jane@example.org"]);
        assert_eq!(q.text, vec!["grant"]);
        assert!(q.has_attachments);
        assert_eq!(q.folder.as_deref(), Some("sent"));
        assert_eq!(q.unread, Some(true));
        let q = parse("from: alex quarterly report", 0);
        assert_eq!(q.from, vec!["alex"]);
        assert_eq!(q.text, vec!["quarterly", "report"]);
        assert!(parse("\"\"", 0).is_empty());
    }
}
