//! The preview pane — the rendered Markdown.
//!
//! [`TextView`] re-parses the entire document on every `set_text`; it is not an
//! incremental renderer. The workspace is responsible for not calling this on
//! every keystroke.

use std::path::{Path, PathBuf};

use gpui_kit::base::{TextView, TextViewState};
use gpui_kit::*;

use md_core::image;

use crate::settings::{AppSettings, preview_padding};

pub struct PreviewPane {
    state: Entity<TextViewState>,
    /// The Markdown currently on screen. Remembering it lets an unchanged
    /// document skip the re-parse, and makes the pane's contents observable.
    source: String,
    /// The folder this document's relative image URLs are relative to.
    ///
    /// `None` until the tab has a file: an unsaved buffer has no directory, and
    /// resolving `diagram.png` against the process's working directory would
    /// point at some unrelated file rather than at nothing.
    base_dir: Option<PathBuf>,
}

impl PreviewPane {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            state: cx.new(|cx| TextViewState::markdown("", cx)),
            source: String::new(),
            base_dir: None,
        }
    }

    /// Point the preview at the folder images should be resolved from.
    ///
    /// A tab's file changes under it — save-as gives an untitled buffer a path,
    /// and a rename in the sidebar moves the document — so the base directory is
    /// set wherever the path is, not only once.
    pub fn set_base_dir(&mut self, base_dir: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.base_dir == base_dir {
            return;
        }
        self.base_dir = base_dir;
        // Every image on screen may now resolve somewhere else.
        cx.notify();
    }

    /// Re-render the preview from Markdown source.
    pub fn set_markdown(&mut self, markdown: &str, cx: &mut Context<Self>) {
        if self.source == markdown {
            return;
        }
        self.source.clear();
        self.source.push_str(markdown);
        self.state
            .update(cx, |state, cx| state.set_text(markdown, cx));
    }

    /// The Markdown currently rendered.
    ///
    /// Only the tests read this back; the app renders through `TextView`. It is
    /// gated rather than `pub` so the shipping binary carries no dead code.
    #[cfg(test)]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The folder images are resolved from. Read back by the tab's tests, which
    /// check that it follows the document.
    #[cfg(test)]
    pub fn base_dir(&self) -> Option<&Path> {
        self.base_dir.as_deref()
    }
}

impl Render for PreviewPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The resolver is `'static`, so it owns its copy of the directory rather
        // than borrowing the pane.
        let base_dir = self.base_dir.clone();
        // Read on every frame rather than cached: the settings dialog can change
        // this while the pane is on screen, and the frame after it does is when
        // the new margin has to be there.
        let padding = preview_padding(&AppSettings::current(cx));
        div().size_full().p(px(padding)).child(
            TextView::new(&self.state)
                .selectable(true)
                .scrollable(true)
                .image_source(move |url| {
                    match base_dir.as_deref().and_then(|dir| resolve(dir, url)) {
                        // A file we found: load it from disk.
                        Some(path) => path.into(),
                        // Anything else — a remote URL, an absolute path, a data
                        // URL — goes back untouched, and `TextView` handles it as it
                        // would have without us. Note this takes `SharedUri`, not
                        // `&str`: a `&str` that fails to parse as a URL is treated as
                        // an *embedded* asset name, which would break every remote
                        // image in the document.
                        None => url.clone().into(),
                    }
                }),
        )
    }
}

/// The image URL from the document, as text.
///
/// Split out of the closure so the `&SharedUri` to `&str` deref happens in one
/// place with a name attached to it.
fn resolve(base_dir: &Path, url: &SharedUri) -> Option<PathBuf> {
    image::resolve(base_dir, url)
}
