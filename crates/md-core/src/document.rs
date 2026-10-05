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
}

impl Document {
    /// A new, empty, never-saved document.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load a document from disk.
    pub fn open(path: impl Into<PathBuf>) -> std::io::Result<Self> {
        let path = path.into();
        let text = std::fs::read_to_string(&path)?;
        Ok(Self {
            path: Some(path),
            saved_text: text.clone(),
            text,
        })
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
    pub fn display_name(&self) -> String {
        self.path
            .as_deref()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_string())
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
