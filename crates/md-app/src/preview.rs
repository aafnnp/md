//! The preview pane — the rendered Markdown.
//!
//! [`TextView`] re-parses the entire document on every `set_text`; it is not an
//! incremental renderer. The workspace is responsible for not calling this on
//! every keystroke.

use gpui_kit::component::text::{TextView, TextViewState};
use gpui_kit::*;

pub struct PreviewPane {
    state: Entity<TextViewState>,
    /// The Markdown currently on screen. Remembering it lets an unchanged
    /// document skip the re-parse, and makes the pane's contents observable.
    source: String,
}

impl PreviewPane {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            state: cx.new(|cx| TextViewState::markdown("", cx)),
            source: String::new(),
        }
    }

    /// Re-render the preview from Markdown source.
    pub fn set_markdown(&mut self, markdown: &str, cx: &mut Context<Self>) {
        if self.source == markdown {
            return;
        }
        self.source.clear();
        self.source.push_str(markdown);
        self.state
            .update(cx, |state, cx| state.set_text(markdown, cx));
    }

    /// The Markdown currently rendered.
    ///
    /// Only the tests read this back; the app renders through `TextView`. It is
    /// gated rather than `pub` so the shipping binary carries no dead code.
    #[cfg(test)]
    pub fn source(&self) -> &str {
        &self.source
    }
}

impl Render for PreviewPane {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(TextView::new(&self.state).selectable(true).scrollable(true))
    }
}
