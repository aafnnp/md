//! The recently-opened file list.
//!
//! Pure data: most-recent-first, de-duplicated by path, and capped. The one
//! method that looks at disk, [`RecentFiles::retain_existing`], is separated
//! from the ordering rules so the ordering can be tested without it.
//!
//! The list lives in its own file rather than in `settings.json`. That file is
//! meant to be read and edited by hand, and twenty paths are noise in it —
//! besides which the settings file is written only when a setting changes,
//! while this one changes every time a file is opened.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::settings::config_dir;

/// The file name inside the platform's configuration directory.
pub const FILE_NAME: &str = "recent.json";

/// How many entries the list keeps.
const CAPACITY: usize = 20;

/// Recently-opened files, most recent first.
///
/// Written as the bare array rather than as an object wrapping one, so the
/// file reads as the list of paths it is.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
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
    ///
    /// A path that is not valid UTF-8 is not recorded at all. The list is
    /// stored as JSON, which cannot spell one, and a single entry that cannot
    /// be written would cost every other entry its place on every save from
    /// then on. Losing the one is the smaller loss.
    pub fn push(&mut self, path: impl Into<PathBuf>) {
        let path = path.into();
        if path.to_str().is_none() {
            return;
        }
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

    /// Force the list back into the shape it is meant to be in: no duplicates,
    /// most recent first as written, and no more than [`CAPACITY`] entries.
    ///
    /// [`push`](Self::push) maintains all three by construction. This is for
    /// the other way in — a file that was edited by hand, or written by a
    /// version whose capacity was larger.
    pub fn normalize(&mut self) {
        let mut unique: Vec<PathBuf> = Vec::with_capacity(self.entries.len());
        for path in std::mem::take(&mut self.entries) {
            if !unique.contains(&path) {
                unique.push(path);
            }
        }
        unique.truncate(CAPACITY);
        self.entries = unique;
    }

    /// Read the list, falling back to an empty one.
    ///
    /// The same bargain as the settings file: a missing file is a first run,
    /// and a file that cannot be read or parsed must not keep the app from
    /// starting. The difference is what the failure costs, which here is only
    /// a convenience.
    pub fn load() -> Self {
        match recent_path() {
            Some(path) => Self::load_from(&path),
            None => Self::default(),
        }
    }

    /// Read the list from `path`.
    ///
    /// Split from [`Self::load`] so the tests do not have to write into the
    /// user's real configuration directory.
    pub fn load_from(path: &Path) -> Self {
        let mut recent = fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Self>(&text).ok())
            .unwrap_or_default();
        recent.normalize();
        recent
    }

    /// Write the list to the platform's configuration file.
    pub fn save(&self) -> io::Result<()> {
        let path = recent_path().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "this platform has no per-user configuration directory",
            )
        })?;
        self.save_to(&path)
    }

    /// Write the list to `path`.
    ///
    /// An empty list removes the file instead of writing `[]`. The file exists
    /// to remember paths, and having none to remember is better said by there
    /// being no file than by a document that says nothing.
    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        if self.is_empty() {
            return match fs::remove_file(path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                other => other,
            };
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut text = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        text.push('\n');
        fs::write(path, text)
    }
}

/// Where the recent-files list lives, beside the settings file.
pub fn recent_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join(FILE_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory private to one test, so the tests can run in
    /// parallel without treading on each other's files.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("md-core-recent-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

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
        let dir = scratch("retain");
        let kept = dir.join("kept.md");
        std::fs::write(&kept, "x").unwrap();

        let mut recent = RecentFiles::new();
        recent.push(kept.clone());
        recent.push(dir.join("gone.md"));

        recent.retain_existing();

        assert_eq!(recent.entries(), [kept]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn normalizing_drops_duplicates_and_keeps_the_first_copy() {
        let mut recent = RecentFiles::new();
        // What a hand-edited file could say. `push` cannot produce this, which
        // is the whole reason the two are separate.
        recent.entries = vec!["/a.md".into(), "/b.md".into(), "/a.md".into()];

        recent.normalize();

        assert_eq!(
            recent.entries(),
            [PathBuf::from("/a.md"), PathBuf::from("/b.md")]
        );
    }

    #[test]
    fn normalizing_trims_a_list_longer_than_the_capacity() {
        let mut recent = RecentFiles::new();
        recent.entries = (0..CAPACITY + 5)
            .map(|i| PathBuf::from(format!("/file-{i}.md")))
            .collect();

        recent.normalize();

        assert_eq!(recent.len(), CAPACITY);
        // Trimming keeps the front, which is the end that means "most recent".
        assert_eq!(recent.entries()[0], PathBuf::from("/file-0.md"));
    }

    #[test]
    fn the_list_round_trips_through_a_file() {
        let dir = scratch("round-trip");
        let path = dir.join(FILE_NAME);
        let mut recent = RecentFiles::new();
        recent.push("/a.md");
        recent.push("/b.md");

        recent.save_to(&path).unwrap();

        assert_eq!(RecentFiles::load_from(&path), recent);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_file_that_is_not_a_list_of_paths_loads_as_empty() {
        let dir = scratch("corrupt");
        let path = dir.join(FILE_NAME);

        assert_eq!(RecentFiles::load_from(&path), RecentFiles::new());
        assert_eq!(RecentFiles::load_from(&dir), RecentFiles::new());

        std::fs::write(&path, "{ not json at all").unwrap();
        assert_eq!(RecentFiles::load_from(&path), RecentFiles::new());

        // Valid JSON that is the wrong shape.
        std::fs::write(&path, r#"{"entries": ["/a.md"]}"#).unwrap();
        assert_eq!(RecentFiles::load_from(&path), RecentFiles::new());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn loading_puts_a_file_that_was_edited_by_hand_back_in_shape() {
        let dir = scratch("hand-edited");
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, r#"["/a.md", "/a.md", "/b.md"]"#).unwrap();

        let recent = RecentFiles::load_from(&path);

        assert_eq!(
            recent.entries(),
            [PathBuf::from("/a.md"), PathBuf::from("/b.md")]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn saving_an_empty_list_leaves_no_file_behind() {
        let dir = scratch("empty");
        let path = dir.join(FILE_NAME);
        let mut recent = RecentFiles::new();
        recent.push("/a.md");
        recent.save_to(&path).unwrap();
        assert!(path.exists());

        recent.clear();
        recent.save_to(&path).unwrap();

        assert!(!path.exists());
        // Saving again over a file that is already gone is not a failure.
        recent.save_to(&path).unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_cannot_be_written_as_json_is_not_recorded() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let mut recent = RecentFiles::new();
        recent.push("/fine.md");
        recent.push(PathBuf::from(OsStr::from_bytes(b"/not-\xff-utf8.md")));

        // The entry that could be named is kept, and the one that could not is
        // not — rather than the whole list failing to save from here on.
        assert_eq!(recent.entries(), [PathBuf::from("/fine.md")]);
    }

    #[test]
    fn the_recent_file_sits_beside_the_settings_file() {
        // The platform cannot always name a directory, so this checks the
        // relationship rather than a path that differs from machine to machine.
        if let (Some(recent), Some(settings)) = (recent_path(), crate::settings::settings_path()) {
            assert!(recent.is_absolute());
            assert_eq!(
                recent.file_name().and_then(|name| name.to_str()),
                Some(FILE_NAME)
            );
            assert_eq!(recent.parent(), settings.parent());
        }
    }
}
