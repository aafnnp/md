//! Resolving the image URLs in a document to files on disk.
//!
//! A Markdown image URL is relative to the document it appears in, not to the
//! process that renders it. The preview's built-in handling treats every URL as
//! an opaque resource id, so `![](diagram.png)` inside `~/notes/todo.md` asks
//! for `diagram.png` in the working directory and renders nothing. This module
//! is what turns that URL into `~/notes/diagram.png`.

use std::path::{Path, PathBuf};

/// Resolve one image URL against the directory its document lives in.
///
/// `None` means the URL is not ours to resolve and the caller should leave it
/// alone: an absolute path, a Windows rooted or UNC path, and anything carrying
/// a scheme (`https:`, `data:`, `file:`) all already mean something on their
/// own, and rewriting them would only break them.
pub fn resolve(base_dir: &Path, url: &str) -> Option<PathBuf> {
    let url = reference(url);
    if url.is_empty() {
        return None;
    }
    // A scheme is checked before absoluteness because `C:\shots\a.png` is an
    // absolute path that also looks like a scheme on every platform but Windows.
    if has_scheme(&url) {
        return None;
    }
    if Path::new(&url).is_absolute() {
        return None;
    }
    // Windows reads a leading `\` as rooted in the current drive, and a joined
    // path would silently replace the base rather than extend it. Backslash is a
    // legal file name character on Unix, where no such file exists in practice.
    if url.starts_with('\\') {
        return None;
    }
    Some(base_dir.join(percent_decode(&url)))
}

/// The URL with the parts that are not the file name taken off.
///
/// CommonMark allows a URL to be wrapped in angle brackets, and a fragment or
/// query is a position inside the file rather than part of its name — `a.png#L2`
/// and `a.png?raw=1` both name `a.png`.
fn reference(url: &str) -> String {
    let url = url.trim();
    let url = match url
        .strip_prefix('<')
        .and_then(|rest| rest.strip_suffix('>'))
    {
        Some(inner) => inner,
        None => url,
    };
    let end = url.find(['#', '?']).unwrap_or(url.len());
    url[..end].trim().to_string()
}

/// Whether `url` begins with a URL scheme — `alpha *( alpha / digit / "+" / "-"
/// / "." )` followed by a colon, per RFC 3986.
fn has_scheme(url: &str) -> bool {
    let Some(colon) = url.find(':') else {
        return false;
    };
    let scheme = &url[..colon];
    !scheme.is_empty()
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Undo the percent-encoding a URL uses for characters a file name cannot carry
/// literally: `my%20diagram.png` is the file `my diagram.png`.
///
/// `+` is deliberately left alone. It means a space in a form submission, not in
/// a path, and decoding it would rename every file that has a plus in it.
///
/// A `%` that is not followed by two hex digits is kept as written, since the
/// URL was going to name a file containing that `%`.
fn percent_decode(url: &str) -> String {
    let bytes = url.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escape = if bytes[i] == b'%' && i + 2 < bytes.len() {
            match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                (Some(high), Some(low)) => Some(high * 16 + low),
                _ => None,
            }
        } else {
            None
        };
        match escape {
            Some(byte) => {
                decoded.push(byte);
                i += 3;
            }
            None => {
                decoded.push(bytes[i]);
                i += 1;
            }
        }
    }
    // Encoded bytes are UTF-8, and one encoded character may span several of
    // them — `%E4%B8%AD` is 中.
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex(byte: u8) -> Option<u8> {
    (byte as char).to_digit(16).map(|digit| digit as u8)
}

#[cfg(test)]
mod tests {
    use super::resolve;
    use std::path::Path;

    fn notes(url: &str) -> Option<std::path::PathBuf> {
        resolve(Path::new("/home/me/notes"), url)
    }

    #[test]
    fn a_relative_url_resolves_against_the_document() {
        assert_eq!(
            notes("diagram.png"),
            Some(Path::new("/home/me/notes/diagram.png").to_path_buf())
        );
    }

    #[test]
    fn a_url_with_its_own_subdirectory_keeps_it() {
        assert_eq!(
            notes("img/diagram.png"),
            Some(Path::new("/home/me/notes/img/diagram.png").to_path_buf())
        );
        // `..` is a step up the tree, not an error.
        assert_eq!(
            notes("../shared/logo.png"),
            Some(Path::new("/home/me/notes/../shared/logo.png").to_path_buf())
        );
    }

    #[test]
    fn a_url_that_already_means_something_is_left_alone() {
        assert_eq!(notes("https://example.com/a.png"), None);
        assert_eq!(notes("http://example.com/a.png"), None);
        assert_eq!(notes("data:image/png;base64,iVBORw0KGgo="), None);
        assert_eq!(notes("file:///tmp/a.png"), None);
        // `a:b.png` is a scheme too — `a` is a perfectly good one — so a colon
        // only stops being a scheme once the text before it cannot be one.
        assert_eq!(notes("a:b.png"), None);
        assert_eq!(
            notes("shots/a:b.png"),
            Some(Path::new("/home/me/notes/shots/a:b.png").to_path_buf())
        );
    }

    #[test]
    fn an_absolute_path_is_left_alone() {
        assert_eq!(notes("/var/shots/a.png"), None);
    }

    /// Joining a Windows rooted path would replace the base directory instead of
    /// extending it, so it is one of the cases that has to be refused rather
    /// than approximated.
    #[test]
    fn a_windows_rooted_or_unc_path_is_left_alone() {
        assert_eq!(notes(r"\shots\a.png"), None);
        assert_eq!(notes(r"\\server\share\a.png"), None);
        assert_eq!(notes(r"C:\shots\a.png"), None);
    }

    #[test]
    fn a_fragment_or_query_is_not_part_of_the_file_name() {
        assert_eq!(
            notes("a.png#L2"),
            Some(Path::new("/home/me/notes/a.png").to_path_buf())
        );
        assert_eq!(
            notes("a.png?raw=1"),
            Some(Path::new("/home/me/notes/a.png").to_path_buf())
        );
        // ...whichever comes first wins.
        assert_eq!(
            notes("a.png?x=1#top"),
            Some(Path::new("/home/me/notes/a.png").to_path_buf())
        );
    }

    #[test]
    fn an_angle_bracketed_url_is_unwrapped() {
        assert_eq!(
            notes("<my diagram.png>"),
            Some(Path::new("/home/me/notes/my diagram.png").to_path_buf())
        );
    }

    #[test]
    fn percent_encoding_is_decoded() {
        assert_eq!(
            notes("my%20diagram.png"),
            Some(Path::new("/home/me/notes/my diagram.png").to_path_buf())
        );
        // One character, several encoded bytes.
        assert_eq!(
            notes("%E4%B8%AD%E6%96%87.png"),
            Some(Path::new("/home/me/notes/中文.png").to_path_buf())
        );
    }

    /// `+` means a space in a form body, not in a path. Decoding it here would
    /// silently point at a different file than the one that exists.
    #[test]
    fn a_plus_is_a_plus() {
        assert_eq!(
            notes("a+b.png"),
            Some(Path::new("/home/me/notes/a+b.png").to_path_buf())
        );
    }

    #[test]
    fn a_percent_that_does_not_encode_anything_is_kept() {
        assert_eq!(
            notes("100%.png"),
            Some(Path::new("/home/me/notes/100%.png").to_path_buf())
        );
        assert_eq!(
            notes("a%zz.png"),
            Some(Path::new("/home/me/notes/a%zz.png").to_path_buf())
        );
        // A truncated escape at the very end has no second hex digit to read.
        assert_eq!(
            notes("a%4"),
            Some(Path::new("/home/me/notes/a%4").to_path_buf())
        );
    }

    #[test]
    fn a_url_with_no_file_in_it_has_nothing_to_resolve() {
        assert_eq!(notes(""), None);
        assert_eq!(notes("   "), None);
        assert_eq!(notes("#top"), None);
        assert_eq!(notes("?raw=1"), None);
    }

    #[test]
    fn surrounding_whitespace_is_not_part_of_the_name() {
        assert_eq!(
            notes("  a.png  "),
            Some(Path::new("/home/me/notes/a.png").to_path_buf())
        );
    }
}
