use std::path::{Path, PathBuf};

/// A single open markdown buffer.
///
/// Tracks the on-disk path (absent for a never-saved document), the current
/// text, and whether the text has diverged from what is on disk.
#[derive(Debug, Clone, Default)]
pub struct Document {
    path: Option<PathBuf>,
    text: String,
    /// Text as it was last read from or written to disk, used to decide
    /// whether the buffer is dirty and to support revert.
    saved_text: String,
    /// Which numbered untitled buffer this is.
    ///
    /// `None` is "no number", which is the first untitled tab — it shows as a
    /// bare `Untitled`. The number only counts while there is no path; see
    /// [`Self::untitled_number`].
    untitled: Option<u32>,
}

impl Document {
    /// A new, empty, never-saved document.
    pub fn new() -> Self {
        Self::default()
    }

    /// A never-saved document that starts holding `text` and starts clean.
    ///
    /// This is for a buffer the app seeded itself rather than one the user
    /// typed into — the starter document a new window opens with. Going through
    /// [`Self::new`] and [`Self::set_text`] instead leaves `saved_text` empty,
    /// so the buffer is dirty from the first frame: the tab wears a dot for text
    /// nobody wrote, and closing it asks whether to discard changes that were
    /// never made.
    ///
    /// Not `open` and not `save_as`: nothing is written, and the document still
    /// has no path, so it is still "Untitled" with no folder to resolve images
    /// against until it is saved somewhere.
    pub fn scratch(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            path: None,
            saved_text: text.clone(),
            text,
            untitled: None,
        }
    }

    /// Load a document from disk.
    pub fn open(path: impl Into<PathBuf>) -> std::io::Result<Self> {
        let path = path.into();
        let text = std::fs::read_to_string(&path)?;
        Ok(Self {
            path: Some(path),
            saved_text: text.clone(),
            text,
            // A file that came off disk has a name, so it never holds a number.
            untitled: None,
        })
    }

    /// Give this untitled buffer a number, for the tab strip.
    ///
    /// A builder rather than a setter so a document can be numbered in the same
    /// expression that creates it, before it is handed to a tab.
    pub fn numbered(mut self, number: u32) -> Self {
        self.untitled = Some(number);
        self
    }

    /// The number this buffer wears on the tab strip, but only while it has
    /// never been saved.
    ///
    /// `None` once there is a path: the file has a name of its own now, and the
    /// number is retired. Without that, closing `Untitled 2` and asking for
    /// another buffer would hand out `2` again while a *named* file is still
    /// holding it.
    pub fn untitled_number(&self) -> Option<u32> {
        self.path.is_none().then_some(self.untitled).flatten()
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The directory relative image and link paths in this document are
    /// resolved against.
    ///
    /// `None` for a buffer that has never been saved. There is no honest answer
    /// for one — the working directory is where the process happens to have been
    /// started, not where the document is — so the caller is told there is
    /// nothing to resolve against rather than handed a guess.
    pub fn base_dir(&self) -> Option<PathBuf> {
        self.path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replace the buffer contents, leaving the dirty flag to `is_dirty`.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
    }

    pub fn is_dirty(&self) -> bool {
        self.text != self.saved_text
    }

    /// Write the buffer to disk, adopting `path` if the document has none yet
    /// (i.e. this is a "Save As").
    pub fn save_as(&mut self, path: impl Into<PathBuf>) -> std::io::Result<()> {
        let path = path.into();
        std::fs::write(&path, &self.text)?;
        self.path = Some(path);
        self.saved_text = self.text.clone();
        Ok(())
    }

    /// Save to the existing path, or fail if the document has never been saved.
    pub fn save(&mut self) -> std::io::Result<()> {
        match self.path.clone() {
            Some(path) => self.save_as(path),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "document has no path; use save_as",
            )),
        }
    }

    /// Point the document at a file it has been moved to, without touching
    /// either the filesystem or the buffer.
    ///
    /// Renaming an open file has to bring its tab along. Left where it was, the
    /// tab would show the old name, and a later save would write the old path
    /// back into existence — recreating the very file the user renamed away.
    /// The contents and the dirty flag are untouched: nothing about the text
    /// changed, only where it lives.
    pub fn repath(&mut self, path: impl Into<PathBuf>) {
        self.path = Some(path.into());
    }

    /// Re-read from disk, discarding unsaved changes.
    pub fn revert(&mut self) -> std::io::Result<()> {
        let path = self
            .path
            .clone()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no path"))?;
        *self = Self::open(path)?;
        Ok(())
    }

    /// Name to show on the tab: the file name, or `Untitled` when never saved.
    ///
    /// Untitled buffers are numbered so several of them can be told apart:
    /// `Untitled`, `Untitled 2`, `Untitled 3`. The first carries no number —
    /// the convention is "the first one is plain", not "the first one is
    /// `Untitled 1`".
    pub fn display_name(&self) -> String {
        if let Some(name) = self.path.as_deref().and_then(Path::file_name) {
            return name.to_string_lossy().into_owned();
        }
        match self.untitled {
            Some(number) if number > 1 => format!("Untitled {number}"),
            _ => "Untitled".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_document_is_untitled_and_clean() {
        let doc = Document::new();
        assert_eq!(doc.display_name(), "Untitled");
        assert!(!doc.is_dirty());
        assert!(doc.path().is_none());
    }

    #[test]
    fn editing_marks_dirty_and_reverting_clears_it() {
        let mut doc = Document::new();
        doc.set_text("hello");
        assert!(doc.is_dirty());

        // Writing the same text back to the saved value clears the flag.
        doc.set_text("");
        assert!(!doc.is_dirty());
    }

    /// A seeded buffer is clean to begin with, which is what keeps a new
    /// window's starter tab from asking to discard changes nobody made.
    #[test]
    fn a_scratch_document_holds_its_text_and_starts_clean() {
        let doc = Document::scratch("# md\n");

        assert_eq!(doc.text(), "# md\n");
        assert!(!doc.is_dirty());
        // Still an untitled buffer: seeding it is not saving it.
        assert_eq!(doc.display_name(), "Untitled");
        assert_eq!(doc.path(), None);
        assert_eq!(doc.base_dir(), None);
    }

    #[test]
    fn a_scratch_document_goes_dirty_once_it_is_edited() {
        let mut doc = Document::scratch("# md\n");
        doc.set_text("# md\n\nand more\n");
        assert!(doc.is_dirty());

        // And clean again when the text is put back, which is what makes the
        // starter text a real baseline rather than a flag that was suppressed.
        doc.set_text("# md\n");
        assert!(!doc.is_dirty());
    }

    #[test]
    fn save_then_edit_tracks_dirty_state() {
        let dir = std::env::temp_dir().join(format!("md-core-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.md");

        let mut doc = Document::new();
        doc.set_text("# Title\n");
        doc.save_as(&path).unwrap();

        assert!(!doc.is_dirty());
        assert_eq!(doc.display_name(), "note.md");
        assert_eq!(doc.base_dir(), Some(dir.clone()));

        doc.set_text("# Title\n\nbody\n");
        assert!(doc.is_dirty());

        doc.revert().unwrap();
        assert!(!doc.is_dirty());
        assert_eq!(doc.text(), "# Title\n");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_without_path_is_an_error() {
        let mut doc = Document::new();
        doc.set_text("x");
        assert!(doc.save().is_err());
    }

    /// A buffer that has never been saved has no directory to resolve images
    /// against, and says so rather than guessing at the working directory.
    #[test]
    fn an_unsaved_document_has_no_base_directory() {
        let mut doc = Document::new();
        assert_eq!(doc.base_dir(), None);

        doc.set_text("![](a.png)");
        assert_eq!(doc.base_dir(), None);
    }

    /// Several untitled buffers open at once have to be told apart on the tab
    /// strip. The first is plain `Untitled`; the rest carry their number.
    #[test]
    fn numbering_an_untitled_document_shows_it_on_the_tab() {
        assert_eq!(Document::new().numbered(1).display_name(), "Untitled");
        assert_eq!(Document::new().numbered(2).display_name(), "Untitled 2");
        assert_eq!(Document::new().numbered(3).display_name(), "Untitled 3");

        // And the number is readable back, which is what lets the workspace
        // find the lowest one still free.
        assert_eq!(Document::new().numbered(2).untitled_number(), Some(2));
        assert_eq!(Document::new().untitled_number(), None);
    }

    /// A number is only for a buffer with no name. Once the document is saved
    /// it retires, so a later buffer can take the number without colliding
    /// with a file that is holding it.
    #[test]
    fn saving_retires_the_untitled_number() {
        let dir = std::env::temp_dir().join(format!("md-core-number-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.md");

        let mut doc = Document::new().numbered(2);
        assert_eq!(doc.display_name(), "Untitled 2");

        doc.save_as(&path).unwrap();
        assert_eq!(doc.display_name(), "note.md");
        assert_eq!(
            doc.untitled_number(),
            None,
            "a named file must not go on holding a number"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn repathing_follows_a_rename_without_touching_the_buffer() {
        let dir = std::env::temp_dir().join(format!("md-core-repath-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let before = dir.join("before.md");
        let after = dir.join("after.md");
        std::fs::write(&before, "# body\n").unwrap();

        let mut doc = Document::open(&before).unwrap();
        std::fs::rename(&before, &after).unwrap();
        doc.repath(&after);

        assert_eq!(doc.path(), Some(after.as_path()));
        assert_eq!(doc.display_name(), "after.md");
        assert_eq!(doc.text(), "# body\n");
        assert_eq!(doc.base_dir(), Some(dir.clone()));
        // The move did not make it dirty: the text still matches what was read.
        assert!(!doc.is_dirty());

        // Saving now writes to the new name. Without `repath` this would
        // recreate `before.md` and leave `after.md` untouched.
        doc.set_text("# edited\n");
        doc.save().unwrap();
        assert_eq!(std::fs::read_to_string(&after).unwrap(), "# edited\n");
        assert!(!before.exists());

        std::fs::remove_dir_all(&dir).ok();
    }
}
