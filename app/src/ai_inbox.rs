//! QML object + background jobs for features 11-15: labels, needs reply, follow-ups, plain-words search, briefing.
//! WORKER C2 owns this file. See docs/ai/DESIGN.md for the contract.

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
    }
}

#[derive(Default)]
pub struct AiInboxRust {}

impl qobject::AiInbox {
    fn available(&self) -> bool {
        crate::ai::available()
    }
}
