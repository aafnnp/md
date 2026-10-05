//! `md` — a GPUI desktop Markdown editor.

mod actions;
mod editor;
mod preview;
mod settings;
mod sidebar;
mod tab;
mod workspace;

use gpui_kit::*;
use md_core::settings::Settings;

use crate::actions::{CloseTab, ExportHtml, OpenFile, Quit, Save, SaveAs, SelectTab, ToggleTheme};
use crate::settings::AppSettings;
use crate::workspace::Workspace;

/// `Cmd+1`..`Cmd+9` select the first nine tabs, matching the convention every
/// browser and editor uses.
fn tab_shortcuts() -> Vec<KeyBinding> {
    (0..9)
        .map(|n| KeyBinding::new(&format!("cmd-{}", n + 1), SelectTab(n), None))
        .collect()
}

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

            // The settings are read before the window opens so the first frame
            // is already in the right theme, rather than showing the default
            // one and correcting itself.
            let settings = Settings::load();
            AppSettings::set(settings.clone(), cx);
            settings::apply(&settings, None, cx);

            cx.bind_keys([
                KeyBinding::new("cmd-q", Quit, None),
                KeyBinding::new("cmd-w", CloseTab, None),
                KeyBinding::new("cmd-shift-t", ToggleTheme, None),
                KeyBinding::new("cmd-o", OpenFile, None),
                KeyBinding::new("cmd-s", Save, None),
                KeyBinding::new("cmd-shift-s", SaveAs, None),
                KeyBinding::new("cmd-shift-e", ExportHtml, None),
            ]);
            cx.bind_keys(tab_shortcuts());
            gpui_kit::open_window(window_options, cx, |window, cx| {
                cx.new(|cx| Workspace::new(window, cx))
            })
            .expect("failed to open window");
            cx.activate(true);
        });
}
