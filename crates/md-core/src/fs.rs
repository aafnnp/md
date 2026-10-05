//! The filesystem operations a file tree performs.
//!
//! Each of these acts on a single file or directory that the tree already
//! lists. None of them recurses, and none of them can be pointed outside the
//! directory it was handed: a proposed name is validated rather than joined
//! blindly, so a name carrying a path separator is refused instead of silently
//! escaping the folder the user picked.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// The extensions the editor treats as Markdown.
pub const MARKDOWN_EXTENSIONS: [&str; 3] = ["md", "markdown", "mdx"];

/// Why a proposed name was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameError {
    /// Nothing but whitespace.
    Empty,
    /// A path separator, the names `.` and `..`, or a trailing dot or space —
    /// the last of which Windows silently strips, leaving the file under a name
    /// the user did not ask for.
    NotAFileName,
    /// Starts with a dot, which would hide it from the tree that just offered
    /// to create it.
    Hidden,
    /// Has an extension, but not one the tree lists, so the result would not
    /// appear in the tree it was created from.
    NotMarkdown,
}

impl NameError {
    /// A sentence to show the user.
    pub fn message(self) -> &'static str {
        match self {
            Self::Empty => "Enter a name.",
            Self::NotAFileName => {
                "A name cannot contain “/” or “\\”, and cannot end with a dot or a space."
            }
            Self::Hidden => "A name cannot start with a dot.",
            Self::NotMarkdown => "Only .md, .markdown and .mdx files are listed here.",
        }
    }
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for NameError {}

/// Why a file operation failed.
#[derive(Debug)]
pub enum FileOpError {
    /// The proposed name was not usable.
    Name(NameError),
    /// The filesystem refused.
    Io(io::Error),
}

impl fmt::Display for FileOpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name(error) => write!(f, "{error}"),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for FileOpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Name(error) => Some(error),
            Self::Io(error) => Some(error),
        }
    }
}

impl From<NameError> for FileOpError {
    fn from(error: NameError) -> Self {
        Self::Name(error)
    }
}

impl From<io::Error> for FileOpError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<&NameError> for FileOpError {
    fn from(error: &NameError) -> Self {
        Self::Name(*error)
    }
}

/// Whether `path` names a Markdown file, by extension and case-insensitively.
pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            let extension = extension.to_lowercase();
            MARKDOWN_EXTENSIONS.contains(&extension.as_str())
        })
}

/// Check a single path component — one name, not a path.
///
/// This is the guard that keeps a name inside the directory it was typed in.
fn check_name(name: &str) -> Result<&str, NameError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if name == "." || name == ".." {
        return Err(NameError::NotAFileName);
    }
    if name.contains(['/', '\\']) || name.ends_with(['.', ' ']) {
        return Err(NameError::NotAFileName);
    }
    if name.starts_with('.') {
        return Err(NameError::Hidden);
    }
    Ok(name)
}

/// The file name `name` should become.
///
/// A name with no extension at all gains `.md`, so that what the user typed
/// is what they meant — nobody creating a note means a file with no
/// extension. A name with some *other* extension is refused rather than
/// rewritten, because silently turning `notes.txt` into `notes.txt.md` would
/// not be what was asked for either.
pub fn resolve_file_name(name: &str) -> Result<String, NameError> {
    let name = check_name(name)?;
    match Path::new(name).extension() {
        None => Ok(format!("{name}.md")),
        Some(_) if is_markdown(Path::new(name)) => Ok(name.to_string()),
        Some(_) => Err(NameError::NotMarkdown),
    }
}

/// The directory name `name` should become. Directories have no extension
/// convention to enforce, so this only rejects what is not a plain name.
pub fn resolve_directory_name(name: &str) -> Result<String, NameError> {
    Ok(check_name(name)?.to_string())
}

/// Create an empty Markdown file called `name` inside `directory`.
pub fn create_file(directory: &Path, name: &str) -> Result<PathBuf, FileOpError> {
    let name = resolve_file_name(name)?;
    let path = directory.join(&name);
    if path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("“{name}” already exists"),
        )
        .into());
    }
    // `create_new` rather than the existence check above alone: the check makes
    // the message readable, and this is what actually guarantees the file was
    // not there a moment ago.
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    Ok(path)
}

/// Rename `path` to `name`, in the directory it already sits in.
///
/// `as_directory` picks which set of naming rules to apply; the file and
/// directory rules differ only in how they treat extensions.
pub fn rename(path: &Path, name: &str, as_directory: bool) -> Result<PathBuf, FileOpError> {
    let name = if as_directory {
        resolve_directory_name(name)?
    } else {
        resolve_file_name(name)?
    };
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "that file has no parent directory to rename it within",
            )
        })?;

    let target = parent.join(&name);
    if target == path {
        // Renamed to what it already was. Succeeding without touching the
        // filesystem is not just an optimisation: some filesystems would
        // replace the file, and the user asked for no change.
        return Ok(target);
    }
    if target.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("“{name}” already exists"),
        )
        .into());
    }
    std::fs::rename(path, &target)?;
    Ok(target)
}

/// Delete a single file.
///
/// Refuses a directory outright. Deleting a folder from a context menu is one
/// stray click away from removing work that is not in the tree — the tree
/// lists Markdown files, but the directory holds everything else too — and
/// there is no undo. `remove_file` would fail on a directory on most
/// platforms anyway; this makes the refusal the point rather than a side
/// effect.
pub fn delete_file(path: &Path) -> Result<(), FileOpError> {
    if path.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "refusing to delete a directory",
        )
        .into());
    }
    std::fs::remove_file(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory private to one test, so the tests can run in
    /// parallel without treading on each other's files.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("md-core-fs-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn markdown_extensions_are_recognised_regardless_of_case() {
        assert!(is_markdown(Path::new("a.md")));
        assert!(is_markdown(Path::new("a.MD")));
        assert!(is_markdown(Path::new("a.markdown")));
        assert!(is_markdown(Path::new("a.mdx")));
        assert!(!is_markdown(Path::new("a.txt")));
        assert!(!is_markdown(Path::new("a")));
        assert!(!is_markdown(Path::new("a.md.bak")));
    }

    #[test]
    fn a_bare_name_gains_the_markdown_extension() {
        assert_eq!(resolve_file_name("notes").unwrap(), "notes.md");
        assert_eq!(resolve_file_name("  notes  ").unwrap(), "notes.md");
        assert_eq!(resolve_file_name("notes.md").unwrap(), "notes.md");
        assert_eq!(
            resolve_file_name("Notes.MARKDOWN").unwrap(),
            "Notes.MARKDOWN"
        );
    }

    #[test]
    fn a_name_that_would_escape_the_directory_is_refused() {
        // The point of validating rather than joining: each of these would
        // otherwise name a file outside the folder the user right-clicked.
        assert_eq!(
            resolve_file_name("../escape.md"),
            Err(NameError::NotAFileName)
        );
        assert_eq!(
            resolve_file_name("nested/escape.md"),
            Err(NameError::NotAFileName)
        );
        assert_eq!(
            resolve_file_name("nested\\escape.md"),
            Err(NameError::NotAFileName)
        );
        assert_eq!(resolve_file_name(".."), Err(NameError::NotAFileName));
        assert_eq!(resolve_file_name("."), Err(NameError::NotAFileName));
        // A trailing dot is stripped by Windows, so the file would land under a
        // name the user never typed. A trailing *space* is trimmed instead —
        // see `a_bare_name_gains_the_markdown_extension` — because that one is
        // unambiguous.
        assert_eq!(resolve_file_name("bad."), Err(NameError::NotAFileName));
    }

    #[test]
    fn empty_and_hidden_and_foreign_names_are_refused() {
        assert_eq!(resolve_file_name("   "), Err(NameError::Empty));
        assert_eq!(resolve_file_name(".hidden.md"), Err(NameError::Hidden));
        assert_eq!(resolve_file_name("notes.txt"), Err(NameError::NotMarkdown));
        assert_eq!(resolve_file_name("v1.2"), Err(NameError::NotMarkdown));
    }

    #[test]
    fn directory_names_are_not_given_an_extension() {
        assert_eq!(resolve_directory_name("drafts").unwrap(), "drafts");
        // A directory called `x.txt` is fine; only files have an extension
        // convention to keep the tree's listing honest about.
        assert_eq!(resolve_directory_name("x.txt").unwrap(), "x.txt");
        assert_eq!(resolve_directory_name("a/b"), Err(NameError::NotAFileName));
        assert_eq!(resolve_directory_name(""), Err(NameError::Empty));
    }

    #[test]
    fn creating_a_file_leaves_it_empty_and_on_disk() {
        let dir = scratch("create");
        let path = create_file(&dir, "note").unwrap();
        assert_eq!(path, dir.join("note.md"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");

        // The second attempt is the interesting one: creating over an existing
        // file must not truncate it.
        std::fs::write(&path, "important").unwrap();
        let error = create_file(&dir, "note.md").unwrap_err();
        assert!(
            matches!(error, FileOpError::Io(ref e) if e.kind() == io::ErrorKind::AlreadyExists)
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "important");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn renaming_moves_the_file_and_keeps_its_contents() {
        let dir = scratch("rename");
        std::fs::write(dir.join("old.md"), "# body").unwrap();

        let moved = rename(&dir.join("old.md"), "new", false).unwrap();
        assert_eq!(moved, dir.join("new.md"));
        assert_eq!(std::fs::read_to_string(&moved).unwrap(), "# body");
        assert!(!dir.join("old.md").exists());

        // Renaming to the name it already has is a no-op that succeeds, and
        // leaves the file alone.
        assert_eq!(rename(&moved, "new.md", false).unwrap(), moved);
        assert!(moved.exists());

        // Renaming onto an existing file would destroy it, so it is refused.
        std::fs::write(dir.join("other.md"), "other").unwrap();
        let error = rename(&moved, "other.md", false).unwrap_err();
        assert!(
            matches!(error, FileOpError::Io(ref e) if e.kind() == io::ErrorKind::AlreadyExists)
        );
        assert_eq!(std::fs::read_to_string(&moved).unwrap(), "# body");
        assert_eq!(
            std::fs::read_to_string(dir.join("other.md")).unwrap(),
            "other"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_directory_is_renamed_without_gaining_an_extension() {
        let dir = scratch("rename-dir");
        std::fs::create_dir(dir.join("drafts")).unwrap();
        std::fs::write(dir.join("drafts").join("a.md"), "# a").unwrap();

        let moved = rename(&dir.join("drafts"), "notes", true).unwrap();
        assert_eq!(moved, dir.join("notes"));
        assert!(moved.is_dir());
        // Whatever was inside came along.
        assert!(moved.join("a.md").exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn deleting_removes_a_file_but_refuses_a_directory() {
        let dir = scratch("delete");
        let file = dir.join("gone.md");
        std::fs::write(&file, "# gone").unwrap();
        delete_file(&file).unwrap();
        assert!(!file.exists());

        // Even an empty directory is refused: nothing here recurses, and the
        // tree's one dangerous menu item should not be one click from a
        // directory the user only meant to look at.
        std::fs::create_dir(dir.join("keep")).unwrap();
        let error = delete_file(&dir.join("keep")).unwrap_err();
        assert!(matches!(error, FileOpError::Io(ref e) if e.kind() == io::ErrorKind::InvalidInput));
        assert!(dir.join("keep").exists());

        std::fs::remove_dir_all(&dir).ok();
    }
}
