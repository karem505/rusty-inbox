use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

const ICONS: [&str; 50] = [
    "account",
    "archive",
    "back",
    "bell",
    "calendar",
    "checkbox",
    "chevron",
    "chevron-down",
    "clear-format",
    "clock",
    "close",
    "collapse",
    "compose",
    "copy",
    "done",
    "download",
    "drafts",
    "expand",
    "eye",
    "eye-off",
    "folder",
    "forward",
    "inbox",
    "info",
    "keyboard",
    "link",
    "list-bullet",
    "list-number",
    "mail",
    "news",
    "palette",
    "paperclip",
    "people",
    "pin",
    "plus",
    "quote",
    "reply",
    "reply-all",
    "select",
    "send",
    "sent",
    "settings",
    "signature",
    "spam",
    "template",
    "trash",
    "undo",
    "sparkle",
    "unread",
    "update",
];

fn main() {
    // Windows: embed the application icon into the .exe
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        #[cfg(windows)]
        embed_resource::compile("packaging/windows/rusty-inbox.rc", embed_resource::NONE).manifest_required().unwrap();
    }
    let mut icons: Vec<String> = ICONS.iter().map(|n| format!("icons/{n}.svg")).collect();
    icons.push("icons/app.png".into());
    CxxQtBuilder::new_qml_module(QmlModule::new("org.rustyinbox").qml_files([
        QmlFile::from("qml/Theme.qml").singleton(true),
        "qml/Main.qml".into(),
        "qml/Sidebar.qml".into(),
        "qml/ThreadList.qml".into(),
        "qml/Reader.qml".into(),
        "qml/IconButton.qml".into(),
        "qml/SnoozeMenu.qml".into(),
        "qml/Toast.qml".into(),
        "qml/Settings.qml".into(),
        "qml/RecipientField.qml".into(),
        "qml/Composer.qml".into(),
        "qml/TickBox.qml".into(),
        "qml/ContactCard.qml".into(),
        // AI (docs/ai/DESIGN.md): B = AiSettingsPage, C1 = AiComposerTools + AiReaderPanel, C2 = AiBriefing
        "qml/AiSettingsPage.qml".into(),
        "qml/AiComposerTools.qml".into(),
        "qml/AiReaderPanel.qml".into(),
        "qml/AiBriefing.qml".into(),
        "qml/AiButton.qml".into(),
        "qml/AiDots.qml".into(),
        "qml/AiPill.qml".into(),
        "qml/AiSubjectChips.qml".into(),
    ]))
    .qrc_resources(icons)
    .qt_module("Quick")
    .qt_module("QuickControls2")
    .qt_module("WebEngineQuick")
    .files(["src/bridge.rs", "src/ai_settings.rs", "src/assistant.rs", "src/ai_inbox.rs"])
    .cpp_file("cpp/webengine_init.cpp")
    .include_dir("cpp")
    .build();
}
