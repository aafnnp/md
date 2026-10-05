//! `md` — a GPUI desktop Markdown editor.

mod actions;
mod editor;
mod preview;
mod sidebar;
mod workspace;

use gpui_kit::*;

use crate::actions::Quit;
use crate::workspace::Workspace;

fn main() {
    let window_options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: Point::default(),
            size: size(px(1200.), px(820.)),
        })),
        ..Default::default()
    };

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
            gpui_kit::open_window(window_options, cx, |window, cx| {
                cx.new(|cx| Workspace::new(window, cx))
            })
            .expect("failed to open window");
            cx.activate(true);
        });
}
