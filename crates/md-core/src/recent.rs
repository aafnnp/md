//! The recently-opened file list.
//!
//! Pure data: most-recent-first, de-duplicated by path, and capped. Persisting
//! the list is the caller's job, which keeps this testable without touching the
//! filesystem — the one method that does look at disk, [`RecentFiles::retain_existing`],
//! is separated out so the ordering rules can be tested without it.

use std::path::{Path, PathBuf};

/// How many entries the list keeps.
const CAPACITY: usize = 20;

/// Recently-opened files, most recent first.
#[derive(Debug, Default, Clone)]
pub struct RecentFiles {
    entries: Vec<PathBuf>,
}

impl RecentFiles {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> &[PathBuf] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Record `path` as the most recently opened file.
    ///
    /// Re-opening something already in the list moves it to the front rather
    /// than adding a second copy, and the list is trimmed back to [`CAPACITY`].
    pub fn push(&mut self, path: impl Into<PathBuf>) {
        let path = path.into();
        self.entries.retain(|existing| existing != &path);
        self.entries.insert(0, path);
        self.entries.truncate(CAPACITY);
    }

    pub fn remove(&mut self, path: &Path) {
        self.entries.retain(|existing| existing != path);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Drop entries that no longer exist, as happens when a file is moved or
    /// deleted between sessions.
    pub fn retain_existing(&mut self) {
        self.entries.retain(|path| path.is_file());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_keeps_most_recent_first() {
        let mut recent = RecentFiles::new();
        recent.push("/a.md");
        recent.push("/b.md");

        assert_eq!(
            recent.entries(),
            [PathBuf::from("/b.md"), PathBuf::from("/a.md")]
        );
    }

    #[test]
    fn reopening_moves_to_front_without_duplicating() {
        let mut recent = RecentFiles::new();
        recent.push("/a.md");
        recent.push("/b.md");
        recent.push("/a.md");

        assert_eq!(recent.len(), 2);
        assert_eq!(recent.entries()[0], PathBuf::from("/a.md"));
    }

    #[test]
    fn list_is_capped_dropping_the_oldest() {
        let mut recent = RecentFiles::new();
        for i in 0..CAPACITY + 5 {
            recent.push(format!("/file-{i}.md"));
        }

        assert_eq!(recent.len(), CAPACITY);
        // The newest survived and the oldest fell off the end.
        assert_eq!(
            recent.entries()[0],
            PathBuf::from(format!("/file-{}.md", CAPACITY + 4))
        );
        assert!(!recent.entries().contains(&PathBuf::from("/file-0.md")));
    }

    #[test]
    fn remove_drops_only_the_named_entry() {
        let mut recent = RecentFiles::new();
        recent.push("/a.md");
        recent.push("/b.md");
        recent.remove(Path::new("/a.md"));

        assert_eq!(recent.entries(), [PathBuf::from("/b.md")]);
    }

    #[test]
    fn retain_existing_forgets_deleted_files() {
        let dir = std::env::temp_dir().join(format!("md-recent-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let kept = dir.join("kept.md");
        std::fs::write(&kept, "x").unwrap();

        let mut recent = RecentFiles::new();
        recent.push(kept.clone());
        recent.push(dir.join("gone.md"));

        recent.retain_existing();

        assert_eq!(recent.entries(), [kept]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
