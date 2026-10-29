//! QML object + background jobs for features 11-15: labels, needs reply, follow-ups, plain-words
//! search, briefing. WORKER C2 owns this file. See docs/ai/DESIGN.md for the contract.
//!
//! One `std::thread` (never the engine's tokio runtime, a long AI call there would stall mail sync)
//! wakes after each sync pass (`wake`, called from `bridge.rs`), when settings change (`refresh`)
//! and every few minutes. Each pass:
//! - labels new inbox mail (Smart filtering, Needs reply): only mail newer than the moment the
//!   feature was switched on, batches of 20, at most [`CALLS_PER_HOUR`] calls an hour,
//! - checks sent mail that is old enough for a follow-up and tells the user once per item,
//! - writes the daily briefing once per day from the chosen hour.
//!
//! A failing call (quota, sign-in, network) is logged and shown once as a quiet status in the sidebar;
//! the runner then waits (5 min, doubling up to 1 h) before it tries again.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type AiInbox = super::AiInboxRust;

        /// AI is configured and on.
        #[qinvokable]
        fn available(self: &AiInbox) -> bool;

        /// Start the background runner (idempotent).
        #[qinvokable]
        fn start(self: Pin<&mut AiInbox>);

        /// Settings changed or the window needs fresh results: run a pass soon.
        #[qinvokable]
        fn refresh(self: &AiInbox);

        /// Which organizing features are usable now: {available, filter, needsReply, followUps, search, briefing}.
        #[qinvokable]
        fn features_json(self: &AiInbox) -> QString;

        /// {error}: what went wrong last (empty when all is well).
        #[qinvokable]
        fn status_json(self: &AiInbox) -> QString;

        /// The newest briefing: {state: idle|loading|error, error, day, markdown, atMs, today, has}.
        #[qinvokable]
        fn briefing_json(self: &AiInbox) -> QString;

        /// Write the briefing now (the Refresh button); answers with `briefing_changed`.
        #[qinvokable]
        fn refresh_briefing(self: &AiInbox);

        /// Turn a question into a search query; answers with `ask_done(id, query, error)`. Returns the id.
        #[qinvokable]
        fn ask(self: Pin<&mut AiInbox>, question: &QString) -> i64;

        /// "Not needed": clear Needs reply and the follow-up of a conversation.
        #[qinvokable]
        fn dismiss(self: &AiInbox, thread_id: i64);

        /// The conversation a message (as named in a briefing link) belongs to, 0 if gone.
        #[qinvokable]
        fn thread_of_message(self: &AiInbox, message_id: i64) -> i64;

        /// New labels, follow-ups or counts: reload the lists.
        #[qsignal]
        fn labels_changed(self: Pin<&mut AiInbox>);

        /// The error status changed.
        #[qsignal]
        fn status_changed(self: Pin<&mut AiInbox>);

        /// The briefing finished, failed or started.
        #[qsignal]
        fn briefing_changed(self: Pin<&mut AiInbox>);

        /// Result of `ask`: the query, or an error sentence.
        #[qsignal]
        fn ask_done(self: Pin<&mut AiInbox>, id: i64, query: QString, error: QString);

        /// A notification about a follow-up was clicked.
        #[qsignal]
        fn open_requested(self: Pin<&mut AiInbox>, thread_id: i64);

        /// The briefing notification was clicked.
        #[qsignal]
        fn briefing_requested(self: Pin<&mut AiInbox>);
    }

    impl cxx_qt::Threading for AiInbox {}
}

use crate::{ai, engine};
use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use rusty_inbox_ai::features::organize as org;
use rusty_inbox_ai::features::MailForAi;
use rusty_inbox_core::ai_store::{AiLabelIn, AiMessage};
use rusty_inbox_core::store::Store;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Most model calls the background jobs make in an hour.
const CALLS_PER_HOUR: usize = 60;
/// Batches of mail labelled in one pass.
const BATCHES_PER_PASS: usize = 5;
/// Follow-up checks in one pass.
const FOLLOW_UP_CHECKS_PER_PASS: usize = 10;
/// Sent mail older than this is not looked at for follow-ups.
const FOLLOW_UP_WINDOW_DAYS: i64 = 30;
/// The runner looks around at least this often even when nothing woke it.
const IDLE_WAIT: Duration = Duration::from_secs(5 * 60);
const MIN_PASS_GAP: Duration = Duration::from_secs(5);
const DAY_MS: i64 = 86_400_000;

#[derive(Default)]
pub struct AiInboxRust {
    started: bool,
}

type Qt = cxx_qt::CxxQtThread<qobject::AiInbox>;

#[derive(Default)]
struct Wake {
    poke: bool,
    briefing: bool,
}

#[derive(Default)]
struct Shared {
    error: String,
    failures: u32,
    retry_at: Option<Instant>,
    calls: VecDeque<Instant>,
    /// unreadable answers per message id (after three the message is left unlabelled for good)
    bad_answers: HashMap<i64, u8>,
    briefing_busy: bool,
    briefing_error: String,
}

struct Runner {
    wake: Mutex<Wake>,
    cv: Condvar,
    shared: Mutex<Shared>,
    qt: Mutex<Option<Qt>>,
}

fn runner() -> &'static Runner {
    static R: OnceLock<Runner> = OnceLock::new();
    R.get_or_init(|| Runner {
        wake: Default::default(),
        cv: Condvar::new(),
        shared: Default::default(),
        qt: Mutex::new(None),
    })
}

/// A sync pass or a push delivered mail: look for work soon. Cheap; called from `bridge.rs`.
pub fn wake() {
    let r = runner();
    if let Ok(mut w) = r.wake.lock() {
        w.poke = true;
    }
    r.cv.notify_one();
}

/// The scripted UI check treats the seeded demo mailbox as new mail and never shows a notification.
fn demo_mode() -> bool {
    static DEMO: OnceLock<bool> = OnceLock::new();
    *DEMO.get_or_init(|| std::env::args().any(|a| a == "--autotest-ai-organize"))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn emit(f: impl FnOnce(Pin<&mut qobject::AiInbox>) + Send + 'static) {
    if let Some(qt) = runner().qt.lock().ok().and_then(|g| g.clone()) {
        let _ = qt.queue(f);
    }
}

fn emit_labels() {
    emit(|mut o| o.as_mut().labels_changed());
}
fn emit_status() {
    emit(|mut o| o.as_mut().status_changed());
}
fn emit_briefing() {
    emit(|mut o| o.as_mut().briefing_changed());
}

impl qobject::AiInbox {
    fn available(&self) -> bool {
        ai::available()
    }

    fn start(mut self: Pin<&mut Self>) {
        if self.rust().started {
            return;
        }
        self.as_mut().rust_mut().started = true;
        let r = runner();
        if let Ok(mut g) = r.qt.lock() {
            *g = Some(self.qt_thread());
        }
        if std::thread::Builder::new().name("ai-inbox".into()).spawn(run_loop).is_err() {
            tracing::warn!("ai-inbox: could not start the runner thread");
        }
        wake();
    }

    fn refresh(&self) {
        wake();
    }

    fn features_json(&self) -> QString {
        QString::from(&features().to_string())
    }

    fn status_json(&self) -> QString {
        let s = runner().shared.lock().map(|s| s.error.clone()).unwrap_or_default();
        QString::from(&serde_json::json!({ "error": s }).to_string())
    }

    fn briefing_json(&self) -> QString {
        QString::from(&briefing_state().to_string())
    }

    fn refresh_briefing(&self) {
        let r = runner();
        if let Ok(mut w) = r.wake.lock() {
            w.briefing = true;
            w.poke = true;
        }
        if let Ok(mut s) = r.shared.lock() {
            s.briefing_busy = true;
            s.briefing_error.clear();
        }
        r.cv.notify_one();
    }

    fn ask(self: Pin<&mut Self>, question: &QString) -> i64 {
        static NEXT: AtomicI64 = AtomicI64::new(1);
        let id = NEXT.fetch_add(1, Ordering::SeqCst);
        let question = question.to_string();
        let qt = self.qt_thread();
        std::thread::spawn(move || {
            let (query, error) = match ask_model(&question) {
                Ok(q) => (q, String::new()),
                Err(e) => (String::new(), e),
            };
            let _ = qt.queue(move |mut o: Pin<&mut qobject::AiInbox>| {
                o.as_mut().ask_done(id, QString::from(&query), QString::from(&error))
            });
        });
        id
    }

    fn dismiss(&self, thread_id: i64) {
        if let Err(e) = engine::core().store().ai_dismiss_thread(thread_id) {
            tracing::warn!("ai-inbox: dismiss {thread_id}: {e}");
        }
    }

    fn thread_of_message(&self, message_id: i64) -> i64 {
        engine::core().store().thread_of_message(message_id).ok().flatten().unwrap_or(0)
    }
}

// ---- the question box ---------------------------------------------------------------------------

fn ask_model(question: &str) -> Result<String, String> {
    if question.trim().is_empty() {
        return Err("Type a question first.".into());
    }
    let accounts = allowed_accounts(engine::core().store());
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let req = org::search_query(question, &today, &accounts);
    let out = ai::run(req, &mut |_| {}, &AtomicBool::new(false))?;
    org::parse_search_query(&out.text).ok_or_else(|| {
        tracing::info!("ai-inbox: unusable search answer ({} chars)", out.text.len());
        "The answer could not be turned into a search. Try other words, or type a query such as \
         from:name has:attachments since:2026-01-01."
            .to_string()
    })
}

// ---- state for QML ------------------------------------------------------------------------------

fn features() -> serde_json::Value {
    let avail = ai::available();
    let on = |k: &str| avail && ai::feature_on(k);
    serde_json::json!({
        "available": avail,
        "filter": on("aiSmartFilter"),
        "needsReply": on("aiNeedsReply"),
        "followUps": on("aiFollowUps"),
        "search": on("aiSearch"),
        "briefing": on("aiBriefing"),
    })
}

fn briefing_state() -> serde_json::Value {
    let store = engine::core().store();
    let (busy, error) = runner().shared.lock().map(|s| (s.briefing_busy, s.briefing_error.clone())).unwrap_or_default();
    let latest = store.ai_latest_briefing().ok().flatten();
    let state = if busy {
        "loading"
    } else if !error.is_empty() {
        "error"
    } else {
        "idle"
    };
    serde_json::json!({
        "state": state,
        "error": error,
        "day": latest.as_ref().map(|l| l.0.clone()).unwrap_or_default(),
        "markdown": latest.as_ref().map(|l| l.1.clone()).unwrap_or_default(),
        "atMs": latest.as_ref().map(|l| l.2).unwrap_or(0),
        "today": local_day(),
        "has": latest.is_some(),
    })
}

fn local_day() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

// ---- the runner ---------------------------------------------------------------------------------

fn run_loop() {
    let r = runner();
    let mut last_pass: Option<Instant> = None;
    loop {
        let force_briefing = {
            let Ok(mut w) = r.wake.lock() else { return };
            if !w.poke && !w.briefing {
                w = match r.cv.wait_timeout(w, IDLE_WAIT) {
                    Ok((g, _)) => g,
                    Err(_) => return,
                };
            }
            w.poke = false;
            std::mem::take(&mut w.briefing)
        };
        if let Some(t) = last_pass {
            if !force_briefing && t.elapsed() < MIN_PASS_GAP {
                std::thread::sleep(MIN_PASS_GAP - t.elapsed());
            }
        }
        last_pass = Some(Instant::now());
        // a panic in one pass must not end the runner
        if std::panic::catch_unwind(|| pass(force_briefing)).is_err() {
            tracing::error!("ai-inbox: a pass panicked");
            if force_briefing {
                finish_briefing("The briefing failed unexpectedly.");
            }
        }
    }
}

fn allowed_accounts(store: &Store) -> Vec<String> {
    store.accounts().unwrap_or_default().into_iter().map(|a| a.email).filter(|e| !ai::excluded(e)).collect()
}

/// When `on`, the time the feature was first seen on (stored); `None` and forgotten when off, so
/// switching it on again starts from then. The demo check treats the seeded mail as new.
fn switched_on_at(store: &Store, key: &str, on: bool, now: i64) -> Option<i64> {
    let key = format!("since:{key}");
    if !on {
        let _ = store.ai_clear_state(&key);
        return None;
    }
    if let Some(t) = store.ai_state(&key).and_then(|v| v.parse().ok()) {
        return Some(t);
    }
    let t = if demo_mode() { 0 } else { now };
    let _ = store.ai_set_state(&key, &t.to_string());
    Some(t)
}

fn pass(force_briefing: bool) {
    let store = engine::core().store();
    let now = now_ms();
    let filter = ai::feature_on("aiSmartFilter");
    let needs_reply = ai::feature_on("aiNeedsReply");
    let follow_ups = ai::feature_on("aiFollowUps");
    let brief = ai::feature_on("aiBriefing");

    // the category override follows the switch at once (and back: the rule category was never changed)
    if store.ai_labels_applied() != filter {
        store.set_ai_labels_applied(filter);
        emit_labels();
    }
    let since_filter = switched_on_at(store, "filter", filter, now);
    let since_reply = switched_on_at(store, "needs_reply", needs_reply, now);
    switched_on_at(store, "follow_ups", follow_ups, now);
    switched_on_at(store, "briefing", brief, now);

    let allowed = allowed_accounts(store);
    if !ai::available() || allowed.is_empty() {
        if force_briefing {
            finish_briefing("AI is not set up. Choose a provider and model in Settings → AI.");
        }
        return;
    }
    let me = allowed.join(", ");

    if !backing_off() {
        if let Some(since) = [since_filter, since_reply].into_iter().flatten().min() {
            label_new_mail(store, &allowed, &me, since, needs_reply);
        }
    }
    if follow_ups && !backing_off() {
        check_follow_ups(store, &allowed);
        notify_follow_ups(store);
    }
    if brief || force_briefing {
        write_briefing(store, &allowed, &me, force_briefing, brief);
    }
}

// ---- the budget and the quiet error status ------------------------------------------------------

fn backing_off() -> bool {
    runner().shared.lock().map(|s| s.retry_at.is_some_and(|t| Instant::now() < t)).unwrap_or(true)
}

/// Spend one model call from this hour's budget; false when it is used up.
fn spend_call() -> bool {
    let Ok(mut s) = runner().shared.lock() else { return false };
    let now = Instant::now();
    while s.calls.front().is_some_and(|t| now.duration_since(*t) > Duration::from_secs(3600)) {
        s.calls.pop_front();
    }
    if s.calls.len() >= CALLS_PER_HOUR {
        return false;
    }
    s.calls.push_back(now);
    true
}

fn succeeded() {
    let changed = runner()
        .shared
        .lock()
        .map(|mut s| {
            let had = !s.error.is_empty();
            s.error.clear();
            s.failures = 0;
            s.retry_at = None;
            had
        })
        .unwrap_or(false);
    if changed {
        emit_status();
    }
}

/// Remember the error, show it once, and wait before the next try (5 min, doubling, at most 1 h).
fn failed(what: &str, e: &str) {
    tracing::warn!("ai-inbox: {what}: {e}");
    let changed = runner()
        .shared
        .lock()
        .map(|mut s| {
            s.failures += 1;
            let wait = (300u64 << (s.failures - 1).min(4)).min(3600);
            s.retry_at = Some(Instant::now() + Duration::from_secs(wait));
            let changed = s.error != e;
            s.error = e.to_string();
            changed
        })
        .unwrap_or(false);
    if changed {
        emit_status();
    }
}

fn for_ai(m: &AiMessage, from_me: bool) -> MailForAi {
    let from = match m.from_name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => format!("{n} <{}>", m.from_addr),
        None => m.from_addr.clone(),
    };
    let date = chrono::DateTime::from_timestamp_millis(m.date_ms)
        .map(|d| d.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_default();
    MailForAi {
        id: m.id,
        from,
        to: m.to.clone(),
        cc: m.cc.clone(),
        date,
        subject: m.subject.clone(),
        body: m.body.clone(),
        attachments: m.attachments.clone(),
        from_me,
    }
}

// ---- 11 + 12: labels ----------------------------------------------------------------------------

fn label_new_mail(store: &Store, allowed: &[String], me: &str, since: i64, with_invitations: bool) {
    let lang = ai::lang();
    let mut changed = false;
    for _ in 0..BATCHES_PER_PASS {
        let Ok(batch) = store.ai_unlabeled(allowed, since, with_invitations, org::MAX_BATCH) else { break };
        if batch.is_empty() {
            break;
        }
        if !spend_call() {
            tracing::info!("ai-inbox: hourly call budget used up, waiting");
            break;
        }
        let mails: Vec<MailForAi> = batch.iter().map(|m| for_ai(m, false)).collect();
        let req = org::classify_in(&mails, me, &lang);
        let out = match ai::run(req, &mut |_| {}, &AtomicBool::new(false)) {
            Ok(o) => o,
            Err(e) => {
                failed("classify", &e);
                break;
            }
        };
        let labels = org::parse_labels(&out.text, &mails);
        if labels.is_empty() {
            // unreadable answer: try again later, give up on these after three tries
            let mut give_up = vec![];
            if let Ok(mut s) = runner().shared.lock() {
                for m in &batch {
                    let n = s.bad_answers.entry(m.id).or_default();
                    *n += 1;
                    if *n >= 3 {
                        give_up.push(m.id);
                    }
                }
            }
            let _ = store.ai_save_skipped(&give_up, now_ms());
            failed("classify", "The model's answer could not be read.");
            break;
        }
        let rows: Vec<AiLabelIn> = labels
            .iter()
            .map(|l| AiLabelIn {
                message_id: l.id,
                category: l.category.clone(),
                needs_reply: l.needs_reply,
                priority: l.priority,
                reason: l.reason.clone(),
            })
            .collect();
        if let Err(e) = store.ai_save_labels(&rows, &out.model, now_ms()) {
            tracing::warn!("ai-inbox: saving labels: {e}");
            break;
        }
        // the model left some out: not asked again
        let left_out: Vec<i64> = batch.iter().map(|m| m.id).filter(|id| !labels.iter().any(|l| l.id == *id)).collect();
        let _ = store.ai_save_skipped(&left_out, now_ms());
        changed = true;
        succeeded();
    }
    if changed {
        emit_labels();
    }
}

// ---- 13: follow-ups -----------------------------------------------------------------------------

fn check_follow_ups(store: &Store, allowed: &[String]) {
    let days = crate::settings::get(store, "aiFollowUpDays").as_i64().unwrap_or(3).clamp(1, 30);
    let now = now_ms();
    let Ok(batch) = store.ai_followup_candidates(
        allowed,
        now - FOLLOW_UP_WINDOW_DAYS * DAY_MS,
        now - days * DAY_MS,
        FOLLOW_UP_CHECKS_PER_PASS,
    ) else {
        return;
    };
    let lang = ai::lang();
    let mut changed = false;
    for m in batch {
        if !spend_call() {
            break;
        }
        let req = org::expects_reply_in(&for_ai(&m, true), &lang);
        let out = match ai::run(req, &mut |_| {}, &AtomicBool::new(false)) {
            Ok(o) => o,
            Err(e) => {
                failed("follow-up check", &e);
                break;
            }
        };
        match org::parse_expects_reply(&out.text) {
            Some((expects, awaiting)) => {
                let _ = store.ai_save_followup(m.id, expects, &awaiting, now_ms());
                changed = true;
                succeeded();
            }
            None => {
                let give_up = runner()
                    .shared
                    .lock()
                    .map(|mut s| {
                        let n = s.bad_answers.entry(m.id).or_default();
                        *n += 1;
                        *n >= 3
                    })
                    .unwrap_or(false);
                if give_up {
                    let _ = store.ai_save_followup(m.id, false, "", now_ms());
                }
                failed("follow-up check", "The model's answer could not be read.");
                break;
            }
        }
    }
    if changed {
        emit_labels();
    }
}

/// One notification per waiting conversation, at most three at a time (the rest in one summary).
fn notify_follow_ups(store: &Store) {
    let Ok(items) = store.ai_followups_to_notify() else { return };
    for (i, it) in items.iter().enumerate() {
        let _ = store.ai_mark_notified(it.message_id);
        if i < 3 {
            let body = if it.awaiting.is_empty() {
                it.subject.clone()
            } else {
                format!("{}\nWaiting for: {}", it.subject, it.awaiting)
            };
            notify("No reply yet", &body, Some(it.thread_id));
        }
    }
    if items.len() > 3 {
        notify("No reply yet", &format!("{} more conversations are waiting for an answer", items.len() - 3), None);
    }
}

/// A desktop notification like the new-mail ones (same switches); a click opens the conversation
/// (`thread`) or, without one, the briefing. Never shown by the scripted check.
fn notify(title: &str, body: &str, thread: Option<i64>) {
    let store = engine::core().store();
    if demo_mode() {
        tracing::info!("ai-inbox (demo): notification '{title}'");
        return;
    }
    if crate::settings::get(store, "notifications") != serde_json::json!(true) {
        return;
    }
    let preview = crate::settings::get(store, "notificationPreview") == serde_json::json!(true);
    let body = if preview { body.to_string() } else { "Open the app to see it".to_string() };
    let title = title.to_string();
    let for_briefing = thread.is_none() && title.contains("briefing");
    std::thread::spawn(move || {
        let shown = notify_rust::Notification::new()
            .appname(crate::APP_NAME)
            .summary(&title)
            .body(&body)
            .icon(crate::APP_ID)
            .action("default", "Open")
            .show();
        match shown {
            #[cfg(all(unix, not(target_os = "macos")))]
            Ok(handle) => handle.wait_for_action(move |a| {
                if a == "default" {
                    match (thread, for_briefing) {
                        (Some(t), _) => emit(move |mut o| o.as_mut().open_requested(t)),
                        (None, true) => emit(|mut o| o.as_mut().briefing_requested()),
                        _ => {}
                    }
                }
            }),
            #[cfg(not(all(unix, not(target_os = "macos"))))]
            Ok(_) => {
                let _ = (thread, for_briefing);
            }
            Err(e) => tracing::warn!("ai-inbox: notification: {e}"),
        }
    });
}

// ---- 15: briefing -------------------------------------------------------------------------------

/// The briefing is no longer being written (`error` empty = it worked).
fn finish_briefing(error: &str) {
    if let Ok(mut s) = runner().shared.lock() {
        s.briefing_busy = false;
        s.briefing_error = error.to_string();
    }
    emit_briefing();
}

fn write_briefing(store: &Store, allowed: &[String], me: &str, forced: bool, scheduled_on: bool) {
    use chrono::Timelike;
    let day = local_day();
    if !forced {
        let hour = crate::settings::get(store, "aiBriefingHour").as_u64().unwrap_or(8) as u32;
        let have = store.ai_briefing(&day).ok().flatten().is_some();
        if !scheduled_on || have || chrono::Local::now().hour() < hour || backing_off() {
            return;
        }
        if let Ok(mut s) = runner().shared.lock() {
            s.briefing_busy = true;
        }
        emit_briefing();
    }
    let unread = store.ai_unread_inbox(allowed, org::BRIEFING_MAX).unwrap_or_default();
    if unread.is_empty() {
        let _ = store.ai_save_briefing(&day, "", now_ms());
        finish_briefing("");
        return;
    }
    if !forced && !spend_call() {
        finish_briefing("");
        return;
    }
    let mails: Vec<MailForAi> = unread.iter().map(|m| for_ai(m, false)).collect();
    let req = org::briefing(&mails, me, &ai::lang());
    let result = ai::run(req, &mut |_| {}, &AtomicBool::new(false))
        .and_then(|o| org::parse_briefing(&o.text, &mails).ok_or_else(|| "The model's answer was empty.".to_string()));
    match result {
        Ok(md) => {
            let _ = store.ai_save_briefing(&day, &md, now_ms());
            finish_briefing("");
            succeeded();
            if !forced {
                notify("Your briefing is ready", &format!("{} unread messages, summarized", mails.len()), None);
            }
        }
        Err(e) => {
            finish_briefing(&e);
            if !forced {
                failed("briefing", &e);
            }
        }
    }
}
