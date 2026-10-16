//! Fill a data directory with a made-up mailbox for demos, screenshots and screen recordings.
//! Every person, address and message here is fictional.
//!
//!     cargo run --release -p rusty-inbox-core --example seed_demo -- <core-data dir>
//!     RUSTY_INBOX_DATA_DIR=<core-data dir> RUSTY_INBOX_OFFLINE=1 rusty-inbox

use rusty_inbox_core::store::{Account, ListFilter, NewAttachment, NewMessage, Store};
use serde_json::json;
use std::path::PathBuf;

const WORK: &str = "alex@northwind.dev";
const HOME: &str = "alex.rivera@fastmail.example";
const ME: &str = "Alex Rivera";

struct Mail<'a> {
    account: &'a str,
    folder: &'a str,
    from: (&'a str, &'a str),
    to: Vec<(&'a str, &'a str)>,
    subject: &'a str,
    hours_ago: f64,
    seen: bool,
    category: &'a str,
    html: String,
    attachments: Vec<(&'a str, &'a str, u64)>,
    id: &'a str,
    reply_to_id: Option<&'a str>,
}

fn person(paragraphs: &[&str], sign: &str) -> String {
    let body: String = paragraphs.iter().map(|p| format!("<p>{p}</p>")).collect();
    format!(
        r#"<div style="font-family:-apple-system,Segoe UI,Helvetica,Arial,sans-serif;font-size:15px;line-height:1.6;color:#1f2328">{body}<p style="color:#57606a">{sign}</p></div>"#
    )
}

fn newsletter(brand: &str, color: &str, issue: &str, items: &[(&str, &str)]) -> String {
    let cards: String = items
        .iter()
        .map(|(t, d)| {
            format!(
                r#"<tr><td style="padding:14px 28px"><div style="font-size:18px;font-weight:700;color:#111">{t}</div><div style="font-size:14px;color:#555;margin-top:4px;line-height:1.55">{d}</div></td></tr>"#
            )
        })
        .collect();
    format!(
        r#"<table width="100%" cellspacing="0" cellpadding="0" style="font-family:Helvetica,Arial,sans-serif;background:#f4f4f6"><tr><td align="center" style="padding:24px 0">
<table width="620" cellspacing="0" cellpadding="0" style="background:#ffffff;border-radius:14px;overflow:hidden">
<tr><td style="background:{color};padding:34px 28px;color:#fff"><div style="font-size:13px;letter-spacing:2px;text-transform:uppercase;opacity:.85">{issue}</div><div style="font-size:30px;font-weight:800;margin-top:6px">{brand}</div></td></tr>
{cards}
<tr><td style="padding:18px 28px 30px"><a style="display:inline-block;background:{color};color:#fff;text-decoration:none;padding:12px 22px;border-radius:8px;font-weight:700">Read the full issue</a></td></tr>
</table><div style="font-size:12px;color:#888;padding:14px">You are receiving this because you subscribed. Unsubscribe</div></td></tr></table>"#
    )
}

fn notice(title: &str, color: &str, lines: &[&str], button: &str) -> String {
    let body: String = lines.iter().map(|l| format!(r#"<p style="margin:8px 0;color:#444">{l}</p>"#)).collect();
    format!(
        r#"<div style="font-family:Helvetica,Arial,sans-serif;max-width:560px;margin:10px auto;border:1px solid #e5e7eb;border-radius:12px;overflow:hidden">
<div style="height:6px;background:{color}"></div><div style="padding:22px 26px"><div style="font-size:20px;font-weight:700;color:#111">{title}</div>{body}
<a style="display:inline-block;margin-top:12px;background:{color};color:#fff;padding:10px 18px;border-radius:7px;text-decoration:none;font-weight:600">{button}</a></div></div>"#
    )
}

fn main() {
    let dir = PathBuf::from(std::env::args().nth(1).expect("usage: seed_demo <core-data dir>"));
    let store = Store::open(&dir.join("databases")).expect("open store");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;

    for (email, name, imap, smtp) in [
        (WORK, "Northwind", "mail.northwind.dev", "mail.northwind.dev"),
        (HOME, "Personal", "imap.fastmail.example", "smtp.fastmail.example"),
    ] {
        store
            .upsert_account(&Account {
                email: email.into(),
                name: name.into(),
                account_type: "customIMAP".into(),
                imap_host: imap.into(),
                imap_port: 993,
                imap_security: "tls".into(),
                smtp_host: smtp.into(),
                smtp_port: 465,
                smtp_security: "tls".into(),
                username: email.into(),
            })
            .unwrap();
    }
    let mut folder = std::collections::HashMap::new();
    for acc in [WORK, HOME] {
        for (path, role) in [
            ("INBOX", Some("inbox")),
            ("Sent", Some("sent")),
            ("Drafts", Some("drafts")),
            ("Archive", Some("archive")),
            ("Spam", Some("spam")),
            ("Trash", Some("trash")),
        ] {
            let id = store.upsert_folder(acc, path, path, Some("/"), role, true).unwrap();
            folder.insert((acc, path), id);
        }
    }
    for (acc, path) in
        [(WORK, "Projects/Launch"), (WORK, "Clients"), (WORK, "Receipts"), (HOME, "Travel"), (HOME, "Family")]
    {
        let name = path.rsplit('/').next().unwrap();
        let id = store.upsert_folder(acc, path, name, Some("/"), None, true).unwrap();
        folder.insert((acc, path), id);
    }

    let me_work = (ME, WORK);
    let me_home = (ME, HOME);
    let mut mails: Vec<Mail> = vec![
        // ---------------- work inbox: people
        Mail { account: WORK, folder: "INBOX", from: ("Maya Chen", "maya@northwind.dev"), to: vec![me_work], subject: "Launch plan for v1.0: final review", hours_ago: 30.0, seen: true, category: "personal",
            html: person(&["Hi Alex,", "I put the launch plan in the shared doc. The big items are the release notes, the AUR and Flatpak packages, and the demo video.", "Can you look at the timeline before Friday?"], "Maya"),
            attachments: vec![("launch-plan.pdf", "application/pdf", 482_000)], id: "launch-1", reply_to_id: None },
        Mail { account: WORK, folder: "Sent", from: me_work, to: vec![("Maya Chen", "maya@northwind.dev")], subject: "Re: Launch plan for v1.0: final review", hours_ago: 26.0, seen: true, category: "personal",
            html: person(&["Looks good to me. I moved the packaging work a day earlier so we have time to test on a clean machine.", "One question: do we announce on Monday or Tuesday?"], "Alex"),
            attachments: vec![], id: "launch-2", reply_to_id: Some("launch-1") },
        Mail { account: WORK, folder: "INBOX", from: ("Maya Chen", "maya@northwind.dev"), to: vec![me_work], subject: "Re: Launch plan for v1.0: final review", hours_ago: 0.4, seen: false, category: "personal",
            html: person(&["Tuesday. Mondays are noisy.", "I also added a section for the benchmark numbers, they are the strongest part of the story: the inbox opens in a fraction of a second and the whole app is a small native binary.", "Thanks for the quick turnaround!"], "Maya"),
            attachments: vec![], id: "launch-3", reply_to_id: Some("launch-2") },
        Mail { account: WORK, folder: "INBOX", from: ("Jonas Weber", "jonas@northwind.dev"), to: vec![me_work], subject: "Benchmarks look great", hours_ago: 1.2, seen: false, category: "personal",
            html: person(&["Hey Alex,", "I ran the benchmark suite on my old laptop. Cold start is under half a second and memory stays low even with three accounts syncing.", "Charts are attached. Want me to add them to the README?"], "Jonas"),
            attachments: vec![("startup-chart.png", "image/png", 128_000), ("memory-chart.png", "image/png", 141_000)], id: "bench", reply_to_id: None },
        Mail { account: WORK, folder: "INBOX", from: ("Priya Nair", "priya@northwind.dev"), to: vec![me_work], subject: "Design review: new sidebar", hours_ago: 2.5, seen: false, category: "personal",
            html: person(&["Hi Alex,", "Here is the new sidebar. Each list unfolds into your accounts, and the colours now come from the system theme, so it matches the desktop.", "Let me know what you think before Thursday's critique."], "Priya"),
            attachments: vec![("sidebar-v3.png", "image/png", 356_000)], id: "design", reply_to_id: None },
        Mail { account: WORK, folder: "INBOX", from: ("Lea Martin", "lea@brightpath.example"), to: vec![me_work], subject: "Contract draft for Q4", hours_ago: 5.0, seen: true, category: "personal",
            html: person(&["Dear Alex,", "Please find the Q4 contract draft attached. The scope is unchanged; I updated the dates and the support hours.", "Happy to set up a call if anything needs discussion."], "Lea Martin<br>Brightpath Consulting"),
            attachments: vec![("Q4-contract-draft.pdf", "application/pdf", 214_000)], id: "contract", reply_to_id: None },
        Mail { account: WORK, folder: "INBOX", from: ("Diego Alvarez", "diego@northwind.dev"), to: vec![me_work], subject: "Packaging for Arch and Flatpak", hours_ago: 7.0, seen: true, category: "personal",
            html: person(&["The AUR package builds cleanly now. Flatpak needs one more permission for notifications.", "I'll open a pull request tonight."], "Diego"),
            attachments: vec![], id: "pkg", reply_to_id: None },
        Mail { account: WORK, folder: "INBOX", from: ("Hannah Kim", "hannah@orbitlabs.example"), to: vec![me_work], subject: "Quick question about IMAP IDLE", hours_ago: 11.0, seen: true, category: "personal",
            html: person(&["Hi Alex, quick one: does the client keep one IDLE connection per account, or poll?", "We are seeing very different battery numbers between clients."], "Hannah"),
            attachments: vec![], id: "idle", reply_to_id: None },
        Mail { account: WORK, folder: "INBOX", from: ("Sam Okafor", "sam@northwind.dev"), to: vec![me_work], subject: "Lunch on Thursday?", hours_ago: 20.0, seen: true, category: "personal",
            html: person(&["The new ramen place opened next to the office. Thursday at 12:30?"], "Sam"),
            attachments: vec![], id: "lunch", reply_to_id: None },
        // ---------------- work inbox: invitations
        Mail { account: WORK, folder: "INBOX", from: ("Maya Chen", "maya@northwind.dev"), to: vec![me_work], subject: "Invitation: Release sync, Friday 10:00", hours_ago: 3.0, seen: false, category: "invitations",
            html: notice("Release sync", "#2f6bed", &["Friday, 10:00 to 10:30", "Video call, link in the calendar event", "Agenda: launch checklist, packaging, announcement"], "Accept"),
            attachments: vec![], id: "inv1", reply_to_id: None },
        Mail { account: WORK, folder: "INBOX", from: ("Priya Nair", "priya@northwind.dev"), to: vec![me_work], subject: "Invitation: Design critique, Thursday 15:00", hours_ago: 9.0, seen: true, category: "invitations",
            html: notice("Design critique", "#8b5cf6", &["Thursday, 15:00 to 16:00", "Studio room 2"], "Accept"),
            attachments: vec![], id: "inv2", reply_to_id: None },
        // ---------------- work inbox: notifications
        Mail { account: WORK, folder: "INBOX", from: ("CI", "ci@builds.northwind.dev"), to: vec![me_work], subject: "Build #482 passed on main", hours_ago: 0.8, seen: false, category: "notifications",
            html: notice("Build #482 passed", "#16a34a", &["Branch: main", "Duration: 3 min 41 s", "All 214 tests passed"], "View build"),
            attachments: vec![], id: "ci482", reply_to_id: None },
        Mail { account: WORK, folder: "INBOX", from: ("CI", "ci@builds.northwind.dev"), to: vec![me_work], subject: "Build #481 failed on feature/tray-icon", hours_ago: 4.0, seen: true, category: "notifications",
            html: notice("Build #481 failed", "#dc2626", &["Branch: feature/tray-icon", "1 test failed: tray::icon_follows_theme"], "View logs"),
            attachments: vec![], id: "ci481", reply_to_id: None },
        Mail { account: WORK, folder: "INBOX", from: ("Code Review", "noreply@review.northwind.dev"), to: vec![me_work], subject: "3 new comments on pull request #219", hours_ago: 6.0, seen: false, category: "notifications",
            html: notice("Pull request #219", "#0ea5e9", &["Priya: Love the new check boxes.", "Jonas: Can we keep the compact density as an option?", "Diego: Approved."], "Open pull request"),
            attachments: vec![], id: "pr219", reply_to_id: None },
        Mail { account: WORK, folder: "INBOX", from: ("Cloudhost Billing", "billing@cloudhost.example"), to: vec![me_work], subject: "Your invoice for October", hours_ago: 15.0, seen: true, category: "notifications",
            html: notice("Invoice CH-2026-1043", "#111827", &["Amount due: $24.00", "Due date: 1 November"], "View invoice"),
            attachments: vec![("invoice-CH-2026-1043.pdf", "application/pdf", 61_000)], id: "invoice", reply_to_id: None },
        Mail { account: WORK, folder: "INBOX", from: ("Security", "security@northwind.dev"), to: vec![me_work], subject: "New sign-in on Linux", hours_ago: 22.0, seen: true, category: "notifications",
            html: notice("New sign-in", "#f59e0b", &["A new sign-in to your account from Linux.", "If this was you, there is nothing to do."], "Review activity"),
            attachments: vec![], id: "signin", reply_to_id: None },
        // ---------------- work inbox: newsletters
        Mail { account: WORK, folder: "INBOX", from: ("Ferrous Weekly", "hello@ferrousweekly.example"), to: vec![me_work], subject: "Ferrous Weekly, issue 214: async without the tears", hours_ago: 8.0, seen: false, category: "newsletters",
            html: newsletter("Ferrous Weekly", "#b7410e", "Issue 214", &[("Async without the tears", "A practical guide to structuring tokio services so they stay readable."), ("Faster builds", "Five settings that cut incremental build times in half."), ("Crate of the week", "A tiny, well-tested IMAP parser.")]),
            attachments: vec![], id: "nl1", reply_to_id: None },
        Mail { account: WORK, folder: "INBOX", from: ("Qt Notes", "notes@qtnotes.example"), to: vec![me_work], subject: "QML performance: five tips for smooth lists", hours_ago: 26.0, seen: true, category: "newsletters",
            html: newsletter("Qt Notes", "#41cd52", "Monthly", &[("Smooth lists", "Reuse delegates and avoid rebuilding models on every change."), ("Animations that feel fast", "Short durations and the right easing matter more than frame counts.")]),
            attachments: vec![], id: "nl2", reply_to_id: None },
        Mail { account: WORK, folder: "INBOX", from: ("Linux Desktop Digest", "digest@desktopdigest.example"), to: vec![me_work], subject: "This week on the Linux desktop", hours_ago: 40.0, seen: true, category: "newsletters",
            html: newsletter("Desktop Digest", "#6d28d9", "Weekly", &[("Tiling is mainstream", "Why more people switch to tiling window managers."), ("Themes everywhere", "Apps that follow the system theme feel at home.")]),
            attachments: vec![], id: "nl3", reply_to_id: None },
        // ---------------- personal inbox
        Mail { account: HOME, folder: "INBOX", from: ("Nadia Rivera", "nadia.rivera@fastmail.example"), to: vec![me_home], subject: "Family dinner on Sunday", hours_ago: 1.8, seen: false, category: "personal",
            html: person(&["Hi dear,", "Dinner at our place on Sunday at 7. Your aunt is coming too.", "Bring the photos from the trip!"], "Mum"),
            attachments: vec![], id: "dinner", reply_to_id: None },
        Mail { account: HOME, folder: "INBOX", from: ("Omar Haddad", "omar.h@fastmail.example"), to: vec![me_home], subject: "Photos from the hike", hours_ago: 13.0, seen: true, category: "personal",
            html: person(&["What a view from the top. I uploaded the best ones, the sunrise shot is my favourite."], "Omar"),
            attachments: vec![("sunrise.jpg", "image/jpeg", 2_400_000), ("summit.jpg", "image/jpeg", 2_100_000)], id: "hike", reply_to_id: None },
        Mail { account: HOME, folder: "INBOX", from: ("SkyWays", "trips@skyways.example"), to: vec![me_home], subject: "Your flight to Lisbon is confirmed", hours_ago: 18.0, seen: true, category: "notifications",
            html: notice("Booking confirmed", "#0369a1", &["Lisbon, 14 November, 09:20", "Seat 12A, booking reference K7Q2PX"], "Manage booking"),
            attachments: vec![], id: "flight", reply_to_id: None },
        Mail { account: HOME, folder: "INBOX", from: ("Weekend Reads", "hello@weekendreads.example"), to: vec![me_home], subject: "Weekend Reads: slow mornings", hours_ago: 34.0, seen: true, category: "newsletters",
            html: newsletter("Weekend Reads", "#be185d", "Saturday edition", &[("Slow mornings", "On the small rituals that make a weekend feel long."), ("Three recipes", "Easy, warm and ready in twenty minutes.")]),
            attachments: vec![], id: "nl4", reply_to_id: None },
        Mail { account: HOME, folder: "INBOX", from: ("City Library", "notices@library.example"), to: vec![me_home], subject: "Reminder: renew your library card", hours_ago: 50.0, seen: true, category: "personal",
            html: person(&["Your library card expires at the end of the month. Renew it online or at any branch."], "City Library"),
            attachments: vec![], id: "snoozeme", reply_to_id: None },
        // ---------------- sent, drafts, archive, folders, spam, trash
        Mail { account: WORK, folder: "Sent", from: me_work, to: vec![("Jonas Weber", "jonas@northwind.dev")], subject: "Re: Release checklist", hours_ago: 3.5, seen: true, category: "personal",
            html: person(&["Checklist updated. Packaging is on me, release notes on you."], "Alex"), attachments: vec![], id: "s1", reply_to_id: None },
        Mail { account: WORK, folder: "Sent", from: me_work, to: vec![("Lea Martin", "lea@brightpath.example")], subject: "Re: Statement of work", hours_ago: 28.0, seen: true, category: "personal",
            html: person(&["Thanks Lea, signed copy attached."], "Alex"), attachments: vec![("SOW-signed.pdf", "application/pdf", 180_000)], id: "s2", reply_to_id: None },
        Mail { account: WORK, folder: "Sent", from: me_work, to: vec![("Hannah Kim", "hannah@orbitlabs.example")], subject: "Re: Conference talk", hours_ago: 52.0, seen: true, category: "personal",
            html: person(&["I'd be glad to give the talk. Thirty minutes works."], "Alex"), attachments: vec![], id: "s3", reply_to_id: None },
        Mail { account: HOME, folder: "Sent", from: me_home, to: vec![("Nadia Rivera", "nadia.rivera@fastmail.example")], subject: "Re: Birthday plans", hours_ago: 60.0, seen: true, category: "personal",
            html: person(&["Count me in. I'll bring dessert."], "Alex"), attachments: vec![], id: "s4", reply_to_id: None },
        Mail { account: HOME, folder: "Sent", from: me_home, to: vec![("Omar Haddad", "omar.h@fastmail.example")], subject: "Hike on Saturday?", hours_ago: 80.0, seen: true, category: "personal",
            html: person(&["Weather looks perfect. Meet at the trailhead at 6?"], "Alex"), attachments: vec![], id: "s5", reply_to_id: None },
        Mail { account: WORK, folder: "Drafts", from: me_work, to: vec![("Maya Chen", "maya@northwind.dev")], subject: "Launch announcement (draft)", hours_ago: 0.2, seen: true, category: "personal",
            html: person(&["Today we are releasing v1.0: a fast, native email client that feels at home on your desktop..."], ""), attachments: vec![], id: "d1", reply_to_id: None },
        Mail { account: HOME, folder: "Drafts", from: me_home, to: vec![], subject: "Packing list for Lisbon", hours_ago: 19.0, seen: true, category: "personal",
            html: person(&["Passport, charger, rain jacket, camera"], ""), attachments: vec![], id: "d2", reply_to_id: None },
        Mail { account: WORK, folder: "Archive", from: ("Jonas Weber", "jonas@northwind.dev"), to: vec![me_work], subject: "Kickoff notes", hours_ago: 200.0, seen: true, category: "personal",
            html: person(&["Notes from the kickoff are in the wiki."], "Jonas"), attachments: vec![], id: "a1", reply_to_id: None },
        Mail { account: WORK, folder: "Projects/Launch", from: ("Maya Chen", "maya@northwind.dev"), to: vec![me_work], subject: "Press kit assets", hours_ago: 70.0, seen: true, category: "personal",
            html: person(&["Logos, screenshots and the one-pager are in the press kit folder."], "Maya"), attachments: vec![("press-kit.zip", "application/zip", 8_400_000)], id: "f1", reply_to_id: None },
        Mail { account: WORK, folder: "Clients", from: ("Lea Martin", "lea@brightpath.example"), to: vec![me_work], subject: "Kickoff agenda", hours_ago: 120.0, seen: true, category: "personal",
            html: person(&["Agenda for next week's kickoff attached."], "Lea"), attachments: vec![], id: "f2", reply_to_id: None },
        Mail { account: HOME, folder: "Travel", from: ("SkyWays", "trips@skyways.example"), to: vec![me_home], subject: "Your boarding pass", hours_ago: 300.0, seen: true, category: "notifications",
            html: notice("Boarding pass", "#0369a1", &["Gate B12"], "Open"), attachments: vec![], id: "f3", reply_to_id: None },
        Mail { account: WORK, folder: "Spam", from: ("Prize Center", "winner@prize-center.example"), to: vec![me_work], subject: "You have won a free cruise!!!", hours_ago: 10.0, seen: false, category: "personal",
            html: person(&["Click here to claim your prize."], ""), attachments: vec![], id: "sp1", reply_to_id: None },
        Mail { account: WORK, folder: "Trash", from: ("Old Service", "news@oldservice.example"), to: vec![me_work], subject: "We miss you", hours_ago: 90.0, seen: true, category: "newsletters",
            html: person(&["It's been a while."], ""), attachments: vec![], id: "t1", reply_to_id: None },
    ];

    // done threads (shown under Done)
    for (i, (who, addr, subj)) in [
        ("Jonas Weber", "jonas@northwind.dev", "Release notes outline"),
        ("Priya Nair", "priya@northwind.dev", "Icon options"),
        ("Diego Alvarez", "diego@northwind.dev", "CI cache is fixed"),
    ]
    .into_iter()
    .enumerate()
    {
        mails.push(Mail {
            account: WORK,
            folder: "INBOX",
            from: (who, addr),
            to: vec![me_work],
            subject: subj,
            hours_ago: 45.0 + i as f64 * 6.0,
            seen: true,
            category: "personal",
            html: person(&["Done, thanks!"], who),
            attachments: vec![],
            id: ["done1", "done2", "done3"][i],
            reply_to_id: None,
        });
    }

    for (uid, m) in (1u32..).zip(mails.iter()) {
        let addr =
            |l: &[(&str, &str)]| json!(l.iter().map(|(n, a)| json!({"name": n, "mailbox": a})).collect::<Vec<_>>());
        let spaced = m.html.replace("</p>", "\n").replace("<br>", "\n").replace("</div>", "\n").replace("</tr>", "\n");
        let text: String = spaced.split('<').map(|s| s.split_once('>').map(|x| x.1).unwrap_or(s)).collect::<String>();
        let preview: String = text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(160).collect();
        store
            .insert_message(&NewMessage {
                account: m.account.into(),
                folder_id: folder[&(m.account, m.folder)],
                uid,
                message_id: Some(format!("<{}@demo.rustyinbox>", m.id)),
                in_reply_to: m.reply_to_id.map(|r| format!("<{r}@demo.rustyinbox>")),
                refs: m.reply_to_id.map(|r| vec![format!("<{r}@demo.rustyinbox>")]).unwrap_or_default(),
                subject: m.subject.into(),
                from_name: Some(m.from.0.into()),
                from_addr: m.from.1.into(),
                to: addr(&m.to),
                cc: json!([]),
                bcc: json!([]),
                reply_to: json!([]),
                date_ms: now - (m.hours_ago * 3_600_000.0) as i64,
                seen: m.seen,
                flagged: false,
                answered: false,
                draft: m.folder == "Drafts",
                size: m.html.len() as u32,
                has_attachments: !m.attachments.is_empty(),
                preview,
                body_text: Some(text),
                body_html: Some(m.html.clone()),
                category: m.category.into(),
                attachments: m
                    .attachments
                    .iter()
                    .enumerate()
                    .map(|(i, (f, mime, size))| NewAttachment {
                        part: format!("{}", i + 2),
                        filename: (*f).into(),
                        mime: (*mime).into(),
                        size: *size,
                        content_id: None,
                        inline: false,
                    })
                    .collect(),
            })
            .unwrap();
    }

    // pin, done and snooze a few threads by subject
    let rows = store
        .thread_rows(&ListFilter::Inbox { account: None, done: false, category: None, with_done: false }, 500)
        .unwrap();
    let find = |s: &str| rows.iter().find(|r| r.subject == s).map(|r| r.thread_id);
    if let Some(t) = find("Contract draft for Q4") {
        store.set_thread_flag(t, "pinned", true).unwrap();
    }
    for s in ["Release notes outline", "Icon options", "CI cache is fixed"] {
        if let Some(t) = find(s) {
            store.set_thread_flag(t, "done", true).unwrap();
        }
    }
    if let Some(t) = find("Reminder: renew your library card") {
        store.set_snooze(t, Some(now + 2 * 86_400_000)).unwrap();
    }
    store.checkpoint().unwrap();
    println!("seeded {} messages into {}", mails.len(), dir.display());
}
