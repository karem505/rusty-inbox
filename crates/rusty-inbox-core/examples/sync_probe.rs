//! Sync one account once and report (debugging aid).
//! `cargo run --release -p rusty-inbox-core --example sync_probe -- <data dir> <account>`
use rusty_inbox_core::{Engine, EngineConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let dir = a.next().expect("data dir");
    let acct = a.next().expect("account");
    let e = Engine::new(EngineConfig::for_dir(std::path::Path::new(&dir)))?;
    let t = std::time::Instant::now();
    match e.sync_account(&acct).await {
        Ok(n) => println!("sync ok: {n} new in {:?}", t.elapsed()),
        Err(err) => println!("sync FAILED after {:?}: {err:#}", t.elapsed()),
    }
    Ok(())
}
