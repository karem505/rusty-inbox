//! QML object behind Settings → AI: providers, keys, ChatGPT sign-in, live model list, thinking levels.
//! WORKER B owns this file. See docs/ai/DESIGN.md for the contract.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type AiSettings = super::AiSettingsRust;

        /// AI is configured and on.
        #[qinvokable]
        fn available(self: &AiSettings) -> bool;
    }
}

#[derive(Default)]
pub struct AiSettingsRust {}

impl qobject::AiSettings {
    fn available(&self) -> bool {
        crate::ai::available()
    }
}
