//! Live check of composing against a real account (sends only to the account itself).
//!
//! `cargo run --release -p rusty-inbox-core --example send_probe -- <data dir> <account>`
//!
//! 1. saves a draft to the server's Drafts folder, then saves an edited version (the first copy
//!    must be replaced, not duplicated), 2. sends it to the account itself through the outbox,
//! 3. syncs and checks the copy in Sent and that the draft is gone, 4. replies to the received
//!    message and checks the reply threads with it.

use rusty_inbox_core::compose::{self, Addr};
use rusty_inbox_core::{Engine, EngineConfig};
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("data dir");
    let account = args.next().expect("account");
    let e = Engine::new(EngineConfig::for_dir(std::path::Path::new(&dir)))?;
    let store = e.store();
    let tag = format!("ri-probe-{}", chrono::Utc::now().timestamp());

    let mut d = compose::new_draft(store, &account, "Sent by the Rusty Inbox send probe");
    d.to.push(Addr { name: Some("Me".into()), address: account.clone() });
    d.subject = format!("{tag} first");
    d.html = "<p>Draft one</p>".into();
    let id = e.save_draft(0, &d)?;
    println!("draft upload 1: {}", e.upload_draft(&d).await);
    d.subject = format!("{tag} subject");
    d.html = "<p>Hello from <b>Rusty Inbox</b>.</p><ul><li>one</li><li>two</li></ul>".into();
    e.save_draft(id, &d)?;
    println!("draft upload 2: {}", e.upload_draft(&d).await);
    e.sync_account(&account).await?;
    let drafts_with_tag = count(store, &account, "drafts", &tag);
    println!("server drafts with tag after 2 saves: {drafts_with_tag} (want 1)");

    e.queue(id, &d, 0)?;
    println!("outbox: {:?}", drain(&e, id).await);
    tokio::time::sleep(Duration::from_secs(4)).await;
    e.sync_account(&account).await?;
    println!("drafts with tag after send: {} (want 0)", count(store, &account, "drafts", &tag));
    println!("sent with tag: {} (want 1)", count(store, &account, "sent", &tag));
    println!("inbox with tag: {} (want 1)", count(store, &account, "inbox", &tag));

    // reply to what arrived
    let Some(received) = find(store, &account, "inbox", &tag) else {
        println!("not received yet");
        return Ok(());
    };
    let mine = vec![account.clone()];
    let mut rd = compose::respond(store, received, "reply", &mine, "").expect("reply draft");
    rd.html = format!("<p>Reply works.</p>{}", rd.html);
    println!("reply to={:?} subject={:?} in_reply_to={:?}", rd.to, rd.subject, rd.in_reply_to);
    let rid = e.save_draft(0, &rd)?;
    e.queue(rid, &rd, 0)?;
    println!("outbox: {:?}", drain(&e, rid).await);
    tokio::time::sleep(Duration::from_secs(4)).await;
    e.sync_account(&account).await?;
    let (thread, _) = store.message(received)?.map(|(t, m)| (t, m.id)).unwrap();
    let in_thread = store.thread_messages(thread)?;
    println!("messages in the conversation: {} (want ≥3: sent, received, reply)", in_thread.len());
    println!("TAG {tag}");
    Ok(())
}

/// Run the outbox until message `id` is sent (row gone) or given up; returns every attempt.
async fn drain(e: &Engine, id: i64) -> Vec<(i64, Result<(), String>)> {
    let mut all = vec![];
    for _ in 0..60 {
        all.extend(e.process_outbox().await);
        match e.store().outgoing(id).ok().flatten() {
            None => break,
            Some(r) if r.state == "failed" => break,
            _ => tokio::time::sleep(Duration::from_secs(5)).await,
        }
    }
    all
}

fn ids(store: &rusty_inbox_core::store::Store, account: &str, role: &str, tag: &str) -> Vec<i64> {
    let Ok(Some(f)) = store.folder_by_role(account, role) else { return vec![] };
    store.folder_messages_matching(f.id, tag).unwrap_or_default()
}

fn count(store: &rusty_inbox_core::store::Store, account: &str, role: &str, tag: &str) -> usize {
    ids(store, account, role, tag).len()
}

fn find(store: &rusty_inbox_core::store::Store, account: &str, role: &str, tag: &str) -> Option<i64> {
    ids(store, account, role, tag).into_iter().next()
}
