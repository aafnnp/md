//! The source pane — the Markdown the user types.
//!
//! This owns the [`EditorState`] and nothing else. The preview is driven by the
//! workspace subscribing to this state's change events, so the pane does not
//! need to know the preview exists.

use gpui_kit::component::input::{Editor, EditorState};
use gpui_kit::*;

use crate::settings::AppSettings;

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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = Editor::new(&self.state).size_full();

        match AppSettings::current(cx).editor_max_width {
            // Centred in the pane: a maximised window would otherwise stretch
            // every line to the full width of the screen, which is further than
            // the eye can travel back.
            Some(width) => div()
                .size_full()
                .flex()
                .justify_center()
                .child(div().h_full().w(px(width)).max_w_full().child(editor))
                .into_any_element(),
            None => div().size_full().child(editor).into_any_element(),
        }
    }
}
