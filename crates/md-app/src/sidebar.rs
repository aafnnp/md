//! The left sidebar.
//!
//! Placeholder for M1: it holds the layout's shape so the splitter has
//! something real to resize. M2 fills it with the file tree.

use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

pub struct Sidebar;

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p_3()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child("No folder open")
    }
}
