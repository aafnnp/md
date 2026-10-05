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

    /// The directory used to resolve relative image and link paths in the
    /// preview. Falls back to the working directory for unsaved documents.
    pub fn base_dir(&self) -> PathBuf {
        self.path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
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
        assert_eq!(doc.base_dir(), dir);

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
}
