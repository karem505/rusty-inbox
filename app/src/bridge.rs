//! QML-facing backend object. One `Backend` instance lives in QML; it owns no state itself:
//! the engine is a process-wide singleton (see `crate::engine`). Reads that hit only the local
//! store are synchronous (they are fast); anything touching the network runs on the engine's
//! tokio runtime and reports back to the Qt thread through a signal.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "C++" {
        include!("webengine_init.h");
        fn rustyinbox_init_webengine();
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type Backend = super::BackendRust;

        /// Start background sync and new-mail notifications (idempotent).
        #[qinvokable]
        fn start(self: Pin<&mut Backend>);

        /// The UI palette and font (Omarchy theme when present). See `crate::theme`.
        #[qinvokable]
        fn theme_json(self: &Backend) -> QString;

        /// `file://` URL of the status-bar icon for the current theme (with an unread dot if asked).
        #[qinvokable]
        fn tray_icon_url(self: &Backend, unread: bool) -> QString;

        /// Start a sync pass now instead of waiting for the next poll.
        #[qinvokable]
        fn sync_now(self: &Backend);

        /// All preferences as JSON (defaults overlaid with saved values).
        #[qinvokable]
        fn settings_json(self: &Backend) -> QString;

        /// Save one preference (`value_json` is a JSON value). Returns "" or an error message.
        #[qinvokable]
        fn set_setting(self: Pin<&mut Backend>, key: &QString, value_json: &QString) -> QString;

        /// Accounts with their server settings (no passwords).
        #[qinvokable]
        fn account_details_json(self: &Backend) -> QString;

        /// Verify and add an account in the background; answers with `account_added`.
        #[qinvokable]
        fn add_account(self: Pin<&mut Backend>, request_json: &QString);

        /// Accounts found in Thunderbird (JSON list, no passwords).
        #[qinvokable]
        fn thunderbird_json(self: &Backend) -> QString;

        /// Import Thunderbird accounts: `{"emails": [...], "passwords": {email: password}}`.
        /// Answers with one `import_result` per account.
        #[qinvokable]
        fn import_thunderbird(self: Pin<&mut Backend>, request_json: &QString);

        /// Remove an account and its local mail. Returns "" or an error message.
        #[qinvokable]
        fn remove_account(self: Pin<&mut Backend>, email: &QString) -> QString;

        /// Change an account's display name. Returns "" or an error message.
        #[qinvokable]
        fn rename_account(self: Pin<&mut Backend>, email: &QString, name: &QString) -> QString;

        /// The request this launch was started with (`show`, `compose`, `mailto:...`), once.
        #[qinvokable]
        fn take_request(self: &Backend) -> QString;

        /// A composer draft (JSON): kind `new` | `reply` | `replyAll` | `forward` | `draft`.
        #[qinvokable]
        fn compose_json(self: &Backend, kind: &QString, message_id: i64, account: &QString) -> QString;

        /// Address suggestions for `prefix` (JSON list of {name, address}).
        #[qinvokable]
        fn contacts_json(self: &Backend, prefix: &QString) -> QString;

        /// {name, mime, size, path} of a local file, or null.
        #[qinvokable]
        fn file_info_json(self: &Backend, url: &QString) -> QString;

        /// A person's card for an address (`engine::contact_of`): name, mail counts, conversations.
        #[qinvokable]
        fn contact_json(self: &Backend, address: &QString) -> QString;

        /// Download an attachment of received mail (once; later from the cache) and open it with
        /// the system's app; answers with `attachment_ready` (action "open", or "reveal" for files
        /// that would run code: their folder opens instead).
        #[qinvokable]
        fn open_attachment(self: Pin<&mut Backend>, attachment_id: i64);

        /// Save an attachment to `dest_url` (a file, or a folder: it keeps its name there);
        /// answers with `attachment_ready` (action "save").
        #[qinvokable]
        fn save_attachment(self: Pin<&mut Backend>, attachment_id: i64, dest_url: &QString);

        /// `file://` URL of the Downloads folder.
        #[qinvokable]
        fn downloads_url(self: &Backend) -> QString;

        /// Save a draft locally (id 0 creates it); with `upload` also to the server's Drafts.
        /// Returns the outbox id.
        #[qinvokable]
        fn save_draft(self: &Backend, id: i64, draft_json: &QString, upload: bool) -> i64;

        /// Delete a draft locally and on the server.
        #[qinvokable]
        fn discard_draft(self: &Backend, id: i64, draft_json: &QString);

        /// Queue a draft to send at `send_at_ms`. Returns {"id": n} or {"error": "..."}.
        #[qinvokable]
        fn queue_send(self: &Backend, id: i64, draft_json: &QString, send_at_ms: i64) -> QString;

        /// Take a queued message back (undo send / edit a scheduled one): its draft JSON or "".
        #[qinvokable]
        fn unqueue(self: &Backend, id: i64) -> QString;

        /// Saved templates (JSON list of {id, name, subject, html}).
        #[qinvokable]
        fn templates_json(self: &Backend) -> QString;

        /// Add or replace a template (JSON). Returns {"id": n} or {"error": "..."}.
        #[qinvokable]
        fn save_template(self: &Backend, template_json: &QString) -> QString;

        #[qinvokable]
        fn delete_template(self: &Backend, id: i64);

        /// The signature of an account.
        #[qinvokable]
        fn signature(self: &Backend, email: &QString) -> QString;

        /// Set the signature of an account. Returns "" or an error.
        #[qinvokable]
        fn set_signature(self: &Backend, email: &QString, text: &QString) -> QString;

        /// Database id of the newest message of a conversation (0 if none).
        #[qinvokable]
        fn latest_message_id(self: &Backend, thread_id: i64) -> i64;

        /// The Updates page: current version, install type, check / download state (JSON).
        #[qinvokable]
        fn update_json(self: &Backend) -> QString;

        /// Look for a new release now; the result arrives as `update_changed`.
        #[qinvokable]
        fn check_update(self: Pin<&mut Backend>);

        /// Download and apply the release found by the last check (progress via `update_changed`).
        #[qinvokable]
        fn install_update(self: Pin<&mut Backend>);

        /// Start the installed update after this process quits. False if there is nothing to start.
        #[qinvokable]
        fn restart_into_update(self: &Backend) -> bool;

        /// Ask the compositor to focus our window (Hyprland: no focus stealing without this).
        #[qinvokable]
        fn focus_native(self: &Backend);

        /// App version (from Cargo.toml).
        #[qinvokable]
        fn version(self: &Backend) -> QString;

        /// Product name (one place to rename the app).
        #[qinvokable]
        fn app_name(self: &Backend) -> QString;

        /// The system theme changed; `palette_json` is the new `theme_json`.
        #[qsignal]
        fn theme_changed(self: Pin<&mut Backend>, palette_json: QString);

        /// JSON array of `{email,name}`.
        #[qinvokable]
        fn accounts_json(self: &Backend) -> QString;

        /// Sidebar model: unread counts per list/category and each account's folders.
        #[qinvokable]
        fn sidebar_json(self: &Backend) -> QString;

        /// `spec`: a list name or `{"list","account","category","folder"}`. JSON array of thread rows.
        #[qinvokable]
        fn threads_json(self: &Backend, spec: &QString, limit: i32) -> QString;

        /// Emit `thread_loaded` with what is stored, then again once missing bodies are fetched.
        #[qinvokable]
        fn open_thread(self: Pin<&mut Backend>, thread_id: i64);

        /// Run a thread action in the background (`until_ms` for snooze, else 0); emits `action_done`.
        #[qinvokable]
        fn act(self: Pin<&mut Backend>, action: &QString, ids_json: &QString, until_ms: i64);

        /// Run a thread action and wait for it (flushes pending undoable actions on quit).
        #[qinvokable]
        fn act_now(self: &Backend, action: &QString, ids_json: &QString, until_ms: i64);

        /// Whether the action is committed only after the undo window (it moves mail out of the list).
        #[qinvokable]
        fn is_deferred(self: &Backend, action: &QString) -> bool;

        /// The action that undoes an immediate action, or "" if it is undone by cancelling.
        #[qinvokable]
        fn inverse_of(self: &Backend, action: &QString) -> QString;

        #[qsignal]
        fn thread_loaded(self: Pin<&mut Backend>, thread_id: i64, messages_json: QString);

        #[qsignal]
        fn action_done(self: Pin<&mut Backend>, action: QString);

        /// Fired after each sync pass.
        /// Result of `add_account`: `error` is empty on success.
        #[qsignal]
        fn account_added(self: Pin<&mut Backend>, email: QString, error: QString);

        /// A preference changed (JSON of all preferences).
        #[qsignal]
        fn settings_changed(self: Pin<&mut Backend>, json: QString);

        /// One Thunderbird account imported (`error` empty) or not.
        #[qsignal]
        fn import_result(self: Pin<&mut Backend>, email: QString, error: QString);

        /// A queued message was sent (`error` empty) or failed.
        #[qsignal]
        fn send_result(self: Pin<&mut Backend>, id: i64, subject: QString, error: QString);

        /// Another launch handed over a request (`show`, `compose`, `mailto:...`).
        #[qsignal]
        fn activated(self: Pin<&mut Backend>, request: QString);

        #[qsignal]
        fn mail_changed(self: Pin<&mut Backend>);

        /// The update state changed (JSON, see `update_json`).
        #[qsignal]
        fn update_changed(self: Pin<&mut Backend>, json: QString);

        /// A background check found a newer release.
        #[qsignal]
        fn update_available(self: Pin<&mut Backend>, version: QString);

        /// A new-mail notification was clicked.
        #[qsignal]
        fn open_requested(self: Pin<&mut Backend>, thread_id: i64);

        /// An attachment is on disk (`url` of the file, `folder_url` of its folder) or failed (`error`).
        #[qsignal]
        fn attachment_ready(
            self: Pin<&mut Backend>,
            attachment_id: i64,
            action: QString,
            url: QString,
            folder_url: QString,
            error: QString,
        );
    }

    impl cxx_qt::Threading for Backend {}
}

use crate::engine;
use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;

pub use qobject::rustyinbox_init_webengine;

#[derive(Default)]
pub struct BackendRust {
    started: bool,
}

impl qobject::Backend {
    fn start(mut self: Pin<&mut Self>) {
        if self.rust().started {
            return;
        }
        self.as_mut().rust_mut().started = true;
        let qt = self.qt_thread();
        let qt_notify = self.qt_thread();
        let qt_theme = self.qt_thread();
        crate::theme::watch(move |p| {
            let json = p.to_string();
            let _ = qt_theme
                .queue(move |mut obj: Pin<&mut qobject::Backend>| obj.as_mut().theme_changed(QString::from(&json)));
        });
        let mode = crate::settings::get(engine::core().store(), "themeMode");
        crate::theme::set_mode(mode.as_str().unwrap_or("system"));
        engine::on_new_mail(move |n| {
            if crate::settings::get(engine::core().store(), "notifications") != serde_json::json!(true) {
                return;
            }
            let qt = qt_notify.clone();
            std::thread::spawn(move || show_notification(n, qt));
        });
        let report = update_reporter(self.qt_thread());
        if std::env::var_os("RUSTY_INBOX_OFFLINE").is_none() {
            crate::updater::start_auto(report);
        }
        let qt_inst = self.qt_thread();
        crate::instance::listen(move |req, token| {
            let _ = qt_inst.queue(move |mut obj: Pin<&mut qobject::Backend>| {
                // Qt's Wayland backend uses this token on the next requestActivate()
                if let Some(t) = token {
                    std::env::set_var("XDG_ACTIVATION_TOKEN", t);
                }
                obj.as_mut().activated(QString::from(&req))
            });
        });
        let qt_out = self.qt_thread();
        engine::on_outbox(move |id, subject, result| {
            let err = result.err().unwrap_or_default();
            let _ = qt_out.queue(move |mut obj: Pin<&mut qobject::Backend>| {
                obj.as_mut().send_result(id, QString::from(&subject), QString::from(&err));
                obj.as_mut().mail_changed();
            });
        });
        engine::on_synced(move || {
            let _ = qt.queue(|mut obj: Pin<&mut qobject::Backend>| obj.as_mut().mail_changed());
            crate::ai_inbox::wake(); // AI(C2): new mail may need a label
        });
    }

    fn theme_json(&self) -> QString {
        QString::from(&crate::theme::current().to_string())
    }

    fn tray_icon_url(&self, unread: bool) -> QString {
        let url = crate::theme::tray_icon_file(unread).map(|p| format!("file://{}", p.display())).unwrap_or_default();
        QString::from(&url)
    }

    fn sync_now(&self) {
        engine::sync_now();
    }

    fn settings_json(&self) -> QString {
        QString::from(&crate::settings::all(engine::core().store()).to_string())
    }

    fn set_setting(mut self: Pin<&mut Self>, key: &QString, value_json: &QString) -> QString {
        let key = key.to_string();
        let value: serde_json::Value = match serde_json::from_str(&value_json.to_string()) {
            Ok(v) => v,
            Err(e) => return QString::from(&format!("bad value: {e}")),
        };
        if let Err(e) = crate::settings::set(engine::core().store(), &key, &value) {
            return QString::from(&e);
        }
        match key.as_str() {
            "themeMode" => {
                crate::theme::set_mode(value.as_str().unwrap_or("system"));
                let json = crate::theme::current().to_string();
                self.as_mut().theme_changed(QString::from(&json));
            }
            "syncMinutes" => engine::sync_now(),
            _ => {}
        }
        let all = crate::settings::all(engine::core().store()).to_string();
        self.as_mut().settings_changed(QString::from(&all));
        QString::default()
    }

    fn account_details_json(&self) -> QString {
        QString::from(&engine::account_details(engine::core().store()).to_string())
    }

    fn add_account(self: Pin<&mut Self>, request_json: &QString) {
        let qt = self.qt_thread();
        let req: serde_json::Value = serde_json::from_str(&request_json.to_string()).unwrap_or_default();
        engine::runtime().spawn(async move {
            let (email, error) = match engine::add_account(req).await {
                Ok(e) => (e, String::new()),
                Err(e) => (String::new(), e),
            };
            let _ = qt.queue(move |mut obj: Pin<&mut qobject::Backend>| {
                obj.as_mut().account_added(QString::from(&email), QString::from(&error));
                obj.as_mut().mail_changed();
            });
        });
    }

    fn thunderbird_json(&self) -> QString {
        QString::from(&engine::thunderbird_of(engine::core().store()).to_string())
    }

    fn import_thunderbird(self: Pin<&mut Self>, request_json: &QString) {
        let qt = self.qt_thread();
        let req: serde_json::Value = serde_json::from_str(&request_json.to_string()).unwrap_or_default();
        let emails: Vec<String> = req["emails"]
            .as_array()
            .map(|a| a.iter().filter_map(|e| e.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let passwords: std::collections::HashMap<String, String> = req["passwords"]
            .as_object()
            .map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
            .unwrap_or_default();
        engine::runtime().spawn(async move {
            engine::import_thunderbird(emails, passwords, move |email, r| {
                let err = r.err().unwrap_or_default();
                let _ = qt.queue(move |mut obj: Pin<&mut qobject::Backend>| {
                    obj.as_mut().import_result(QString::from(&email), QString::from(&err));
                    obj.as_mut().mail_changed();
                });
            })
            .await;
        });
    }

    fn remove_account(mut self: Pin<&mut Self>, email: &QString) -> QString {
        match engine::remove_account(&email.to_string()) {
            Ok(()) => {
                self.as_mut().mail_changed();
                QString::default()
            }
            Err(e) => QString::from(&e),
        }
    }

    fn rename_account(mut self: Pin<&mut Self>, email: &QString, name: &QString) -> QString {
        match engine::rename_account(&email.to_string(), &name.to_string()) {
            Ok(()) => {
                self.as_mut().mail_changed();
                QString::default()
            }
            Err(e) => QString::from(&e),
        }
    }

    fn take_request(&self) -> QString {
        QString::from(&INITIAL_REQUEST.lock().map(|mut r| std::mem::take(&mut *r)).unwrap_or_default())
    }

    fn compose_json(&self, kind: &QString, message_id: i64, account: &QString) -> QString {
        let v = engine::compose_of(engine::core().store(), &kind.to_string(), message_id, &account.to_string());
        QString::from(&v.to_string())
    }

    fn contacts_json(&self, prefix: &QString) -> QString {
        QString::from(&engine::contacts_of(engine::core().store(), &prefix.to_string()).to_string())
    }

    fn file_info_json(&self, url: &QString) -> QString {
        QString::from(&engine::file_info(&url.to_string()).to_string())
    }

    fn contact_json(&self, address: &QString) -> QString {
        QString::from(&engine::contact_of(engine::core().store(), &address.to_string(), 30).to_string())
    }

    fn open_attachment(self: Pin<&mut Self>, attachment_id: i64) {
        let qt = self.qt_thread();
        engine::runtime().spawn(async move {
            let r = engine::core().attachment_file(attachment_id).await.map_err(|e| format!("{e:#}"));
            let action = match &r {
                Ok(p) if engine::runs_code(&p.to_string_lossy()) => "reveal",
                _ => "open",
            };
            report_attachment(qt, attachment_id, action, r);
        });
    }

    fn save_attachment(self: Pin<&mut Self>, attachment_id: i64, dest_url: &QString) {
        let qt = self.qt_thread();
        let dest = engine::path_of_url(&dest_url.to_string());
        engine::runtime().spawn(async move {
            let r = engine::core().save_attachment(attachment_id, &dest).await.map_err(|e| format!("{e:#}"));
            report_attachment(qt, attachment_id, "save", r);
        });
    }

    fn downloads_url(&self) -> QString {
        QString::from(&engine::url_of_path(&engine::downloads_dir()))
    }

    fn save_draft(&self, id: i64, draft_json: &QString, upload: bool) -> i64 {
        let d = draft_of(draft_json);
        let id = engine::core().save_draft(id, &d).unwrap_or(id);
        if upload {
            engine::runtime().spawn(async move {
                if engine::core().upload_draft(&d).await {
                    tracing::info!("draft saved to the server");
                }
            });
        }
        id
    }

    fn discard_draft(&self, id: i64, draft_json: &QString) {
        let d = draft_of(draft_json);
        engine::runtime().spawn(async move { engine::core().discard_draft(id, &d).await });
    }

    fn queue_send(&self, id: i64, draft_json: &QString, send_at_ms: i64) -> QString {
        let d = draft_of(draft_json);
        let v = match engine::core().queue(id, &d, send_at_ms) {
            Ok(id) => {
                engine::wake_outbox();
                serde_json::json!({ "id": id })
            }
            Err(e) => serde_json::json!({ "error": e.to_string() }),
        };
        QString::from(&v.to_string())
    }

    fn unqueue(&self, id: i64) -> QString {
        let v = engine::core().unqueue(id).map(|d| d.to_json().to_string()).unwrap_or_default();
        QString::from(&v)
    }

    fn templates_json(&self) -> QString {
        QString::from(&crate::settings::templates(engine::core().store()).to_string())
    }

    fn save_template(&self, template_json: &QString) -> QString {
        let t: serde_json::Value = serde_json::from_str(&template_json.to_string()).unwrap_or_default();
        let v = match crate::settings::save_template(engine::core().store(), &t) {
            Ok(id) => serde_json::json!({ "id": id }),
            Err(e) => serde_json::json!({ "error": e }),
        };
        QString::from(&v.to_string())
    }

    fn delete_template(&self, id: i64) {
        let _ = crate::settings::delete_template(engine::core().store(), id);
    }

    fn signature(&self, email: &QString) -> QString {
        QString::from(&crate::settings::signature(engine::core().store(), &email.to_string()))
    }

    fn set_signature(&self, email: &QString, text: &QString) -> QString {
        QString::from(
            &crate::settings::set_signature(engine::core().store(), &email.to_string(), &text.to_string())
                .err()
                .unwrap_or_default(),
        )
    }

    fn latest_message_id(&self, thread_id: i64) -> i64 {
        engine::core().store().thread_messages(thread_id).ok().and_then(|m| m.last().map(|m| m.id)).unwrap_or(0)
    }

    fn update_json(&self) -> QString {
        QString::from(&crate::updater::status_json().to_string())
    }

    fn check_update(self: Pin<&mut Self>) {
        let report = update_reporter(self.qt_thread());
        std::thread::spawn(move || {
            crate::updater::check(&|| report(false));
        });
    }

    fn install_update(self: Pin<&mut Self>) {
        let report = update_reporter(self.qt_thread());
        std::thread::spawn(move || crate::updater::install_latest(&|| report(false)));
    }

    fn restart_into_update(&self) -> bool {
        crate::updater::restart()
    }

    fn focus_native(&self) {
        crate::platform::focus_own_window();
    }

    fn version(&self) -> QString {
        QString::from(env!("CARGO_PKG_VERSION"))
    }

    fn app_name(&self) -> QString {
        QString::from(crate::APP_NAME)
    }

    fn accounts_json(&self) -> QString {
        QString::from(&engine::accounts_of(engine::core().store()).to_string())
    }

    fn sidebar_json(&self) -> QString {
        let t = std::time::Instant::now();
        let s = engine::sidebar_json().to_string();
        tracing::debug!("sidebar_json {:?}", t.elapsed());
        QString::from(&s)
    }

    fn threads_json(&self, spec: &QString, limit: i32) -> QString {
        let t = std::time::Instant::now();
        let v = engine::threads_json(&spec.to_string(), limit.max(1) as usize);
        tracing::debug!("threads_json {:?}", t.elapsed());
        QString::from(&v.to_string())
    }

    fn open_thread(self: Pin<&mut Self>, thread_id: i64) {
        let qt = self.qt_thread();
        // what is stored shows at once, without waiting for the network engine
        let json = engine::messages_json(thread_id).to_string();
        let _ = qt.queue(move |mut obj: Pin<&mut qobject::Backend>| {
            obj.as_mut().thread_loaded(thread_id, QString::from(&json))
        });
        engine::runtime().spawn(async move {
            if engine::core().fetch_missing_bodies(thread_id).await {
                let json = engine::messages_json(thread_id).to_string();
                let _ = qt.queue(move |mut obj: Pin<&mut qobject::Backend>| {
                    obj.as_mut().thread_loaded(thread_id, QString::from(&json))
                });
            }
        });
    }

    fn act(self: Pin<&mut Self>, action: &QString, ids_json: &QString, until_ms: i64) {
        let qt = self.qt_thread();
        let action = action.to_string();
        let ids = engine::ids_of(&ids_json.to_string());
        let until = (until_ms > 0).then_some(until_ms);
        engine::runtime().spawn(async move {
            if !engine::core().thread_action(&action, &ids, until).await {
                tracing::warn!("unknown action {action}");
            }
            let _ =
                qt.queue(move |mut obj: Pin<&mut qobject::Backend>| obj.as_mut().action_done(QString::from(&action)));
        });
    }

    fn act_now(&self, action: &QString, ids_json: &QString, until_ms: i64) {
        let action = action.to_string();
        let ids = engine::ids_of(&ids_json.to_string());
        let until = (until_ms > 0).then_some(until_ms);
        engine::runtime().block_on(engine::core().thread_action(&action, &ids, until));
    }

    fn is_deferred(&self, action: &QString) -> bool {
        engine::is_deferred(&action.to_string())
    }

    fn inverse_of(&self, action: &QString) -> QString {
        QString::from(engine::inverse_of(&action.to_string()).unwrap_or(""))
    }
}

static INITIAL_REQUEST: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// Remember what this launch asked for; QML takes it once the window is up.
pub fn set_initial_request(r: &str) {
    if let Ok(mut g) = INITIAL_REQUEST.lock() {
        *g = r.to_string();
    }
}

fn draft_of(json: &QString) -> rusty_inbox_core::compose::Draft {
    let v: serde_json::Value = serde_json::from_str(&json.to_string()).unwrap_or_default();
    rusty_inbox_core::compose::Draft::from_json(&v)
}

/// Hands the outcome of an attachment download to QML.
fn report_attachment(
    qt: cxx_qt::CxxQtThread<qobject::Backend>,
    attachment_id: i64,
    action: &'static str,
    result: Result<std::path::PathBuf, String>,
) {
    let (url, folder, err) = match result {
        Ok(p) => {
            let folder = p.parent().map(engine::url_of_path).unwrap_or_default();
            (engine::url_of_path(&p), folder, String::new())
        }
        Err(e) => {
            tracing::warn!("attachment {attachment_id}: {e}");
            (String::new(), String::new(), e)
        }
    };
    let _ = qt.queue(move |mut obj: Pin<&mut qobject::Backend>| {
        obj.as_mut().attachment_ready(
            attachment_id,
            QString::from(action),
            QString::from(&url),
            QString::from(&folder),
            QString::from(&err),
        )
    });
}

/// Sends the update state to QML: `update_changed` always, `update_available` when `newly` found.
fn update_reporter(qt: cxx_qt::CxxQtThread<qobject::Backend>) -> impl Fn(bool) + Send + Sync + 'static {
    move |newly| {
        let st = crate::updater::status_json();
        let json = st.to_string();
        let version = st["latest"]["version"].as_str().unwrap_or_default().to_string();
        let _ = qt.queue(move |mut obj: Pin<&mut qobject::Backend>| {
            obj.as_mut().update_changed(QString::from(&json));
            if newly {
                obj.as_mut().update_available(QString::from(&version));
            }
        });
    }
}

/// Show one desktop notification and wait (on this helper thread) for a click.
fn show_notification(n: engine::NewMail, qt: cxx_qt::CxxQtThread<qobject::Backend>) {
    let preview = crate::settings::get(engine::core().store(), "notificationPreview") == serde_json::json!(true);
    let body = if !preview {
        "New message".to_string()
    } else if n.subtitle.is_empty() {
        n.body.clone()
    } else {
        format!("{}\n{}", n.subtitle, n.body)
    };
    let shown = notify_rust::Notification::new()
        .appname(crate::APP_NAME)
        .summary(&n.title)
        .body(&body)
        .icon(crate::APP_ID)
        .action("default", "Open")
        .show();
    match shown {
        #[cfg(all(unix, not(target_os = "macos")))]
        Ok(handle) => {
            let tid = n.thread_id;
            handle.wait_for_action(move |a| {
                if a == "default" {
                    let _ = qt.queue(move |mut obj: Pin<&mut qobject::Backend>| obj.as_mut().open_requested(tid));
                }
            });
        }
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        Ok(_) => {}
        Err(e) => tracing::warn!("notification: {e}"),
    }
}
