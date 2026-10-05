//! `md` — a GPUI desktop Markdown editor.
//!
//! M0 smoke test: prove that `gpui-kit` can open a window on this machine.
//! This is the go/no-go gate for the whole GPUI stack — if this does not come
//! up, the architecture in the plan has to be reconsidered before M1 starts.

use gpui_kit::*;

struct Hello;

impl Render for Hello {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .child("md")
            .child("GPUI is up.")
    }
}

fn main() {
    gpui_kit::application().run(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Hello))
            .expect("failed to open window");
        cx.activate(true);
    });
}
