//! How much of a document there is, for the status bar to report.
//!
//! Two numbers, both of which are well defined for any text:
//!
//! - **Characters**, counted as Unicode scalar values — what `chars()` gives.
//!   Not bytes, which would report three for `中`, and not grapheme clusters,
//!   which would report one for an emoji with a skin-tone modifier and one for
//!   an `e` with an accent: both are defensible, and both need a table of
//!   Unicode properties to be right. Scalar values need nothing and are never
//!   surprising for the languages this editor is for. An emoji that is one
//!   picture may therefore count as two, which is the one thing to know about
//!   the number.
//! - **Lines**, counted by `str::lines()`. An empty document is zero lines, a
//!   trailing newline does not add an empty one after it, and `\r\n` counts
//!   once. A file saved by any editor ends in a newline, so counting the
//!   newlines themselves would report one line fewer than the file has — which
//!   is why this is not `wc -l`.
//!
//! There is deliberately no word count. Splitting on whitespace reports `一篇文章`
//! as one word, so the number would be badly wrong for exactly the documents
//! this editor is built to write, and a number that is wrong in a way the reader
//! cannot see is worse than no number.

/// The size of a document, as the status bar shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    /// Unicode scalar values in the text.
    pub characters: usize,
    /// Lines in the text.
    pub lines: usize,
}

/// Count the text.
pub fn counts(text: &str) -> Counts {
    Counts {
        characters: text.chars().count(),
        lines: text.lines().count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_document_is_empty() {
        assert_eq!(
            counts(""),
            Counts {
                characters: 0,
                lines: 0
            }
        );
    }

    #[test]
    fn a_trailing_newline_does_not_add_a_line() {
        // What every file written by an editor looks like. Counting newlines
        // would call this two lines; it is one.
        assert_eq!(counts("# Title\n").lines, 1);
        assert_eq!(counts("# Title\n\nbody\n").lines, 3);
        assert_eq!(counts("# Title\n\nbody").lines, 3);
    }

    #[test]
    fn a_blank_line_between_text_still_counts() {
        assert_eq!(counts("\n").lines, 1);
        assert_eq!(counts("a\n\n\nb").lines, 4);
    }

    #[test]
    fn a_carriage_return_does_not_start_a_line_of_its_own() {
        // A file written on Windows reads as the same number of lines here.
        assert_eq!(counts("a\r\nb\r\n").lines, 2);
        assert_eq!(counts("a\r\nb\r\n").characters, 6);
    }

    #[test]
    fn characters_are_counted_rather_than_bytes() {
        // Four characters, twelve bytes.
        assert_eq!(counts("中文字符").characters, 4);
        assert_eq!(counts("café").characters, 4);
        // Accents written as a combining mark count twice: see the module note.
        assert_eq!(counts("cafe\u{301}").characters, 5);
    }

    #[test]
    fn the_counts_move_with_the_text() {
        assert_eq!(counts("hello").characters, 5);
        assert_eq!(counts("hello\nworld").characters, 11);
        assert_eq!(counts("hello\nworld").lines, 2);
    }
}
