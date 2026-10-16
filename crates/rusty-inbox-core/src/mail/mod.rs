//! Mail protocol layer: server autodetection, MIME parsing, IMAP sync and SMTP send.

pub mod imap;
pub mod mime;
pub mod smtp;
pub mod tls;

use crate::store::Account;

/// Strip reply/forward prefixes (`Re:`, `Fwd:`, `FW:`, Arabic `رد:`/`إعادة توجيه:`) for thread subjects.
pub fn normalize_subject(s: &str) -> String {
    let mut cur = s.trim();
    loop {
        let lower = cur.to_lowercase();
        let mut stripped = false;
        for p in ["re:", "fwd:", "fw:", "aw:", "wg:", "tr:", "رد:", "إعادة توجيه:", "توجيه:"] {
            if lower.starts_with(p) {
                cur = cur[p.len()..].trim_start();
                stripped = true;
                break;
            }
        }
        // `Re[2]:` style
        if !stripped && lower.starts_with("re[") {
            if let Some(i) = cur.find("]:") {
                cur = cur[i + 2..].trim_start();
                stripped = true;
            }
        }
        if !stripped {
            break;
        }
    }
    cur.to_string()
}

/// Known providers; everything else gets `mail.<domain>` (the common cPanel convention), then `imap.<domain>` / `smtp.<domain>`.
pub fn server_candidates(email: &str) -> Vec<(String, u16, String, String, u16, String)> {
    let domain = email.rsplit('@').next().unwrap_or("").to_lowercase();
    match domain.as_str() {
        "gmail.com" | "googlemail.com" => {
            vec![("imap.gmail.com".into(), 993, "tls".into(), "smtp.gmail.com".into(), 465, "tls".into())]
        }
        "outlook.com" | "hotmail.com" | "live.com" | "msn.com" => vec![(
            "outlook.office365.com".into(),
            993,
            "tls".into(),
            "smtp.office365.com".into(),
            587,
            "starttls".into(),
        )],
        "yahoo.com" => {
            vec![("imap.mail.yahoo.com".into(), 993, "tls".into(), "smtp.mail.yahoo.com".into(), 465, "tls".into())]
        }
        "icloud.com" | "me.com" | "mac.com" => {
            vec![("imap.mail.me.com".into(), 993, "tls".into(), "smtp.mail.me.com".into(), 587, "starttls".into())]
        }
        _ => vec![
            (format!("mail.{domain}"), 993, "tls".into(), format!("mail.{domain}"), 465, "tls".into()),
            (format!("imap.{domain}"), 993, "tls".into(), format!("smtp.{domain}"), 465, "tls".into()),
            (format!("mail.{domain}"), 993, "tls".into(), format!("mail.{domain}"), 587, "starttls".into()),
        ],
    }
}

/// The value `detectAccountType` returns (observed: `customIMAP` for a cPanel domain).
pub fn detect_account_type(email: &str) -> &'static str {
    let domain = email.rsplit('@').next().unwrap_or("").to_lowercase();
    match domain.as_str() {
        "gmail.com" | "googlemail.com" => "gmail",
        "outlook.com" | "hotmail.com" | "live.com" | "msn.com" => "outlook",
        "yahoo.com" => "yahoo",
        "icloud.com" | "me.com" | "mac.com" => "icloud",
        "aol.com" => "aol",
        _ => "customIMAP",
    }
}

/// Try candidate servers until IMAP login succeeds. Returns the working account definition.
pub async fn autodetect(email: &str, name: &str, password: &str) -> anyhow::Result<Account> {
    let mut last_err = anyhow::anyhow!("no server candidates for {email}");
    let mut tried = std::collections::HashSet::new();
    for (ih, ip, is, sh, sp, ss) in server_candidates(email) {
        if !tried.insert((ih.clone(), ip)) {
            // IMAP side already verified/failed with this host; only the SMTP side differs.
            continue;
        }
        let acct = Account {
            email: email.to_string(),
            name: name.to_string(),
            account_type: detect_account_type(email).to_string(),
            imap_host: ih,
            imap_port: ip,
            imap_security: is,
            smtp_host: sh,
            smtp_port: sp,
            smtp_security: ss,
            username: email.to_string(),
        };
        match imap::Connection::connect(&acct, password).await {
            Ok(mut c) => {
                let _ = c.logout().await;
                return Ok(acct);
            }
            // keep the most telling error: a server that answered (e.g. rejected the login) beats
            // a candidate host that does not exist
            Err(e) => {
                if !is_lookup_failure(&e) || is_lookup_failure(&last_err) {
                    last_err = e;
                }
            }
        }
    }
    Err(last_err)
}

fn is_lookup_failure(e: &anyhow::Error) -> bool {
    let m = format!("{e:#}");
    m.contains("failed to lookup address")
        || m.contains("no server candidates")
        || m.contains("Name or service not known")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subjects_normalize() {
        assert_eq!(normalize_subject("Re: Fwd: Hello"), "Hello");
        assert_eq!(normalize_subject("RE: re: FW: x"), "x");
        assert_eq!(normalize_subject("Re[3]: y"), "y");
        assert_eq!(normalize_subject("Fwd: ملاحظات "), "ملاحظات");
        assert_eq!(normalize_subject("Report Q3"), "Report Q3");
    }

    #[test]
    fn autodetect_reports_the_answering_server() {
        assert!(is_lookup_failure(&anyhow::anyhow!("connect to imap.x:993: failed to lookup address information")));
        assert!(!is_lookup_failure(&anyhow::anyhow!("login failed: AUTHENTICATIONFAILED")));
    }

    #[test]
    fn detection() {
        assert_eq!(detect_account_type("a@example.org"), "customIMAP");
        assert_eq!(detect_account_type("a@gmail.com"), "gmail");
        assert_eq!(server_candidates("a@example.org")[0].0, "mail.example.org");
    }
}
