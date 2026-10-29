//! QML object for the composer and reader AI tools (features 1-10, 16): every call gets an id (i64),
//! runs on its own `std::thread` (never on the Qt thread or the engine's runtime), streams the
//! answer so far with `partial(id, text)` (at most every 50 ms), and ends with `finished(id, json)` or
//! `failed(id, message)`. `cancel(id)` stops a call; a cancelled call reports nothing more.
//! Reader results are cached in memory per (feature, conversation, newest message, language).
//! See docs/ai/DESIGN.md for the contract.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type Assistant = super::AssistantRust;

        /// AI is configured and on.
        #[qinvokable]
        fn available(self: &Assistant) -> bool;

        /// The master switch and the feature's switch (`aiComposeTools`, `aiReadTools`, `aiAutoSummary`, ...) are on.
        #[qinvokable]
        fn feature_on(self: &Assistant, key: &QString) -> bool;

        /// Stop a running call; it reports nothing more.
        #[qinvokable]
        fn cancel(self: &Assistant, id: i64);

        /// Composer: fix spelling, grammar and punctuation of `text`. Result `{text, diffHtml, changed}`.
        #[qinvokable]
        fn proofread(self: Pin<&mut Assistant>, text: &QString, account: &QString) -> i64;

        /// Composer: rewrite `text` in a style (formal, friendly, shorter, longer, clearer, persuasive). Result `{text}`.
        #[qinvokable]
        fn rewrite(self: Pin<&mut Assistant>, text: &QString, style: &QString, account: &QString) -> i64;

        /// Composer: translate `text` into the language named `target`. Result `{text}`.
        #[qinvokable]
        fn translate_draft(self: Pin<&mut Assistant>, text: &QString, target: &QString, account: &QString) -> i64;

        /// Composer: write a whole email from `instruction`; `source_message_id` is the message being replied to (0 = new). Result `{text}`.
        #[qinvokable]
        fn write_for_me(
            self: Pin<&mut Assistant>,
            instruction: &QString,
            source_message_id: i64,
            account: &QString,
        ) -> i64;

        /// Composer: three subject lines for `text`. Result `{subjects: [..]}`.
        #[qinvokable]
        fn subject_ideas(self: Pin<&mut Assistant>, text: &QString, account: &QString) -> i64;

        /// Reader: summary and action items of the conversation of `message_id`. Result `{summary, actions: [{task, owner, due}]}`.
        #[qinvokable]
        fn summarize(self: Pin<&mut Assistant>, message_id: i64) -> i64;

        /// Reader: translate the newest message of the conversation (`target` "" = automatic). Result `{text, target, messageId}`.
        #[qinvokable]
        fn translate_message(self: Pin<&mut Assistant>, message_id: i64, target: &QString) -> i64;

        /// Reader: three short replies. Result `{replies: [..]}`.
        #[qinvokable]
        fn smart_replies(self: Pin<&mut Assistant>, message_id: i64) -> i64;

        /// Reader: phishing check of the newest message from someone else. Result `{risk, reasons, messageId}`.
        #[qinvokable]
        fn phishing(self: Pin<&mut Assistant>, message_id: i64) -> i64;

        /// Reader: summary of an attachment (downloads it first if needed). Result `{text, name}`.
        #[qinvokable]
        fn summarize_attachment(self: Pin<&mut Assistant>, attachment_id: i64) -> i64;

        /// The newest message's sender never wrote to you before and you never wrote to them.
        #[qinvokable]
        fn sender_is_new(self: &Assistant, message_id: i64) -> bool;

        /// Whether a file type can be summarized.
        #[qinvokable]
        fn attachment_supported(self: &Assistant, mime: &QString, name: &QString) -> bool;

        /// The text so far of a streaming call.
        #[qsignal]
        fn partial(self: Pin<&mut Assistant>, id: i64, text: QString);

        /// A call ended well: the result as JSON (shapes above).
        #[qsignal]
        fn finished(self: Pin<&mut Assistant>, id: i64, result_json: QString);

        /// A call failed: a sentence for the user (the provider's own message when it gave one).
        #[qsignal]
        fn failed(self: Pin<&mut Assistant>, id: i64, message: QString);
    }

    impl cxx_qt::Threading for Assistant {}
}

use crate::{ai, engine};
use core::pin::Pin;
use cxx_qt::{CxxQtThread, Threading};
use cxx_qt_lib::QString;
use rusty_inbox_ai::features::compose as fc;
use rusty_inbox_ai::features::read as fr;
use rusty_inbox_ai::features::{Lang, MailForAi};
use rusty_inbox_ai::{extract, ChatRequest};
use rusty_inbox_core::ai_support;
use rusty_inbox_core::store::MessageRow;
use serde_json::{json, Value};

/// Turns the model's full answer into the result JSON sent to QML (or a message for the user).
type Finish = Box<dyn FnOnce(&str) -> Result<Value, String> + Send>;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct AssistantRust {}

const COMPOSE: &[&str] = &["aiComposeTools"];
const READ: &[&str] = &["aiReadTools"];
const READ_OR_SUMMARY: &[&str] = &["aiReadTools", "aiAutoSummary"];
const READ_OR_REPLIES: &[&str] = &["aiReadTools", "aiAutoReplies"];
const READ_OR_PHISHING: &[&str] = &["aiReadTools", "aiAutoPhishing"];
/// An answer streams to the screen at most this often.
const STREAM_EVERY: Duration = Duration::from_millis(50);
/// Reader results kept in memory.
const CACHE_MAX: usize = 300;
/// A conversation longer than this is cut to its newest messages.
const THREAD_MESSAGES: usize = 12;

static NEXT_ID: AtomicI64 = AtomicI64::new(1);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn cancels() -> &'static Mutex<HashMap<i64, Arc<AtomicBool>>> {
    static C: OnceLock<Mutex<HashMap<i64, Arc<AtomicBool>>>> = OnceLock::new();
    C.get_or_init(Default::default)
}

fn cache() -> &'static Mutex<HashMap<String, Value>> {
    static C: OnceLock<Mutex<HashMap<String, Value>>> = OnceLock::new();
    C.get_or_init(Default::default)
}

/// A call prepared on the helper thread: the request, whether to stream the text and how to turn the
/// finished answer into the result.
struct Prepared {
    req: ChatRequest,
    stream: bool,
    cache_key: Option<String>,
    finish: Finish,
}

type Prep = Box<dyn FnOnce() -> Result<Prepared, String> + Send>;

fn qs(s: &str) -> QString {
    QString::from(s)
}

/// Run `prep` and the model call on a new thread and report back through the signals.
fn launch(qt: CxxQtThread<qobject::Assistant>, keys: &'static [&'static str], prep: Prep) -> i64 {
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let cancel = Arc::new(AtomicBool::new(false));
    lock(cancels()).insert(id, cancel.clone());
    std::thread::spawn(move || {
        let result = run_job(&qt, id, keys, prep, &cancel);
        lock(cancels()).remove(&id);
        if cancel.load(Ordering::SeqCst) {
            return; // the user cancelled: the screen has moved on
        }
        let _ = match result {
            Ok(v) => {
                let json = v.to_string();
                qt.queue(move |mut o: Pin<&mut qobject::Assistant>| o.as_mut().finished(id, qs(&json)))
            }
            Err(m) => qt.queue(move |mut o: Pin<&mut qobject::Assistant>| o.as_mut().failed(id, qs(&m))),
        };
    });
    id
}

fn run_job(
    qt: &CxxQtThread<qobject::Assistant>,
    id: i64,
    keys: &'static [&'static str],
    prep: Prep,
    cancel: &AtomicBool,
) -> Result<Value, String> {
    if !keys.iter().any(|k| ai::feature_on(k)) {
        return Err("This AI feature is switched off. Turn it on in Settings → AI.".into());
    }
    ai::active()?;
    let p = prep()?;
    if let Some(hit) = p.cache_key.as_deref().and_then(|k| lock(cache()).get(k).cloned()) {
        return Ok(hit);
    }
    let mut acc = String::new();
    let mut last = Instant::now() - STREAM_EVERY;
    let stream = p.stream;
    let out = ai::run(
        p.req,
        &mut |d| {
            acc.push_str(d);
            if stream && last.elapsed() >= STREAM_EVERY {
                last = Instant::now();
                let text = acc.clone();
                let _ = qt.queue(move |mut o: Pin<&mut qobject::Assistant>| o.as_mut().partial(id, qs(&text)));
            }
        },
        cancel,
    )?;
    let value = (p.finish)(&out.text)?;
    if let Some(k) = p.cache_key {
        let mut c = lock(cache());
        if c.len() >= CACHE_MAX {
            c.clear();
        }
        c.insert(k, value.clone());
    }
    Ok(value)
}

// ---- reading the store ----------------------------------------------------------------------------

/// Nothing from an excluded account is ever sent.
fn guard_account(account: &str) -> Result<(), String> {
    if !account.trim().is_empty() && ai::excluded(account) {
        return Err(format!("AI is turned off for {account}. Change it in Settings → AI."));
    }
    Ok(())
}

fn mailbox_list(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    let addr =
                        x["mailbox"].as_str().or(x["address"].as_str()).map(str::trim).filter(|a| !a.is_empty())?;
                    let name = x["name"].as_str().map(str::trim).filter(|n| !n.is_empty());
                    Some(match name {
                        Some(n) => format!("{n} <{addr}>"),
                        None => addr.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn who(name: &Option<String>, addr: &str) -> String {
    match name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => format!("{n} <{addr}>"),
        None => addr.to_string(),
    }
}

/// `YYYY-MM-DD HH:MM` (UTC) of a time in milliseconds.
fn date_text(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", rem / 3600, rem % 3600 / 60)
}

/// The text of a message for a prompt: plain body (or the HTML turned to text), without quoted history.
fn body_for_ai(m: &MessageRow) -> String {
    let text = match (&m.body_text, &m.body_html) {
        (Some(t), _) if !t.trim().is_empty() => t.clone(),
        (_, Some(h)) if !h.trim().is_empty() => rusty_inbox_core::compose::html_to_text(h),
        _ => m.preview.clone(),
    };
    ai_support::strip_quoted(&text)
}

/// A To / Cc list for the model with the user's own addresses marked.
fn mark_mine(v: &Value, mine: &[String]) -> String {
    let addrs: Vec<&str> = v
        .as_array()
        .map(|a| a.iter().filter_map(|x| x["mailbox"].as_str().or(x["address"].as_str())).collect())
        .unwrap_or_default();
    mailbox_list(v)
        .into_iter()
        .zip(addrs.iter().map(|a| a.trim()).filter(|a| !a.is_empty()))
        .map(|(shown, addr)| {
            if mine.iter().any(|m| m.eq_ignore_ascii_case(addr)) {
                format!("{shown}{}", rusty_inbox_ai::features::USER_MARK)
            } else {
                shown
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn mail_of(m: &MessageRow, mine: &[String]) -> MailForAi {
    MailForAi {
        id: m.id,
        from: who(&m.from_name, &m.from_addr),
        to: mark_mine(&m.to, mine),
        cc: mark_mine(&m.cc, mine),
        date: date_text(m.date_ms),
        subject: m.subject.clone(),
        body: body_for_ai(m),
        attachments: m.attachments.iter().filter(|a| !a.4).map(|a| a.1.clone()).collect(),
        from_me: mine.iter().any(|a| a.eq_ignore_ascii_case(&m.from_addr)),
    }
}

struct Loaded {
    tid: i64,
    rows: Vec<MessageRow>,
    mails: Vec<MailForAi>,
    account: String,
    mine: Vec<String>,
}

impl Loaded {
    fn newest_id(&self) -> i64 {
        self.rows.last().map(|r| r.id).unwrap_or(0)
    }
    /// All the user's addresses ("Name <address>, ..."), the conversation's own account first: a
    /// message from any of them is the user's.
    fn me(&self) -> String {
        let mut accounts = engine::core().store().accounts().unwrap_or_default();
        accounts.sort_by_key(|a| !a.email.eq_ignore_ascii_case(&self.account));
        let list: Vec<String> = accounts.iter().map(|a| who(&Some(a.name.clone()), &a.email)).collect();
        if list.is_empty() {
            self.account.clone()
        } else {
            list.join(", ")
        }
    }
}

/// The conversation of `message_id` (its newest `limit` messages as prompt input); fails for an
/// excluded account or when there is nothing readable yet.
fn load_thread(message_id: i64, limit: usize) -> Result<Loaded, String> {
    let store = engine::core().store();
    let (tid, _) = store.message(message_id).ok().flatten().ok_or("This message is no longer available.")?;
    let rows = store.thread_messages(tid).map_err(|e| e.to_string())?;
    let account = rows.last().map(|r| r.account.clone()).unwrap_or_default();
    guard_account(&account)?;
    let mine: Vec<String> = store.accounts().unwrap_or_default().into_iter().map(|a| a.email).collect();
    let skip = rows.len().saturating_sub(limit);
    let mails: Vec<MailForAi> = rows[skip..].iter().map(|r| mail_of(r, &mine)).collect();
    if mails.iter().all(|m| m.body.trim().is_empty()) {
        return Err("The message is still downloading. Try again in a moment.".into());
    }
    Ok(Loaded { tid, rows, mails, account, mine })
}

fn arabic_share(text: &str) -> f32 {
    let letters = text.chars().filter(|c| c.is_alphabetic()).count();
    if letters == 0 {
        return 0.0;
    }
    text.chars().filter(|c| ('\u{0600}'..='\u{06FF}').contains(c)).count() as f32 / letters as f32
}

/// Where a translation goes when the user did not say: the language setting, else the other one of
/// English / Arabic.
fn auto_target(text: &str) -> String {
    match ai::lang() {
        Lang::Named(l) => l,
        Lang::Auto if arabic_share(text) > 0.3 => "English".into(),
        Lang::Auto => "Arabic".into(),
    }
}

fn cache_key(feature: &str, l: &Loaded, extra: &str) -> String {
    format!("{feature}|{}|{}|{:?}|{extra}", l.tid, l.newest_id(), ai::lang())
}

fn empty_answer() -> String {
    "The assistant returned an empty answer. Try again.".into()
}

fn unreadable() -> String {
    "The assistant's answer could not be understood. Try again.".into()
}

// ---- the features ------------------------------------------------------------------------------------

/// The result of a plain-text feature: `{feature, text}`.
fn text_finish(feature: &'static str) -> Finish {
    Box::new(move |answer| {
        let text = fc::parse_text(answer);
        if text.is_empty() {
            return Err(empty_answer());
        }
        Ok(json!({ "feature": feature, "text": text }))
    })
}

fn text_job(account: String, req: ChatRequest, feature: &'static str) -> Prep {
    Box::new(move || {
        guard_account(&account)?;
        Ok(Prepared { req, stream: true, cache_key: None, finish: text_finish(feature) })
    })
}

impl qobject::Assistant {
    fn available(&self) -> bool {
        ai::available()
    }

    fn feature_on(&self, key: &QString) -> bool {
        ai::available() && ai::feature_on(&key.to_string())
    }

    fn cancel(&self, id: i64) {
        if let Some(flag) = lock(cancels()).get(&id) {
            flag.store(true, Ordering::SeqCst);
        }
    }

    fn proofread(self: Pin<&mut Self>, text: &QString, account: &QString) -> i64 {
        let (original, account) = (text.to_string().trim().to_string(), account.to_string());
        let req = fc::proofread(&original);
        launch(
            self.qt_thread(),
            COMPOSE,
            Box::new(move || {
                guard_account(&account)?;
                Ok(Prepared {
                    req,
                    stream: true,
                    cache_key: None,
                    finish: Box::new(move |answer| {
                        let fixed = fc::parse_text(answer);
                        if fixed.is_empty() {
                            return Err(empty_answer());
                        }
                        let diff = fc::word_diff(&original, &fixed);
                        let changed = diff.iter().any(|d| !matches!(d, fc::Diff::Same(_)));
                        Ok(
                            json!({ "feature": "proofread", "text": fixed, "diffHtml": fc::diff_html(&diff), "changed": changed }),
                        )
                    }),
                })
            }),
        )
    }

    fn rewrite(self: Pin<&mut Self>, text: &QString, style: &QString, account: &QString) -> i64 {
        let style = fc::Style::from_id(&style.to_string());
        let (text, account) = (text.to_string(), account.to_string());
        let qt = self.qt_thread();
        match style {
            Some(s) => launch(qt, COMPOSE, text_job(account, fc::rewrite(&text, s), "rewrite")),
            None => launch(qt, COMPOSE, Box::new(|| Err("Unknown rewrite style.".into()))),
        }
    }

    fn translate_draft(self: Pin<&mut Self>, text: &QString, target: &QString, account: &QString) -> i64 {
        let req = fc::translate(&text.to_string(), &target.to_string());
        launch(self.qt_thread(), COMPOSE, text_job(account.to_string(), req, "translate"))
    }

    fn write_for_me(self: Pin<&mut Self>, instruction: &QString, source_message_id: i64, account: &QString) -> i64 {
        let (instruction, account) = (instruction.to_string(), account.to_string());
        launch(
            self.qt_thread(),
            COMPOSE,
            Box::new(move || {
                guard_account(&account)?;
                // the conversation being answered (if it cannot be read, the email is written without it)
                let thread = if source_message_id > 0 {
                    load_thread(source_message_id, 8).map(|l| l.mails).unwrap_or_default()
                } else {
                    vec![]
                };
                let sender =
                    engine::core().store().account(&account).ok().flatten().map(|a| a.name).unwrap_or_default();
                let req = fc::write_for_me(&instruction, &thread, &sender, &ai::lang());
                Ok(Prepared { req, stream: true, cache_key: None, finish: text_finish("write") })
            }),
        )
    }

    fn subject_ideas(self: Pin<&mut Self>, text: &QString, account: &QString) -> i64 {
        let req = fc::subject_ideas(&text.to_string(), &ai::lang());
        let account = account.to_string();
        launch(
            self.qt_thread(),
            COMPOSE,
            Box::new(move || {
                guard_account(&account)?;
                Ok(Prepared {
                    req,
                    stream: false,
                    cache_key: None,
                    finish: Box::new(|answer| {
                        let subjects = fc::parse_subject_ideas(answer);
                        if subjects.is_empty() {
                            return Err(unreadable());
                        }
                        Ok(json!({ "feature": "subject", "subjects": subjects }))
                    }),
                })
            }),
        )
    }

    fn summarize(self: Pin<&mut Self>, message_id: i64) -> i64 {
        launch(
            self.qt_thread(),
            READ_OR_SUMMARY,
            Box::new(move || {
                let l = load_thread(message_id, THREAD_MESSAGES)?;
                let req = fr::summarize_thread(&l.mails, &l.me(), &ai::lang());
                Ok(Prepared {
                    req,
                    stream: false,
                    cache_key: Some(cache_key("summary", &l, "")),
                    finish: Box::new(|answer| {
                        let s = fr::parse_summary(answer).ok_or_else(unreadable)?;
                        let mut v = serde_json::to_value(&s).map_err(|e| e.to_string())?;
                        v["feature"] = json!("summary");
                        Ok(v)
                    }),
                })
            }),
        )
    }

    fn translate_message(self: Pin<&mut Self>, message_id: i64, target: &QString) -> i64 {
        let target = target.to_string();
        launch(
            self.qt_thread(),
            READ,
            Box::new(move || {
                let l = load_thread(message_id, 1)?;
                let msg = l.mails.last().cloned().ok_or("Nothing to translate.")?;
                let target = if target.trim().is_empty() { auto_target(&msg.body) } else { target.trim().to_string() };
                let req = fr::translate_message(&msg, &target);
                let (mid, t) = (msg.id, target.clone());
                Ok(Prepared {
                    req,
                    stream: true,
                    cache_key: Some(cache_key("translate", &l, &target)),
                    finish: Box::new(move |answer| {
                        let text = fr::parse_text(answer);
                        if text.is_empty() {
                            return Err(empty_answer());
                        }
                        Ok(json!({ "feature": "translate", "text": text, "target": t, "messageId": mid }))
                    }),
                })
            }),
        )
    }

    fn smart_replies(self: Pin<&mut Self>, message_id: i64) -> i64 {
        launch(
            self.qt_thread(),
            READ_OR_REPLIES,
            Box::new(move || {
                let l = load_thread(message_id, THREAD_MESSAGES)?;
                let req = fr::smart_replies(&l.mails, &l.me(), &ai::lang());
                Ok(Prepared {
                    req,
                    stream: false,
                    cache_key: Some(cache_key("replies", &l, "")),
                    finish: Box::new(|answer| {
                        let replies = fr::parse_smart_replies(answer);
                        if replies.is_empty() {
                            return Err(unreadable());
                        }
                        Ok(json!({ "feature": "replies", "replies": replies }))
                    }),
                })
            }),
        )
    }

    fn phishing(self: Pin<&mut Self>, message_id: i64) -> i64 {
        launch(
            self.qt_thread(),
            READ_OR_PHISHING,
            Box::new(move || {
                let l = load_thread(message_id, THREAD_MESSAGES)?;
                let idx = l
                    .rows
                    .iter()
                    .rposition(|r| !l.mine.iter().any(|a| a.eq_ignore_ascii_case(&r.from_addr)))
                    .ok_or("This conversation only has messages from you.")?;
                let row = &l.rows[idx];
                let msg = mail_of(row, &l.mine);
                let evidence = fr::PhishingEvidence {
                    from: msg.from.clone(),
                    reply_to: mailbox_list(&row.reply_to).join(", "),
                    // not stored with the message: the check says so instead of guessing
                    return_path: String::new(),
                    authentication_results: String::new(),
                    links: row.body_html.as_deref().map(ai_support::html_links).unwrap_or_default(),
                };
                let findings = fr::evidence_findings(&msg, &evidence);
                let req = fr::phishing_check(&msg, &evidence, &ai::lang());
                let mid = row.id;
                let mut key = cache_key("phishing", &l, "");
                key.push_str(&mid.to_string());
                Ok(Prepared {
                    req,
                    stream: false,
                    cache_key: Some(key),
                    finish: Box::new(move |answer| {
                        let v = fr::parse_phishing(answer).ok_or_else(unreadable)?;
                        let v = fr::apply_floor(v, &findings);
                        Ok(json!({ "feature": "phishing", "risk": v.risk, "reasons": v.reasons, "messageId": mid }))
                    }),
                })
            }),
        )
    }

    fn summarize_attachment(self: Pin<&mut Self>, attachment_id: i64) -> i64 {
        launch(
            self.qt_thread(),
            READ,
            Box::new(move || {
                let store = engine::core().store();
                let (account, _folder, _uid, _part, name, mime, _cached, mid) = store
                    .attachment_location(attachment_id)
                    .ok()
                    .flatten()
                    .ok_or("This attachment is no longer available.")?;
                guard_account(&account)?;
                if !extract::supported(&mime, &name) {
                    return Err(format!("{name} is a type of file that cannot be summarized."));
                }
                let path = engine::runtime().block_on(engine::core().attachment_file(attachment_id)).map_err(|e| {
                    let m = format!("{e:#}");
                    if m == "offline" {
                        "The attachment is not downloaded yet and you are offline.".to_string()
                    } else {
                        format!("Could not get {name}: {m}")
                    }
                })?;
                let text = extract::text_of(&path, &mime, fr::MAX_ATTACHMENT_CHARS)
                    .ok_or_else(|| format!("No readable text in {name} (a scanned document or an empty file)."))?;
                let req = fr::summarize_attachment(&name, &text, &ai::lang());
                let key = format!("attachment|{attachment_id}|{mid}|{:?}", ai::lang());
                let n = name.clone();
                Ok(Prepared {
                    req,
                    stream: true,
                    cache_key: Some(key),
                    finish: Box::new(move |answer| {
                        let text = fr::parse_text(answer);
                        if text.is_empty() {
                            return Err(empty_answer());
                        }
                        Ok(json!({ "feature": "attachment", "text": text, "name": n, "attachmentId": attachment_id }))
                    }),
                })
            }),
        )
    }

    fn sender_is_new(&self, message_id: i64) -> bool {
        let store = engine::core().store();
        let Some((tid, _)) = store.message(message_id).ok().flatten() else { return false };
        let rows = store.thread_messages(tid).unwrap_or_default();
        let mine: Vec<String> = store.accounts().unwrap_or_default().into_iter().map(|a| a.email).collect();
        let Some(row) = rows.iter().rev().find(|r| !mine.iter().any(|a| a.eq_ignore_ascii_case(&r.from_addr))) else {
            return false;
        };
        let stats = store.contact_stats(&row.from_addr).unwrap_or_default();
        let in_thread = rows.iter().filter(|r| r.from_addr.eq_ignore_ascii_case(&row.from_addr)).count() as i64;
        stats.to_them == 0 && stats.from_them <= in_thread
    }

    fn attachment_supported(&self, mime: &QString, name: &QString) -> bool {
        extract::supported(&mime.to_string(), &name.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dates_are_formatted() {
        assert_eq!(date_text(0), "1970-01-01 00:00");
        assert_eq!(date_text(1_760_000_000_000), "2025-10-09 08:53");
        assert_eq!(date_text(951_782_400_000 + 3_600_000 * 13 + 60_000 * 5), "2000-02-29 13:05");
    }

    #[test]
    fn the_users_addresses_are_marked() {
        let v =
            json!([{"name": "Hamza", "mailbox": "h@b.com"}, {"name": null, "mailbox": "Karem@A.ai"}, {"mailbox": ""}]);
        let mine = vec!["karem@a.ai".to_string()];
        assert_eq!(mark_mine(&v, &mine), "Hamza <h@b.com>, Karem@A.ai [THE USER]");
        assert_eq!(mark_mine(&v, &[]), "Hamza <h@b.com>, Karem@A.ai");
    }

    #[test]
    fn mailbox_lists() {
        let v = json!([{"name": "Dana", "mailbox": "d@x.org"}, {"name": null, "mailbox": "e@x.org"}, {"mailbox": ""}]);
        assert_eq!(mailbox_list(&v), vec!["Dana <d@x.org>", "e@x.org"]);
        assert!(mailbox_list(&json!(null)).is_empty());
    }

    #[test]
    fn messages_become_prompt_input() {
        let row = MessageRow {
            id: 7,
            from_name: Some("Dana".into()),
            from_addr: "d@x.org".into(),
            to: json!([{"name": null, "mailbox": "me@x.org"}]),
            subject: "Plan".into(),
            date_ms: 1_760_000_000_000,
            body_text: Some(
                "Friday works.\n\nOn Mon, 5 Oct 2026 at 10:00, Me <me@x.org> wrote:\n> Can we meet?".into(),
            ),
            attachments: vec![
                (1, "a.pdf".into(), "application/pdf".into(), 10, false),
                (2, "i.png".into(), "image/png".into(), 5, true),
            ],
            ..Default::default()
        };
        let m = mail_of(&row, &["me@x.org".to_string()]);
        assert_eq!((m.id, m.from.as_str(), m.to.as_str()), (7, "Dana <d@x.org>", "me@x.org [THE USER]"));
        assert_eq!(m.body, "Friday works.");
        assert_eq!(m.attachments, vec!["a.pdf"]);
        assert!(!m.from_me);
        assert!(mail_of(&row, &["D@X.org".to_string()]).from_me);
        // an html-only message is turned into text
        let html = MessageRow { body_html: Some("<p>Hello <b>there</b></p>".into()), ..Default::default() };
        assert_eq!(body_for_ai(&html), "Hello there");
        let preview = MessageRow { preview: "just a preview".into(), ..Default::default() };
        assert_eq!(body_for_ai(&preview), "just a preview");
    }

    #[test]
    fn translation_target_follows_the_text() {
        assert!(arabic_share("مرحبا بكم") > 0.9);
        assert!(arabic_share("hello") < 0.01);
        assert_eq!(arabic_share("123"), 0.0);
    }
}
