//! The root view: `sidebar | editor | preview`.

use std::time::Duration;

use gpui_kit::base::{h_resizable, resizable_panel};
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::input::InputEvent;
use gpui_kit::*;

use crate::actions::Quit;
use crate::editor::EditorPane;
use crate::preview::PreviewPane;
use crate::sidebar::Sidebar;

/// How long typing must pause before the preview re-renders.
///
/// `TextView` re-parses the whole document on every `set_text` — it has no
/// incremental mode — so re-rendering per keystroke is what makes a long
/// document feel sluggish. Debouncing costs a little latency and removes that
/// cost entirely.
const PREVIEW_DEBOUNCE: Duration = Duration::from_millis(180);

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
    editor: Entity<EditorPane>,
    preview: Entity<PreviewPane>,
    sidebar: Entity<Sidebar>,
    _input_subscription: Subscription,
    /// The debounce timer currently in flight, if any.
    ///
    /// Dropping a GPUI `Task` cancels it, so replacing this field on each
    /// keystroke is what makes the newest one supersede the rest.
    debounce: Option<Task<()>>,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| EditorPane::new(STARTER_DOCUMENT, window, cx));
        let preview = cx.new(PreviewPane::new);
        let sidebar = cx.new(|_| Sidebar);

        let source = editor.read(cx).state.clone();
        let input_subscription = cx.subscribe(&source, |this, source, event, cx| {
            if matches!(event, InputEvent::Change) {
                let markdown = source.read(cx).value().to_string();
                this.queue_preview(markdown, cx);
            }
        });

        let workspace = Self {
            editor,
            preview,
            sidebar,
            _input_subscription: input_subscription,
            debounce: None,
        };
        // Render the starter document rather than waiting for the first keystroke.
        workspace.preview.update(cx, |preview, cx| {
            preview.set_markdown(STARTER_DOCUMENT, cx);
        });
        workspace
    }

    /// Schedule a preview re-render, replacing any pending one.
    fn queue_preview(&mut self, markdown: String, cx: &mut Context<Self>) {
        self.debounce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PREVIEW_DEBOUNCE).await;
            // The workspace may have been dropped while we waited.
            this.update(cx, |workspace, cx| {
                workspace
                    .preview
                    .update(cx, |preview, cx| preview.set_markdown(&markdown, cx));
            })
            .ok();
        }));
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(|_, _: &Quit, _, cx| cx.quit()))
            .child(
                h_resizable("workspace")
                    .child(resizable_panel().size(px(220.)).child(self.sidebar.clone()))
                    .child(
                        resizable_panel()
                            // `size_range` takes a closed range and has no
                            // "unbounded" marker, so the upper bound stands in
                            // for infinity — no real window approaches it.
                            .size_range(px(200.)..px(10_000.))
                            .child(self.editor.clone()),
                    )
                    .child(
                        resizable_panel()
                            // `size_range` takes a closed range and has no
                            // "unbounded" marker, so the upper bound stands in
                            // for infinity — no real window approaches it.
                            .size_range(px(200.)..px(10_000.))
                            .child(self.preview.clone()),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    // Imported narrowly: `use super::*` would drag in the `gpui_kit::*` glob,
    // whose `test` attribute macro shadows the built-in `#[test]`.
    use super::{PREVIEW_DEBOUNCE, STARTER_DOCUMENT, Workspace};
    use gpui_kit::base::Root;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AppContext as _, Bounds, Entity, Focusable as _, Point, TestAppContext, WindowBounds,
        WindowHandle, WindowOptions, px, size,
    };
    use std::time::Duration;

    /// A real window rendering a real `Workspace` — GPUI's headless renderer,
    /// not a mock. Typing here goes through the same input path as typing on
    /// screen.
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

    fn editor_text(cx: &TestAppContext, workspace: &Entity<Workspace>) -> String {
        workspace.read_with(cx, |workspace, cx| {
            workspace.editor.read(cx).state.read(cx).value().to_string()
        })
    }

    fn preview_markdown(cx: &TestAppContext, workspace: &Entity<Workspace>) -> String {
        workspace.read_with(cx, |workspace, cx| {
            workspace.preview.read(cx).source().to_string()
        })
    }

    /// The M1 feature, end to end: a keystroke in the source pane reaches the
    /// preview pane, but only once typing pauses.
    #[gpui_kit::test]
    fn typing_reaches_the_preview_only_after_the_debounce(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);

        // The starter document renders up front, before any keystroke.
        assert_eq!(preview_markdown(cx, &workspace), STARTER_DOCUMENT);

        let focus = workspace.read_with(cx, |workspace, cx| {
            workspace
                .editor
                .read(cx)
                .state
                .read(cx)
                .focus_handle(cx)
                .clone()
        });
        cx.update_window(window.into(), |_, window, cx| {
            window.focus(&focus, cx);
            window.input("hello", cx);
        })
        .unwrap();

        // The keystrokes landed in the editor...
        let typed = editor_text(cx, &workspace);
        assert!(
            typed.contains("hello") && typed != STARTER_DOCUMENT,
            "typing did not reach the editor; editor holds {typed:?}"
        );

        // ...but the preview is deliberately still stale. Re-parsing the whole
        // document on every keystroke is exactly what the debounce avoids, so a
        // preview that were already current here would mean the debounce is
        // doing nothing.
        assert_eq!(preview_markdown(cx, &workspace), STARTER_DOCUMENT);

        // Let the debounce elapse; the pending render fires with the newest
        // text. `advance_clock` only makes the timer ready, so the queued work
        // still has to be drained afterwards.
        cx.executor()
            .advance_clock(PREVIEW_DEBOUNCE + Duration::from_millis(20));
        cx.run_until_parked();
        assert_eq!(preview_markdown(cx, &workspace), typed);
    }
}
