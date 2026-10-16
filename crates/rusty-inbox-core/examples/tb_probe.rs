//! Show what an import from Thunderbird would bring over (never prints passwords).
//! `cargo run -p rusty-inbox-core --example tb_probe`

fn main() {
    for p in rusty_inbox_core::thunderbird::profiles() {
        match rusty_inbox_core::thunderbird::saved_logins(&p) {
            Ok(l) => println!("profile {} ({} saved logins readable)", p.display(), l.len()),
            Err(e) => println!("profile {} (passwords: {e:#})", p.display()),
        }
    }
    for a in rusty_inbox_core::thunderbird::find_accounts() {
        println!(
            "{} imap={}:{} {} smtp={}:{} {} oauth={} password={}",
            a.email,
            a.imap_host,
            a.imap_port,
            a.imap_security,
            a.smtp_host,
            a.smtp_port,
            a.smtp_security,
            a.oauth,
            a.has_password
        );
    }
}
