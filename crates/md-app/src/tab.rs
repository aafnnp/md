//! One open document: its source pane and the preview beside it.

use std::io;
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

        let mut tab = Self {
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
        tab.sync_preview_base(cx);

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

    /// The source pane's text, as it stands right now.
    ///
    /// This is the buffer, not the document: a keystroke reaches the editor a
    /// frame before the change subscription copies it across, so anything that
    /// has to act on what the user is looking at — saving, exporting — reads
    /// it from here rather than trusting the document to have caught up.
    pub fn markdown(&self, cx: &App) -> String {
        self.editor.read(cx).state.read(cx).value().to_string()
    }

    /// The buffer as the editor currently holds it.
    ///
    /// The tests read this back to check that two tabs really do hold separate
    /// buffers.
    #[cfg(test)]
    pub fn text(&self, cx: &App) -> String {
        self.markdown(cx)
    }

    /// A handle on the source pane, so a test can put the caret in it and type.
    ///
    /// Gated like `text`: creating the tab already focuses its editor, so the
    /// only caller that has to do it again is a test moving between tabs.
    #[cfg(test)]
    pub fn editor_focus(&self, cx: &App) -> FocusHandle {
        self.editor.read(cx).state.read(cx).focus_handle(cx)
    }

    /// Write the buffer to disk, and say whether it got there.
    ///
    /// `path` is the Save As case: the file to adopt. `None` writes back to the
    /// file this tab already has, and fails for a document that never had one.
    ///
    /// The editor is read here rather than the document being trusted. The
    /// change subscription keeps the two in step for the dirty dot, but what is
    /// written has to be what is on screen, and the buffer is the authority on
    /// that.
    pub fn save_to(&mut self, path: Option<PathBuf>, cx: &mut Context<Self>) -> io::Result<()> {
        let markdown = self.markdown(cx);
        self.document.set_text(markdown);

        match path {
            Some(path) => self.document.save_as(path),
            None => self.document.save(),
        }?;

        // A first save gives the document a file, and with it a directory the
        // preview's relative images resolve against.
        self.sync_preview_base(cx);

        // The dirty dot goes out, and a first save replaces "Untitled" with the
        // file's own name.
        cx.notify();
        Ok(())
    }

    /// Follow the file this tab was showing to the path it was renamed to.
    ///
    /// Called by the workspace after it moves the file on disk. The buffer is
    /// deliberately left alone: the text is the same, and so is whether it has
    /// been saved.
    pub fn repath(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.document.repath(path);
        // The file moved, so its images did too, relative to a new directory.
        self.sync_preview_base(cx);
        // The tab strip shows the file name, so it has to be redrawn.
        cx.notify();
    }

    /// Tell the preview which folder this document's relative images sit in.
    fn sync_preview_base(&mut self, cx: &mut Context<Self>) {
        let base = self.document.base_dir();
        self.preview
            .update(cx, |preview, cx| preview.set_base_dir(base, cx));
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
        AppContext as _, Bounds, Entity, Focusable as _, Point, TestAppContext, VisualTestContext,
        WindowBounds, WindowHandle, WindowOptions, px, size,
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

    /// Puts some text in a fresh tab and presses one keystroke on it.
    ///
    /// The keystroke goes through the window's key map rather than the action
    /// being dispatched directly, because the point of the tests below is that
    /// the shortcut itself is routed to the source pane — a key context that
    /// only matches where it should.
    fn press_in_editor(
        cx: &mut TestAppContext,
        document: Document,
        keystroke: &str,
    ) -> Entity<Tab> {
        let (window, tab) = open_tab(cx, document);
        let mut cx = VisualTestContext::from_window(window.into(), cx);

        let focus = cx.update(|_, cx| tab.read(cx).editor_focus(cx));
        cx.update(|window, cx| {
            window.focus(&focus, cx);
            window.input("one two one", cx);
        });

        // macOS binds these to Cmd; everywhere else to Ctrl.
        let keystroke = match keystroke {
            "find" if cfg!(target_os = "macos") => "cmd-f",
            "find" => "ctrl-f",
            "replace" if cfg!(target_os = "macos") => "cmd-shift-f",
            "replace" => "ctrl-h",
            other => other,
        };
        cx.simulate_keystrokes(keystroke);
        tab
    }

    /// <kbd>Cmd</kbd>+<kbd>F</kbd> opens the source pane's own find bar.
    ///
    /// The bar is the library's, not the app's: `EditorState::new` turns on
    /// searching for the code-editor mode, and the panel is drawn by the very
    /// `Editor` element the pane renders. That makes this test the only thing
    /// standing between the shortcut and silently doing nothing — if the state
    /// is ever built a different way, the keystroke would fall through to the
    /// window and the bar would never open.
    #[gpui_kit::test]
    fn the_find_bar_opens_on_the_search_shortcut(cx: &mut TestAppContext) {
        let tab = press_in_editor(cx, Document::new(), "find");

        let session = tab.read_with(cx, |tab, cx| {
            tab.editor.read(cx).state.read(cx).search_session().clone()
        });

        assert!(session.open, "the find bar should be open");
        assert!(session.is_active(), "and it should be the one in charge");
        assert!(!session.replace_mode, "Cmd+F finds; it does not replace");
    }

    /// <kbd>Cmd</kbd>+<kbd>Shift</kbd>+<kbd>F</kbd> opens it straight into
    /// replace mode, which is the same bar with the replacement field on it.
    #[gpui_kit::test]
    fn the_replace_bar_opens_on_its_own_shortcut(cx: &mut TestAppContext) {
        let tab = press_in_editor(cx, Document::new(), "replace");

        let session = tab.read_with(cx, |tab, cx| {
            tab.editor.read(cx).state.read(cx).search_session().clone()
        });

        assert!(session.open);
        assert!(
            session.replace_mode,
            "Cmd+Shift+F should ask for a replacement"
        );
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

    fn preview_base(cx: &TestAppContext, tab: &Entity<Tab>) -> Option<std::path::PathBuf> {
        tab.read_with(cx, |tab, cx| {
            tab.preview
                .read(cx)
                .base_dir()
                .map(std::path::Path::to_path_buf)
        })
    }

    /// A document opened from a file resolves its images from that file's
    /// folder, which is what makes `![](diagram.png)` find the diagram next to
    /// the note rather than wherever the app was launched from.
    #[gpui_kit::test]
    fn an_opened_document_resolves_images_from_its_own_folder(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir().join(format!("md-app-base-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.md");
        std::fs::write(&path, "![](diagram.png)\n").unwrap();

        let (_, tab) = open_tab(cx, Document::open(&path).unwrap());
        assert_eq!(preview_base(cx, &tab), Some(dir.clone()));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// An unsaved buffer has no folder, and the preview is told so rather than
    /// being pointed at the working directory.
    #[gpui_kit::test]
    fn an_untitled_buffer_has_no_folder_to_resolve_images_from(cx: &mut TestAppContext) {
        let (_, tab) = open_tab(cx, Document::new());
        assert_eq!(preview_base(cx, &tab), None);
    }

    /// Save-as moves the document, and the images with it: after it, a relative
    /// URL means relative to the new file.
    #[gpui_kit::test]
    fn saving_as_moves_the_folder_images_resolve_from(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir().join(format!("md-app-base-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.md");

        let (_, tab) = open_tab(cx, Document::new());
        assert_eq!(preview_base(cx, &tab), None);

        tab.update(cx, |tab, cx| {
            tab.save_to(Some(path.clone()), cx).unwrap();
        });
        assert_eq!(preview_base(cx, &tab), Some(dir.clone()));

        std::fs::remove_dir_all(&dir).ok();
    }
}
