mod bridge;
mod engine;
mod settings;
mod theme;

/// Product name shown in the window title and notifications (rename here).
pub const APP_NAME: &str = "Rusty Inbox";
/// Desktop entry / Wayland app id (`rusty-inbox.desktop`, window class `rusty-inbox`).
pub const APP_ID: &str = "rusty-inbox";

use cxx_qt_lib::{QFont, QGuiApplication, QQmlApplicationEngine, QString, QUrl};

fn main() {
    let filter = std::env::var("RUSTY_INBOX_LOG").unwrap_or_else(|_| "info".into());
    let _ = tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).try_init();

    // scripted test runs perform real actions (done, trash...): never let them reach a mail server
    if std::env::args().any(|a| a.starts_with("--autotest") || a == "--stress") {
        std::env::set_var("RUSTY_INBOX_OFFLINE", "1");
    }

    // headless setup: `rusty-inbox --add-account=account.json` where the JSON has email, name and
    // either password or passwordFile (a file holding only the password), plus optional servers
    if let Some(path) = std::env::args().find_map(|a| a.strip_prefix("--add-account=").map(str::to_string)) {
        std::process::exit(add_account_cli(&path));
    }

    // must precede QGuiApplication (stock QtWebEngine requirement)
    bridge::rustyinbox_init_webengine();

    let mut app = QGuiApplication::new();
    if let Some(mut a) = app.as_mut() {
        a.as_mut().set_application_name(&QString::from(APP_NAME));
        a.as_mut().set_application_display_name(&QString::from(APP_NAME));
        QGuiApplication::set_desktop_file_name(&QString::from(APP_ID));
        // the system font (Omarchy: `omarchy font current`) for every Text, not only Controls
        if let Some(family) = theme::current()["font"].as_str() {
            let mut f: QFont = a.font();
            f.set_family(&QString::from(family));
            a.as_mut().set_application_font(&f);
        }
    }
    let mut qml = QQmlApplicationEngine::new();
    if let Some(qml) = qml.as_mut() {
        qml.load(&QUrl::from("qrc:/qt/qml/org/rustyinbox/qml/Main.qml"));
    }
    if let Some(app) = app.as_mut() {
        app.exec();
    }
}

fn add_account_cli(path: &str) -> i32 {
    let req = std::fs::read_to_string(path)
        .map_err(|e| e.to_string())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).map_err(|e| e.to_string()));
    let mut req = match req {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{path}: {e}");
            return 2;
        }
    };
    if let Some(f) = req["passwordFile"].as_str().map(str::to_string) {
        let f = f.strip_prefix("~/").map(|r| format!("{}/{r}", std::env::var("HOME").unwrap_or_default())).unwrap_or(f);
        match std::fs::read_to_string(&f) {
            Ok(p) => req["password"] = serde_json::Value::String(p.trim_end_matches(['\n', '\r']).to_string()),
            Err(e) => {
                eprintln!("passwordFile: {e}");
                return 2;
            }
        }
    }
    match engine::runtime().block_on(engine::add_account(req)) {
        Ok(email) => {
            println!("added {email}");
            0
        }
        Err(e) => {
            eprintln!("could not add the account: {e}");
            1
        }
    }
}
