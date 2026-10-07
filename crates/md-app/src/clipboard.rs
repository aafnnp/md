//! The system clipboard, in the one place that writes rich text to it.
//!
//! Everything platform-specific about a copy is confined to this file: the
//! workspace asks for a document to be put on the clipboard and is handed a
//! `Result` back, the same shape every other fallible operation in the app has.
//!
//! A `text/html` flavour is what makes a paste into a WeChat or 头条 editor
//! arrive as a laid-out document instead of one long paragraph. Neither the
//! document's Markdown nor its rendered HTML would do on its own — the Markdown
//! pastes as literal punctuation, and GPUI's own clipboard carries plain text
//! only.

use std::cell::RefCell;

use arboard::Clipboard;
use md_core::typeset::Layout;

thread_local! {
    /// The clipboard, built once and kept for the life of the thread.
    ///
    /// Kept rather than dropped because of X11: a copy there is an announcement,
    /// and the process that made it is the one that serves the paste. Letting
    /// this go at the end of [`copy`] would leave the next <kbd>Cmd</kbd>+<kbd>V</kbd>
    /// asking an owner that no longer exists, which looks exactly like a copy
    /// that never happened.
    ///
    /// A connection that could not be made leaves this `None` and reports the
    /// error, so it is tried again rather than being written off for the run —
    /// no display at startup is a state a session can come out of.
    /// Built as a constant rather than by the macro's lazy path — the slot
    /// starts `None` either way, and the constant form just drops the
    /// per-access initialisation check.
    static CLIPBOARD: RefCell<Option<Clipboard>> = const { RefCell::new(None) };
}

/// Put `layout` on the system clipboard.
///
/// Both flavours go at once: the HTML for an editor that takes it, and the
/// plain text for everywhere else — a terminal, a plain text field, another
/// Markdown file. The platform picks, so a paste that cannot be rich degrades
/// to the document's text rather than to nothing.
pub fn copy(layout: &Layout) -> Result<(), String> {
    CLIPBOARD.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(Clipboard::new().map_err(|error| error.to_string())?);
        }
        slot.as_mut()
            .expect("just filled in")
            .set_html(layout.html.as_str(), Some(layout.plain.as_str()))
            .map_err(|error| error.to_string())
    })
}
