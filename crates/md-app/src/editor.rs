//! The source pane — the Markdown the user types.
//!
//! This owns the [`EditorState`] and nothing else. The preview is driven by the
//! workspace subscribing to this state's change events, so the pane does not
//! need to know the preview exists.

use gpui_kit::component::input::{Editor, EditorState};
use gpui_kit::*;

pub struct EditorPane {
    /// Public so the workspace can subscribe to its change events.
    pub state: Entity<EditorState>,
}

impl EditorPane {
    pub fn new(initial: &str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            state: cx.new(|cx| EditorState::new(window, cx).default_value(initial)),
        }
    }
}

impl Render for EditorPane {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(Editor::new(&self.state).size_full())
    }
}
