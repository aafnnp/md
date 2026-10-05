//! One open document: its source pane and the preview beside it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::base::{h_resizable, resizable_panel};
use gpui_kit::component::input::InputEvent;
use gpui_kit::*;

use md_core::Document;

use crate::editor::EditorPane;
use crate::preview::PreviewPane;

/// How long typing must pause before the preview re-renders.
///
/// `TextView` re-parses the whole document on every `set_text` — it has no
/// incremental mode — so re-rendering per keystroke is what makes a long
/// document feel sluggish. Debouncing costs a little latency and removes that
/// cost entirely.
const PREVIEW_DEBOUNCE: Duration = Duration::from_millis(180);

pub struct Tab {
    /// Distinguishes this tab's splitter from every other tab's, so each keeps
    /// its own pane widths.
    id: u64,
    document: Document,
    editor: Entity<EditorPane>,
    preview: Entity<PreviewPane>,
    _input_subscription: Subscription,
    /// The debounce timer currently in flight, if any.
    ///
    /// Dropping a GPUI `Task` cancels it, so replacing this field on each
    /// keystroke is what makes the newest one supersede the rest.
    debounce: Option<Task<()>>,
}

impl Tab {
    pub fn new(id: u64, document: Document, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| EditorPane::new(document.text(), window, cx));
        let preview = cx.new(PreviewPane::new);

        let source = editor.read(cx).state.clone();
        let input_subscription = cx.subscribe(&source, |this, source, event, cx| {
            if matches!(event, InputEvent::Change) {
                let markdown = source.read(cx).value().to_string();
                // Keep the document in step with the editor: it is what the tab
                // label's dirty dot and the save/close prompts read.
                this.document.set_text(markdown.clone());
                this.queue_preview(markdown, cx);
                cx.notify();
            }
        });

        let tab = Self {
            id,
            document,
            editor,
            preview,
            _input_subscription: input_subscription,
            debounce: None,
        };
        // Render the document rather than waiting for the first keystroke.
        let initial = tab.document.text().to_string();
        tab.preview
            .update(cx, |preview, cx| preview.set_markdown(&initial, cx));

        // An opened document is the one being edited, so put the caret in it.
        // Without this the first keystroke after opening a file goes nowhere.
        let focus = tab.editor.read(cx).state.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        tab
    }

    /// Name to show on the tab strip.
    pub fn title(&self) -> String {
        self.document.display_name()
    }

    /// Whether the buffer has edits that are not on disk.
    pub fn is_dirty(&self) -> bool {
        self.document.is_dirty()
    }

    /// The file this tab is editing, if it was opened from or saved to one.
    ///
    /// A tab with no path is an untitled buffer, and cannot be the tab that
    /// already shows a file the sidebar is asking to open.
    pub fn path(&self) -> Option<&Path> {
        self.document.path()
    }

    /// The buffer as the editor currently holds it.
    ///
    /// The tests read this back to check that two tabs really do hold separate
    /// buffers. Gated rather than `pub` so the shipping binary carries no dead
    /// code; the app renders through the editor element itself.
    #[cfg(test)]
    pub fn text(&self, cx: &App) -> String {
        self.editor.read(cx).state.read(cx).value().to_string()
    }

    /// Follow the file this tab was showing to the path it was renamed to.
    ///
    /// Called by the workspace after it moves the file on disk. The buffer is
    /// deliberately left alone: the text is the same, and so is whether it has
    /// been saved.
    pub fn repath(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.document.repath(path);
        // The tab strip shows the file name, so it has to be redrawn.
        cx.notify();
    }

    /// Schedule a preview re-render, replacing any pending one.
    fn queue_preview(&mut self, markdown: String, cx: &mut Context<Self>) {
        self.debounce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PREVIEW_DEBOUNCE).await;
            // The tab may have been closed while we waited.
            this.update(cx, |tab, cx| {
                tab.preview
                    .update(cx, |preview, cx| preview.set_markdown(&markdown, cx));
            })
            .ok();
        }));
    }
}

impl Render for Tab {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        h_resizable(("tab-split", self.id))
            .child(
                resizable_panel()
                    // `size_range` takes a closed range and has no "unbounded"
                    // marker, so the upper bound stands in for infinity — no
                    // real window approaches it.
                    .size_range(px(200.)..px(10_000.))
                    .child(self.editor.clone()),
            )
            .child(
                resizable_panel()
                    .size_range(px(200.)..px(10_000.))
                    .child(self.preview.clone()),
            )
    }
}

#[cfg(test)]
mod tests {
    // Imported narrowly: `use super::*` would drag in the `gpui_kit::*` glob,
    // whose `test` attribute macro shadows the built-in `#[test]`.
    use super::{PREVIEW_DEBOUNCE, Tab};
    use gpui_kit::base::Root;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AppContext as _, Bounds, Entity, Focusable as _, Point, TestAppContext, WindowBounds,
        WindowHandle, WindowOptions, px, size,
    };
    use md_core::Document;
    use std::time::Duration;

    fn open_tab(cx: &mut TestAppContext, document: Document) -> (WindowHandle<Root>, Entity<Tab>) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            let bounds = Bounds {
                origin: Point::default(),
                size: size(px(1200.), px(820.)),
            };
            let (window, tab) = gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| Tab::new(0, document, window, cx)),
            )
            .expect("open test window");
            (window.downcast::<Root>().expect("base Root"), tab)
        })
    }

    fn editor_text(cx: &TestAppContext, tab: &Entity<Tab>) -> String {
        tab.read_with(cx, |tab, cx| tab.text(cx))
    }

    fn preview_markdown(cx: &TestAppContext, tab: &Entity<Tab>) -> String {
        tab.read_with(cx, |tab, cx| tab.preview.read(cx).source().to_string())
    }

    /// A keystroke in the source pane reaches the preview pane, but only once
    /// typing pauses.
    #[gpui_kit::test]
    fn typing_reaches_the_preview_only_after_the_debounce(cx: &mut TestAppContext) {
        let (window, tab) = open_tab(cx, Document::new());

        // An empty document renders as an empty preview.
        assert_eq!(preview_markdown(cx, &tab), "");

        let focus = tab.read_with(cx, |tab, cx| {
            tab.editor.read(cx).state.read(cx).focus_handle(cx).clone()
        });
        cx.update_window(window.into(), |_, window, cx| {
            window.focus(&focus, cx);
            window.input("# Title", cx);
        })
        .unwrap();

        // The keystrokes landed in the editor, and made the document dirty...
        let typed = editor_text(cx, &tab);
        assert_eq!(typed, "# Title");
        assert!(tab.read_with(cx, |tab, _| tab.is_dirty()));

        // ...but the preview is deliberately still stale. Re-parsing the whole
        // document on every keystroke is exactly what the debounce avoids, so a
        // preview that were already current here would mean the debounce is
        // doing nothing.
        assert_eq!(preview_markdown(cx, &tab), "");

        // Let the debounce elapse; the pending render fires with the newest
        // text. `advance_clock` only makes the timer ready, so the queued work
        // still has to be drained afterwards.
        cx.executor()
            .advance_clock(PREVIEW_DEBOUNCE + Duration::from_millis(20));
        cx.run_until_parked();
        assert_eq!(preview_markdown(cx, &tab), "# Title");
    }

    /// A document opened from disk starts clean and shows its file name.
    #[gpui_kit::test]
    fn an_opened_document_reaches_the_editor(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir().join(format!("md-app-tab-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.md");
        std::fs::write(&path, "# Saved\n").unwrap();

        let (_, tab) = open_tab(cx, Document::open(&path).unwrap());

        assert_eq!(editor_text(cx, &tab), "# Saved\n");
        assert_eq!(tab.read_with(cx, |tab, _| tab.title()), "note.md");
        assert!(!tab.read_with(cx, |tab, _| tab.is_dirty()));

        std::fs::remove_dir_all(&dir).ok();
    }
}
