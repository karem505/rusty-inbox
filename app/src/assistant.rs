//! QML object for the composer and reader AI tools (features 1-10, 16): streamed requests with ids.
//! WORKER C1 owns this file. See docs/ai/DESIGN.md for the contract.

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
    }
}

#[derive(Default)]
pub struct AssistantRust {}

impl qobject::Assistant {
    fn available(&self) -> bool {
        crate::ai::available()
    }
}
