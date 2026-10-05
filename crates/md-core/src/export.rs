//! Markdown to HTML, for the export command.
//!
//! The parser is `markdown` — the same crate the preview renders with — so a
//! table, a task list, or a strikethrough is the same construct in both. What
//! the preview shows and what the exported file contains cannot drift apart
//! without the parser changing under both.

use std::io;
use std::path::Path;

use markdown::{Options, to_html_with_options};

/// The body of `markdown` as HTML, without an enclosing document.
///
/// GFM is on: tables, task lists, strikethrough, autolinks and footnotes all
/// come out as their HTML equivalents.
pub fn to_html_fragment(markdown: &str) -> String {
    // `to_html_with_options` returns a `Result` for MDX, which has syntax
    // errors a plain Markdown document cannot have. MDX is off, so the error
    // branch is unreachable rather than merely unlikely.
    to_html_with_options(markdown, &Options::gfm())
        .expect("plain Markdown has no syntax errors; only MDX reports them")
}

/// A complete HTML page: the rendered body, wrapped in a document that stands
/// on its own in a browser.
///
/// The wrapper is not decoration. Without `<meta charset>` a browser guesses
/// the encoding, and a file written as UTF-8 by an editor that does not guess
/// correctly — which is every CJK document — opens as mojibake. Without the
/// stylesheet a table loses its borders and code loses its background, so the
/// export would be visibly worse than the preview it is supposed to match.
pub fn to_html_page(title: &str, markdown: &str) -> String {
    format!(
        "<!DOCTYPE html>\n\
         <html>\n\
         <head>\n\
         <meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <meta name=\"generator\" content=\"md\">\n\
         <title>{title}</title>\n\
         <style>\n{STYLESHEET}</style>\n\
         </head>\n\
         <body>\n\
         <main>\n\
         {body}</main>\n\
         </body>\n\
         </html>\n",
        title = escape(title),
        body = to_html_fragment(markdown),
    )
}

/// Render `markdown` and write the page to `path`.
pub fn export_html(path: &Path, title: &str, markdown: &str) -> io::Result<()> {
    std::fs::write(path, to_html_page(title, markdown))
}

/// The fixed name of the exported file, before the user gets a chance to
/// change it: `notes.md` exports to `notes.html`.
///
/// A document with no file behind it — an untitled buffer — exports to
/// `untitled.html` rather than to `.html`, which is not a file name a platform
/// save dialog will accept.
pub fn suggested_file_name(document_name: &str) -> String {
    let stem = Path::new(document_name)
        .file_stem()
        .filter(|stem| !stem.is_empty())
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "untitled".to_string());
    format!("{stem}.html")
}

/// Escape the five characters that would otherwise be markup.
///
/// The title comes from a file name, so it is the user's text, not ours.
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// Enough styling for the page to read the way the preview does, in both light
/// and dark, with no network access and no external files.
const STYLESHEET: &str = "\
  :root { color-scheme: light dark; }\n\
  body {\n\
    margin: 0;\n\
    font-family: -apple-system, BlinkMacSystemFont, \"Segoe UI\", Helvetica, Arial, sans-serif;\n\
    font-size: 16px;\n\
    line-height: 1.6;\n\
    color: #1f2328;\n\
    background: #ffffff;\n\
  }\n\
  main { max-width: 860px; margin: 0 auto; padding: 2.5rem 1.25rem 5rem; }\n\
  h1, h2, h3, h4, h5, h6 { line-height: 1.25; margin: 1.6em 0 0.6em; }\n\
  h1 { font-size: 2em; border-bottom: 1px solid #d8dee4; padding-bottom: 0.3em; }\n\
  h2 { font-size: 1.5em; border-bottom: 1px solid #d8dee4; padding-bottom: 0.3em; }\n\
  a { color: #0969da; }\n\
  code, pre { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 0.9em; }\n\
  code { background: #f0f1f3; border-radius: 4px; padding: 0.15em 0.35em; }\n\
  pre { background: #f6f8fa; border-radius: 6px; padding: 1rem; overflow-x: auto; }\n\
  pre code { background: none; padding: 0; }\n\
  blockquote { margin: 1em 0; padding: 0 1em; color: #59636e; border-left: 4px solid #d0d7de; }\n\
  table { border-collapse: collapse; display: block; overflow-x: auto; margin: 1em 0; }\n\
  th, td { border: 1px solid #d0d7de; padding: 0.4em 0.8em; }\n\
  th { background: #f6f8fa; }\n\
  hr { border: 0; border-top: 1px solid #d0d7de; margin: 2em 0; }\n\
  img { max-width: 100%; }\n\
  ul.contains-task-list { list-style: none; padding-left: 1.4em; }\n\
  @media (prefers-color-scheme: dark) {\n\
    body { color: #e6edf3; background: #0d1117; }\n\
    h1, h2 { border-bottom-color: #30363d; }\n\
    a { color: #4493f8; }\n\
    code { background: #161b22; }\n\
    pre { background: #161b22; }\n\
    blockquote { color: #9198a1; border-left-color: #30363d; }\n\
    th, td { border-color: #30363d; }\n\
    th { background: #161b22; }\n\
    hr { border-top-color: #30363d; }\n\
  }\n";

#[cfg(test)]
mod tests {
    use super::{escape, export_html, suggested_file_name, to_html_fragment, to_html_page};

    fn temp_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("md-core-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn headings_and_paragraphs_become_their_html_equivalents() {
        assert_eq!(to_html_fragment("# Title"), "<h1>Title</h1>");
        assert_eq!(
            to_html_fragment("Hello *world*."),
            "<p>Hello <em>world</em>.</p>"
        );
    }

    /// The point of export: a GFM construct the preview renders has to render
    /// in the file too, not fall back to literal pipes and dashes.
    #[test]
    fn gfm_tables_export_as_tables() {
        let html = to_html_fragment("| a | b |\n| - | - |\n| 1 | 2 |\n");
        assert!(html.contains("<table>"), "{html}");
        assert!(html.contains("<th>a</th>"), "{html}");
        assert!(html.contains("<td>1</td>"), "{html}");
    }

    #[test]
    fn gfm_task_lists_export_as_checkboxes() {
        let html = to_html_fragment("- [x] done\n- [ ] todo\n");
        assert!(html.contains("type=\"checkbox\""), "{html}");
        assert!(html.contains("checked"), "{html}");
        assert!(html.contains("todo"), "{html}");
    }

    #[test]
    fn gfm_strikethrough_exports_as_del() {
        assert_eq!(to_html_fragment("~~gone~~"), "<p><del>gone</del></p>");
    }

    /// Raw HTML in the source is data, not markup. Escaping it is also what
    /// keeps an exported file from running a script the document merely
    /// mentions.
    #[test]
    fn raw_html_in_the_source_is_escaped_rather_than_emitted() {
        let html = to_html_fragment("<script>alert(1)</script>");
        assert!(!html.contains("<script>"), "{html}");
        assert!(html.contains("&lt;script&gt;"), "{html}");
    }

    #[test]
    fn the_page_declares_utf8_so_non_ascii_text_survives_a_browser() {
        let page = to_html_page("笔记", "# 你好，世界\n");
        assert!(page.contains("<meta charset=\"utf-8\">"), "{page}");
        assert!(page.contains("<h1>你好，世界</h1>"), "{page}");
        // The title is the user's file name, so it is escaped like any other.
        assert!(page.contains("<title>笔记</title>"), "{page}");
    }

    #[test]
    fn a_title_containing_markup_is_escaped() {
        let page = to_html_page("a & b <c>", "");
        assert!(
            page.contains("<title>a &amp; b &lt;c&gt;</title>"),
            "{page}"
        );
    }

    #[test]
    fn the_page_is_a_complete_document() {
        let page = to_html_page("note", "text");
        assert!(page.starts_with("<!DOCTYPE html>"), "{page}");
        assert!(page.contains("<html>"), "{page}");
        assert!(page.contains("</html>"), "{page}");
        assert!(page.contains("<body>"), "{page}");
    }

    #[test]
    fn exporting_writes_the_page_to_the_given_path() {
        let path = temp_path("note.html");
        export_html(&path, "note", "# Hi").unwrap();

        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("<h1>Hi</h1>"), "{written}");
        assert_eq!(written, to_html_page("note", "# Hi"));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_markdown_file_name_becomes_the_same_name_with_an_html_extension() {
        assert_eq!(suggested_file_name("notes.md"), "notes.html");
        assert_eq!(suggested_file_name("a.b.markdown"), "a.b.html");
        // A file the user named without an extension still gets one, rather
        // than exporting to a file with no name at all.
        assert_eq!(suggested_file_name("notes"), "notes.html");
        // A buffer that has never been saved has no name to borrow.
        assert_eq!(suggested_file_name(""), "untitled.html");
    }

    #[test]
    fn escaping_covers_the_characters_that_close_a_title_element() {
        assert_eq!(escape("</title><script>"), "&lt;/title&gt;&lt;script&gt;");
        assert_eq!(escape("it's \"quoted\""), "it&#39;s &quot;quoted&quot;");
        assert_eq!(escape("中文"), "中文");
    }
}
