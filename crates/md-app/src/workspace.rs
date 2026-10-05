//! The root view: a tab strip over `sidebar | active tab`.

use std::rc::Rc;

use gpui_kit::base::{h_resizable, resizable_panel};
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::tab::{Tab as TabButton, TabBar};
use gpui_kit::*;

use md_core::Document;

use crate::actions::{CloseTab, Quit, SelectTab};
use crate::sidebar::Sidebar;
use crate::tab::Tab;

const STARTER_DOCUMENT: &str = "\
# md

A Markdown editor. Type on the left, read on the right.

## What works so far

- **Bold**, _italic_, `code`, and [links](https://gpui.rs)
- Lists, quotes, and rules

> The preview re-renders shortly after you stop typing.

```rust
fn main() {
    println!(\"hello\");
}
```
";

pub struct Workspace {
    tabs: Vec<Entity<Tab>>,
    /// Index into `tabs`. Meaningless while `tabs` is empty.
    active: usize,
    sidebar: Entity<Sidebar>,
    /// Source of `Tab` ids. Never reused, so a closed tab's splitter layout
    /// cannot leak into the tab that takes its place.
    next_tab_id: u64,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sidebar = cx.new(|_| Sidebar);
        let mut workspace = Self {
            tabs: Vec::new(),
            active: 0,
            sidebar,
            next_tab_id: 0,
        };

        let mut starter = Document::new();
        starter.set_text(STARTER_DOCUMENT);
        workspace.open(starter, window, cx);
        workspace
    }

    /// Open a document in a new tab and focus it.
    pub fn open(&mut self, document: Document, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;

        let tab = cx.new(|cx| Tab::new(id, document, window, cx));
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        cx.notify();
    }

    /// Close the active tab, asking first when it has unsaved edits.
    pub fn request_close_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        if !tab.read(cx).is_dirty() {
            self.close_active(cx);
            return;
        }

        let name = tab.read(cx).title();
        self.confirm_discard(
            window,
            cx,
            format!("“{name}” has unsaved changes. Closing it will discard them."),
            "Discard",
            Self::close_active,
        );
    }

    /// Quit, asking first if any tab has unsaved edits.
    pub fn request_quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dirty = self
            .tabs
            .iter()
            .filter(|tab| tab.read(cx).is_dirty())
            .count();
        if dirty == 0 {
            cx.quit();
            return;
        }

        let what = if dirty == 1 {
            "1 tab has unsaved changes.".to_string()
        } else {
            format!("{dirty} tabs have unsaved changes.")
        };
        self.confirm_discard(
            window,
            cx,
            format!("{what} Quitting will discard them."),
            "Quit anyway",
            |_, cx| cx.quit(),
        );
    }

    /// Ask before discarding edits, then run `on_confirm`.
    ///
    /// The dialog's callbacks are handed `&mut App`, not `Context<Workspace>`,
    /// so the workspace is reached through a weak handle. A strong one would
    /// close a reference cycle through the window root, which owns the dialog.
    fn confirm_discard(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        message: String,
        ok_text: &'static str,
        on_confirm: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) {
        let workspace = cx.weak_entity();
        // The dialog's builder is an `Fn`, not an `FnOnce` — it may in principle
        // run more than once — so nothing may be moved out of it. Both captures
        // are therefore cloned into the innermost closure instead.
        let on_confirm = Rc::new(on_confirm);
        window.open_alert_dialog(cx, move |alert, _, _| {
            let workspace = workspace.clone();
            let on_confirm = on_confirm.clone();
            alert
                .confirm()
                .title("Discard unsaved changes?")
                .description(message.clone())
                .ok_text(ok_text)
                .cancel_text("Keep editing")
                .on_ok(move |_, _, cx| {
                    // Whatever happens next, the dialog has served its purpose;
                    // leaving it up would only invite a second confirm.
                    workspace
                        .update(cx, |workspace, cx| on_confirm(workspace, cx))
                        .ok();
                    true
                })
        });
    }

    /// Close the active tab and fall back to its left-hand neighbour.
    fn close_active(&mut self, cx: &mut Context<Self>) {
        if self.tabs.is_empty() {
            return;
        }
        self.tabs.remove(self.active);
        // `saturating_sub` covers the case where the last tab was closed and
        // `tabs` is now empty; `active` is simply not read again until a tab
        // is opened, and opening sets it.
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        cx.notify();
    }

    /// Focus the tab at `index`, ignoring an out-of-range request.
    pub fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.tabs.len() && index != self.active {
            self.active = index;
            cx.notify();
        }
    }

    fn render_tab_strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let dirty_dot = cx.theme().primary;
        let buttons = self
            .tabs
            .iter()
            .map(|tab| {
                let tab = tab.read(cx);
                let button = TabButton::new().label(tab.title());
                if tab.is_dirty() {
                    button.suffix(div().size(px(7.)).rounded_full().bg(dirty_dot))
                } else {
                    button
                }
            })
            .collect::<Vec<_>>();

        // `TabBar::on_click` hands the callback a plain `&mut App` rather than a
        // `Context<Workspace>`, so `cx.listener` cannot be used here. A weak
        // handle avoids the cycle a strong `Entity` would create.
        let workspace = cx.weak_entity();
        TabBar::new("tabs")
            .selected_index(self.active)
            .children(buttons)
            .on_click(move |index, _, cx| {
                workspace
                    .update(cx, |workspace, cx| workspace.select(*index, cx))
                    .ok();
            })
    }

    fn render_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(tab) = self.tabs.get(self.active) else {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child("No open documents")
                .into_any_element();
        };

        h_resizable("workspace")
            .child(resizable_panel().size(px(220.)).child(self.sidebar.clone()))
            .child(resizable_panel().child(tab.clone()))
            .into_any_element()
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(|this, _: &Quit, window, cx| this.request_quit(window, cx)))
            .on_action(
                cx.listener(|this, _: &CloseTab, window, cx| this.request_close_active(window, cx)),
            )
            .on_action(cx.listener(|this, action: &SelectTab, _, cx| this.select(action.0, cx)))
            .child(self.render_tab_strip(cx))
            .child(div().flex_1().min_h_0().child(self.render_body(cx)))
    }
}

#[cfg(test)]
mod tests {
    // Imported narrowly: `use super::*` would drag in the `gpui_kit::*` glob,
    // whose `test` attribute macro shadows the built-in `#[test]`.
    use super::{STARTER_DOCUMENT, Workspace};
    use crate::actions::{CloseTab, SelectTab};
    use gpui_kit::base::Root;
    use gpui_kit::component::WindowExt as _;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AppContext as _, Bounds, Entity, Point, TestAppContext, WindowBounds, WindowHandle,
        WindowOptions, px, size,
    };
    use md_core::Document;
    use std::path::{Path, PathBuf};

    fn open_workspace(cx: &mut TestAppContext) -> (WindowHandle<Root>, Entity<Workspace>) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            let bounds = Bounds {
                origin: Point::default(),
                size: size(px(1200.), px(820.)),
            };
            let (window, workspace) = gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| Workspace::new(window, cx)),
            )
            .expect("open test window");
            (window.downcast::<Root>().expect("base Root"), workspace)
        })
    }

    fn titles(cx: &TestAppContext, workspace: &Entity<Workspace>) -> Vec<String> {
        workspace.read_with(cx, |workspace, cx| {
            workspace
                .tabs
                .iter()
                .map(|tab| tab.read(cx).title())
                .collect()
        })
    }

    fn active(workspace: &Entity<Workspace>, cx: &TestAppContext) -> usize {
        workspace.read_with(cx, |workspace, _| workspace.active)
    }

    fn dispatch<A: gpui_kit::Action>(
        window: WindowHandle<Root>,
        action: A,
        cx: &mut TestAppContext,
    ) {
        cx.update_window(window.into(), |_, window, cx| {
            window.dispatch_action(Box::new(action), cx);
        })
        .unwrap();
    }

    /// A document with no unsaved edits, backed by a real file so that
    /// `is_dirty` is false and closing it needs no confirmation.
    fn clean_document(dir: &Path, name: &str) -> Document {
        let path = dir.join(name);
        std::fs::write(&path, format!("# {name}")).unwrap();
        Document::open(&path).unwrap()
    }

    fn open_document(
        window: WindowHandle<Root>,
        workspace: &Entity<Workspace>,
        document: Document,
        cx: &mut TestAppContext,
    ) {
        cx.update_window(window.into(), |_, window, cx| {
            workspace.update(cx, |workspace, cx| workspace.open(document, window, cx));
        })
        .unwrap();
    }

    /// A scratch directory private to one test, so the tests can run in
    /// parallel without treading on each other's files.
    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("md-app-workspace-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn has_dialog(window: WindowHandle<Root>, cx: &mut TestAppContext) -> bool {
        cx.update_window(window.into(), |_, window, cx| window.has_active_dialog(cx))
            .unwrap()
    }

    /// A fresh workspace holds one tab, showing the starter document.
    #[gpui_kit::test]
    fn a_new_workspace_opens_one_tab(cx: &mut TestAppContext) {
        let (_, workspace) = open_workspace(cx);
        assert_eq!(active(&workspace, cx), 0);
        assert_eq!(titles(cx, &workspace), ["Untitled"]);
    }

    /// Opening focuses the new tab; closing falls back to the previous one.
    #[gpui_kit::test]
    fn opening_and_closing_tabs_tracks_the_active_index(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);
        let dir = scratch("index");
        open_document(window, &workspace, clean_document(&dir, "a.md"), cx);
        open_document(window, &workspace, clean_document(&dir, "b.md"), cx);

        assert_eq!(titles(cx, &workspace), ["Untitled", "a.md", "b.md"]);
        assert_eq!(
            active(&workspace, cx),
            2,
            "opening should focus the new tab"
        );

        dispatch(window, CloseTab, cx);
        assert_eq!(titles(cx, &workspace), ["Untitled", "a.md"]);
        assert_eq!(active(&workspace, cx), 1, "should fall back to the left");

        // The remaining tab is the starter document, which has never been
        // saved and so is dirty; `CloseTab` would prompt rather than close it.
        // Drive the close directly to exercise the empty-strip edge case; the
        // prompt itself is covered by `closing_a_dirty_tab_asks_before_discarding`.
        workspace.update(cx, |workspace, cx| workspace.close_active(cx));
        workspace.update(cx, |workspace, cx| workspace.close_active(cx));
        assert!(titles(cx, &workspace).is_empty());
        // Closing an already-empty strip is a no-op, not a panic.
        workspace.update(cx, |workspace, cx| workspace.close_active(cx));
        assert!(titles(cx, &workspace).is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// `SelectTab` moves between tabs and rejects an out-of-range index.
    #[gpui_kit::test]
    fn select_tab_focuses_a_known_tab_only(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);
        let dir = scratch("select");
        open_document(window, &workspace, clean_document(&dir, "a.md"), cx);
        assert_eq!(active(&workspace, cx), 1);

        dispatch(window, SelectTab(0), cx);
        assert_eq!(active(&workspace, cx), 0);

        dispatch(window, SelectTab(7), cx);
        assert_eq!(active(&workspace, cx), 0, "an unknown index is ignored");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Two open documents do not share a buffer: each tab's editor holds the
    /// text it was opened with.
    #[gpui_kit::test]
    fn each_tab_keeps_its_own_buffer(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);
        let dir = scratch("buffers");
        open_document(window, &workspace, clean_document(&dir, "a.md"), cx);

        let buffers = workspace.read_with(cx, |workspace, cx| {
            workspace
                .tabs
                .iter()
                .map(|tab| tab.read(cx).text(cx))
                .collect::<Vec<_>>()
        });
        assert_eq!(buffers, [STARTER_DOCUMENT, "# a.md"]);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// `Cmd+W` on a tab with unsaved edits must not throw them away. It puts up
    /// a dialog and leaves the tab alone until the user answers.
    #[gpui_kit::test]
    fn closing_a_dirty_tab_asks_before_discarding(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);

        // Opening a tab focuses its editor, so this types into the starter
        // document and leaves it dirty.
        cx.update_window(window.into(), |_, window, cx| window.input("edited", cx))
            .unwrap();
        assert!(
            workspace.read_with(cx, |workspace, cx| workspace.tabs[0].read(cx).is_dirty()),
            "typing should have made the tab dirty"
        );

        dispatch(window, CloseTab, cx);
        assert_eq!(
            titles(cx, &workspace),
            ["Untitled"],
            "the tab must survive until the discard is confirmed"
        );
        assert!(has_dialog(window, cx), "a confirmation should be on screen");

        // Dismissing keeps both the tab and the edits.
        cx.update_window(window.into(), |_, window, cx| window.close_dialog(cx))
            .unwrap();
        assert_eq!(titles(cx, &workspace), ["Untitled"]);
        assert!(workspace.read_with(cx, |workspace, cx| workspace.tabs[0].read(cx).is_dirty()));
    }
}
