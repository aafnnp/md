//! Laying a document out for somewhere other than a browser.
//!
//! The page [`crate::export`] writes styles itself from a `<style>` block in its
//! head. That is the right shape for a file you open yourself, and the wrong
//! shape for the places this module is for. A platform's editor reads what you
//! paste and keeps only what it recognises — the ones here are one-way
//! scrubbers. They drop `<style>`, drop `class` and `id`, may rewrite `<div>`
//! as `<p>`, downgrade headings, filter links that point off their own site,
//! and take an image only if it is already hosted by them. What survives is an
//! inline `style` attribute, in px and hexadecimal.
//!
//! So a layout is the same Markdown rendered again, with the styling written
//! onto each element instead of into a stylesheet, in the tags and the
//! measurements that platform still has when the paste is over.
//!
//! The preview pane is deliberately not part of this. It is a native renderer,
//! not a web view — `gpui-base`'s HTML formatter understands three style
//! properties and would read none of this — so a preview of a platform layout
//! could only ever be a second renderer, drifting away from the one that
//! decides what actually gets pasted.
//!
//! # Why this walks the compiler's output instead of parsing it
//!
//! [`lay_out`] reads tags by scanning for `<` and the `>` that closes it. That
//! is sound because of a property of the parser, not because the scan is
//! careful: `markdown`'s compiler escapes `&`, `<`, `>` and `"` in text and in
//! attribute values, and never stops — `allow_dangerous_html` is not among the
//! things [`markdown::Options::gfm`] turns on. No `<` or `>` can reach the
//! output anywhere but inside a tag, so a `<` always begins one and there is no
//! false boundary for a hostile document to plant. Raw HTML in the source
//! arrives escaped, which is also why nothing here has to strip anything:
//! there is no script element to strip.

use std::borrow::Cow;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{export, image};

/// Where a document is going, and what that place will do to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    /// No layout at all: the page the export has always written.
    #[default]
    Plain,
    /// 微信公众号. The strictest of them — see [`WECHAT`].
    ///
    /// Spelled out rather than left to `rename_all`, which would read this
    /// variant as two words and write `we_chat` into the settings file.
    #[serde(rename = "wechat")]
    WeChat,
    /// 今日头条.
    Toutiao,
    /// 小红书.
    Xiaohongshu,
    /// 知乎.
    Zhihu,
}

impl Platform {
    /// Every layout, in the order the settings panel offers them.
    pub const ALL: [Self; 5] = [
        Self::Plain,
        Self::WeChat,
        Self::Toutiao,
        Self::Xiaohongshu,
        Self::Zhihu,
    ];

    /// What the status bar and the settings panel call this one.
    pub fn label(self) -> &'static str {
        match self {
            Self::Plain => "No layout",
            Self::WeChat => "公众号",
            Self::Toutiao => "今日头条",
            Self::Xiaohongshu => "小红书",
            Self::Zhihu => "知乎",
        }
    }

    fn recipe(self) -> &'static Recipe {
        match self {
            Self::Plain => &NO_LAYOUT,
            Self::WeChat => &WECHAT,
            Self::Toutiao => &TOUTIAO,
            Self::Xiaohongshu => &XIAOHONGSHU,
            Self::Zhihu => &ZHIHU,
        }
    }
}

/// How the body of a laid-out document is dressed.
///
/// A second axis, independent of [`Platform`]. The platform decides what the
/// target editor will *keep* — which tags survive, which attributes are worth
/// writing at all — and that is a question about the target, not about taste.
/// The style decides how the surviving document *looks*: its sizes, its
/// leading, its colours and whether a heading wears a rule. The two do not
/// interact: no style changes a tag name or an attribute, and no platform
/// change alters what a style does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Style {
    /// The platform's own dress, which is this module's output before styles
    /// existed. The only variant that is not a [`Look`], and the reason its
    /// output is unchanged: there is nothing here to apply.
    #[default]
    Default,
    /// 简约: a size down, the lines closer together, the colours lighter, and
    /// no decoration on any heading.
    Minimal,
    /// 杂志: a size up, the lines further apart, the colours heavier, and a
    /// rule under the top three heading levels.
    Magazine,
}

impl Style {
    /// Every style, in the order the settings panel offers them.
    pub const ALL: [Self; 3] = [Self::Default, Self::Minimal, Self::Magazine];

    /// What the status bar and the settings panel call this one.
    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "默认",
            Self::Minimal => "简约",
            Self::Magazine => "杂志",
        }
    }

    /// The rules this style rewrites with. `None` for [`Style::Default`].
    fn look(self) -> Option<&'static Look> {
        match self {
            Self::Default => None,
            Self::Minimal => Some(&MINIMAL),
            Self::Magazine => Some(&MAGAZINE),
        }
    }

    /// `style`, as this style asks for it, on an element called `source_tag`.
    ///
    /// Borrowed rather than rebuilt for [`Style::Default`], so that path does
    /// no work and allocates nothing — it is the layout the app already had.
    fn dress<'a>(self, source_tag: &str, style: &'a str) -> Cow<'a, str> {
        match self.look() {
            Some(look) => Cow::Owned(look.dress(source_tag, style)),
            None => Cow::Borrowed(style),
        }
    }
}

/// One style: a set of edits to whatever dress the platform already applies.
///
/// Expressed as edits rather than as a second set of complete styles, because
/// each platform's dress is tuned to that platform — 公众号 reads at 16px,
/// 今日头条 at 17px — and those choices are not this module's to throw away.
/// Scaling them keeps the relationship between the two.
struct Look {
    /// What every `font-size` is multiplied by, rounded to whole pixels.
    scale: f32,
    /// The `line-height` for everything that is not a heading. A heading's
    /// leading is tuned to its own size and is left alone.
    leading: &'static str,
    /// `(what a declaration says, what it says instead)`, compared whole. A
    /// whole-value comparison is what keeps `4px solid #d9d9d9` and
    /// `1px solid #d9d9d9` apart: they are a quote's bar and a table's edge,
    /// and a style has reason to move one without the other.
    palette: &'static [(&'static str, &'static str)],
    /// The same, for headings only, and consulted first.
    ///
    /// Separate because the platforms' colours are not in one-to-one
    /// correspondence: `#1a1a1a` is 公众号's *heading* colour and 知乎's *body*
    /// colour. A single table would take one for the other. Resolving a
    /// heading here first means the value that comes out — `#2b2b2b`, say — is
    /// in neither table, so it cannot then be rewritten as body text.
    heading_palette: &'static [(&'static str, &'static str)],
    /// What a heading of each level wears, `h1` first. Empty for a level that
    /// wears nothing beyond what the platform already gave it — minus the
    /// decoration, which every style strips before this goes on.
    heading_rules: [&'static str; 6],
}

/// What a style takes off a heading before putting its own on.
///
/// Removed rather than overridden: `padding-left: 10px` followed by a rule that
/// says nothing about `padding-left` would leave the 10px there, and the two
/// rules would be read as one declaration set by whoever opens the file.
const DECORATION: [&str; 4] = [
    "border-left",
    "border-bottom",
    "padding-left",
    "padding-bottom",
];

impl Look {
    /// The platform's `style`, rewritten for an element that the Markdown
    /// source called `source_tag`.
    ///
    /// By source tag rather than by written tag: 小红书 writes `<h1>` as `<h2>`
    /// and `<h4>`–`<h6>` as `<h3>`, but a first-level heading is still the
    /// first-level heading to the reader, and its dress should follow that.
    fn dress(&self, source_tag: &str, style: &str) -> String {
        let level = heading_level(source_tag);
        let heading = level.is_some();
        let mut out = String::with_capacity(style.len() + 64);

        for declaration in style.split(';') {
            let declaration = declaration.trim();
            let Some((name, value)) = declaration.split_once(':') else {
                continue;
            };
            let name = name.trim();
            let value = value.trim();

            // Whatever the platform hung on a heading is the style's to
            // replace, so it comes off before anything is added.
            if heading && DECORATION.contains(&name) {
                continue;
            }

            let written = match name {
                "font-size" => Cow::Owned(whole_pixels(value, self.scale)),
                "line-height" if !heading => Cow::Borrowed(self.leading),
                _ => Cow::Borrowed(self.recolour(heading, value)),
            };

            out.push_str(name);
            out.push_str(": ");
            out.push_str(&written);
            out.push_str("; ");
        }

        if let Some(level) = level {
            out.push_str(self.heading_rules[level]);
        }
        // The loop leaves a trailing space behind; the `;` stays, so a dressed
        // style ends the way the recipe it was read from ends.
        let end = out.trim_end().len();
        out.truncate(end);
        out
    }

    /// What `value` becomes under this style's colours.
    ///
    /// A value in neither table is handed back as it was: a style is a set of
    /// choices about the colours it names, not a claim about the rest.
    fn recolour<'a>(&self, heading: bool, value: &'a str) -> &'a str {
        if heading
            && let Some((_, to)) = self.heading_palette.iter().find(|(from, _)| *from == value)
        {
            return to;
        }
        match self.palette.iter().find(|(from, _)| *from == value) {
            Some((_, to)) => to,
            None => value,
        }
    }
}

/// Which heading `tag` is, counting from zero, or `None` for anything else.
fn heading_level(tag: &str) -> Option<usize> {
    let level = tag.strip_prefix('h')?.parse::<usize>().ok()?;
    (1..=6).contains(&level).then(|| level - 1)
}

/// A pixel size multiplied by `scale` and rounded to a whole one.
///
/// `16px` at 0.94 is 15.04px, and a size with a fraction in it is a size the
/// author did not choose, printed to a place no editor will show it. Anything
/// that is not a pixel length — a percentage, a keyword — is left as written
/// rather than guessed at.
fn whole_pixels(value: &str, scale: f32) -> String {
    match value.strip_suffix("px").map(str::parse::<f32>) {
        Some(Ok(size)) => format!("{}px", (size * scale).round()),
        _ => value.to_string(),
    }
}

/// 简约: quieter than any platform's own dress, and undecorated.
static MINIMAL: Look = Look {
    scale: 0.94,
    leading: "1.65",
    heading_palette: &[
        ("#1a1a1a", "#2b2b2b"),
        ("#222222", "#2b2b2b"),
        ("#121212", "#2b2b2b"),
    ],
    palette: &[
        // Body text, once per platform's own choice of it.
        ("#3f3f3f", "#555555"),
        ("#333333", "#555555"),
        ("#1a1a1a", "#555555"),
        // Quoted and secondary text, which goes a shade lighter again.
        ("#555555", "#6e6e6e"),
        ("#666666", "#6e6e6e"),
        ("#646464", "#6e6e6e"),
        ("#888888", "#9a9a9a"),
        // Links, all three platforms' colours, to one that sits back.
        ("#576b95", "#4a6fa5"),
        ("#1e6fd9", "#4a6fa5"),
        ("#175199", "#4a6fa5"),
        // The bar down the side of a quote, as each platform writes it.
        ("4px solid #d9d9d9", "2px solid #d0d0d0"),
        ("4px solid #f04142", "2px solid #d0d0d0"),
        ("3px solid #dddddd", "2px solid #d0d0d0"),
        ("3px solid #d3d3d3", "2px solid #d0d0d0"),
    ],
    heading_rules: ["", "", "", "", "", ""],
};

/// 杂志: larger, more open, and ruled under the top three levels.
static MAGAZINE: Look = Look {
    scale: 1.10,
    leading: "1.95",
    heading_palette: &[
        ("#1a1a1a", "#000000"),
        ("#222222", "#000000"),
        ("#121212", "#000000"),
    ],
    palette: &[
        // Body text. 知乎's is already `#1a1a1a` and so needs no entry.
        ("#3f3f3f", "#1a1a1a"),
        ("#333333", "#1a1a1a"),
        // Secondary text darkens rather than fades: this is the loud style.
        ("#555555", "#444444"),
        ("#666666", "#444444"),
        ("#646464", "#444444"),
        ("#888888", "#6a6a6a"),
        ("#576b95", "#9a3324"),
        ("#1e6fd9", "#9a3324"),
        ("#175199", "#9a3324"),
        ("4px solid #d9d9d9", "4px solid #111111"),
        ("4px solid #f04142", "4px solid #111111"),
        ("3px solid #dddddd", "4px solid #111111"),
        ("3px solid #d3d3d3", "4px solid #111111"),
    ],
    heading_rules: [
        "padding-bottom: 10px; border-bottom: 3px solid #111111;",
        "padding-bottom: 8px; border-bottom: 2px solid #111111;",
        "padding-bottom: 4px; border-bottom: 1px solid #d8d8d8;",
        "",
        "",
        "",
    ],
};

/// A document after it has been laid out for a platform.
pub struct Layout {
    /// The HTML the platform's editor is meant to be handed.
    pub html: String,
    /// The same document as plain text, for a paste target that cannot take
    /// HTML and for the clipboard's fallback flavour.
    pub plain: String,
    /// Images the document referred to that could not be read off the disk.
    /// Still their original `src` in [`Layout::html`], and worth saying out
    /// loud: the reader will see them missing.
    pub unresolved_images: Vec<String>,
}

/// The body of the document, without the page around it.
///
/// This is what goes on the clipboard: a paste target wants the fragment, not
/// a document it would have to find the body inside.
pub fn fragment(
    platform: Platform,
    style: Style,
    markdown: &str,
    base_dir: Option<&Path>,
) -> Layout {
    let compiled = export::to_html_fragment(markdown);
    if platform == Platform::Plain {
        return Layout {
            plain: lay_out(&compiled, &NO_LAYOUT, Style::Default, None).plain,
            html: compiled,
            unresolved_images: Vec::new(),
        };
    }
    lay_out(&compiled, platform.recipe(), style, base_dir)
}

/// A complete page, for saving to a file and opening in a browser.
///
/// A style on [`Platform::Plain`] does nothing: that page is styled by the
/// stylesheet in its own head, and a style here would have to become a second
/// one. No layout means nothing to dress.
pub fn page(
    platform: Platform,
    style: Style,
    title: &str,
    markdown: &str,
    base_dir: Option<&Path>,
) -> String {
    if platform == Platform::Plain {
        return export::to_html_page(title, markdown);
    }
    let body = lay_out(
        &export::to_html_fragment(markdown),
        platform.recipe(),
        style,
        base_dir,
    );
    standalone_page(title, &body.html)
}

/// Render `markdown` for `platform` and write the page to `path`.
pub fn export(
    path: &Path,
    platform: Platform,
    style: Style,
    title: &str,
    markdown: &str,
    base_dir: Option<&Path>,
) -> io::Result<()> {
    std::fs::write(path, page(platform, style, title, markdown, base_dir))
}

/// A laid-out fragment wrapped in the smallest page that stands on its own.
///
/// Not [`export::to_html_page`]'s page: that one styles its elements from a
/// stylesheet, and these elements carry their styling already. All this page
/// adds is a readable measure and a background to read them against.
fn standalone_page(title: &str, body: &str) -> String {
    format!(
        "<!DOCTYPE html>\n\
         <html lang=\"en\">\n\
         <head>\n\
         <meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <meta name=\"generator\" content=\"md\">\n\
         <title>{}</title>\n\
         <style>\n{}\n</style>\n\
         </head>\n\
         <body>\n\
         <main>\n{}\n</main>\n\
         </body>\n\
         </html>\n",
        export::escape(title),
        PAGE_STYLE,
        body
    )
}

/// What a saved layout page sets, and no more than it needs to.
///
/// The colours here are the light ones because the layout is written for a
/// light editor; a dark page under light inline styles would be worse than
/// either.
const PAGE_STYLE: &str = "\
  :root { color-scheme: light; }
  body {
    margin: 0;
    background: #ffffff;
    font-family: -apple-system, BlinkMacSystemFont, \"Segoe UI\", Helvetica, Arial, sans-serif;
  }
  main { max-width: 720px; margin: 0 auto; padding: 2.5rem 1.25rem 5rem; }
";

// ---------------------------------------------------------------------------
// Recipes
// ---------------------------------------------------------------------------

/// What one platform will take, element by element.
struct Recipe {
    /// The element the whole document is wrapped in. Empty for no layout.
    wrapper: &'static str,
    /// What goes on that wrapper.
    wrapper_style: &'static str,
    /// `(the tag the compiler writes, the tag to write instead, the inline
    /// style to put on it)`. A name repeated in the first two columns means
    /// the tag is kept as it is.
    elements: &'static [(&'static str, &'static str, &'static str)],
    /// Attributes the platform drops anyway, left off rather than carried
    /// across to be discarded.
    strip: &'static [&'static str],
    /// What `<code>` gets inside a `<pre>`, where the block's own background
    /// is already doing the work.
    code_in_pre: &'static str,
    /// Whether `<input type=checkbox>` becomes a character. Every platform
    /// here strips the input and keeps nothing in its place.
    checkboxes_as_text: bool,
    /// What a task item's `<ul>` gets, so the character does not sit behind a
    /// bullet.
    task_list_style: &'static str,
}

impl Recipe {
    /// The tag to write and the style to write on it, for a tag the compiler
    /// wrote. `None` leaves the tag alone.
    fn element(&self, name: &str) -> Option<(&'static str, &'static str)> {
        self.elements
            .iter()
            .find(|entry| entry.0 == name)
            .map(|entry| (entry.1, entry.2))
    }
}

/// Used to read a document back as text with nothing in the way — [`fragment`]
/// takes the plain text of a `Plain` document through this.
static NO_LAYOUT: Recipe = Recipe {
    wrapper: "",
    wrapper_style: "",
    elements: &[],
    strip: &[],
    code_in_pre: "",
    checkboxes_as_text: false,
    task_list_style: "",
};

/// 微信公众号.
///
/// The list this is written against is short on purpose. The editor keeps
/// `<section>`, `<p>`, `<span>` and little else — `<div>` may come back as
/// `<p>`, `<h1>` and `<h2>` are downgraded, `<input>` disappears — so headings
/// are written as `<section>` with the size in the style, where nothing can
/// take the size away. `class` and `id` go for the same reason: they do not
/// survive, and a document shipped with them claims a styling it does not have.
static WECHAT: Recipe = Recipe {
    wrapper: "section",
    wrapper_style: "font-size: 16px; line-height: 1.75; color: #3f3f3f;",
    elements: &[
        (
            "p",
            "p",
            "margin: 0 0 16px; font-size: 16px; line-height: 1.75; color: #3f3f3f;",
        ),
        (
            "h1",
            "section",
            "margin: 32px 0 16px; padding-left: 10px; border-left: 4px solid #07c160; font-size: 22px; line-height: 1.4; font-weight: bold; color: #1a1a1a;",
        ),
        (
            "h2",
            "section",
            "margin: 28px 0 14px; padding-left: 10px; border-left: 4px solid #07c160; font-size: 20px; line-height: 1.4; font-weight: bold; color: #1a1a1a;",
        ),
        (
            "h3",
            "section",
            "margin: 24px 0 12px; padding-left: 10px; border-left: 3px solid #07c160; font-size: 18px; line-height: 1.4; font-weight: bold; color: #1a1a1a;",
        ),
        (
            "h4",
            "section",
            "margin: 22px 0 12px; padding-left: 10px; border-left: 3px solid #07c160; font-size: 17px; line-height: 1.4; font-weight: bold; color: #1a1a1a;",
        ),
        (
            "h5",
            "section",
            "margin: 20px 0 10px; padding-left: 10px; border-left: 3px solid #07c160; font-size: 16px; line-height: 1.4; font-weight: bold; color: #1a1a1a;",
        ),
        (
            "h6",
            "section",
            "margin: 20px 0 10px; padding-left: 10px; border-left: 3px solid #07c160; font-size: 16px; line-height: 1.4; font-weight: bold; color: #1a1a1a;",
        ),
        ("ul", "ul", "margin: 0 0 16px; padding-left: 24px;"),
        ("ol", "ol", "margin: 0 0 16px; padding-left: 24px;"),
        (
            "li",
            "li",
            "margin: 0 0 8px; font-size: 16px; line-height: 1.75; color: #3f3f3f;",
        ),
        (
            "blockquote",
            "blockquote",
            "margin: 0 0 16px; padding: 12px 16px; background: #f7f7f7; border-left: 4px solid #d9d9d9; font-size: 15px; line-height: 1.75; color: #666666;",
        ),
        (
            "pre",
            "pre",
            "margin: 0 0 16px; padding: 12px 16px; background: #f6f8fa; border-radius: 4px; overflow-x: auto; font-size: 14px; line-height: 1.6;",
        ),
        (
            "code",
            "code",
            "font-family: Menlo, Consolas, monospace; font-size: 14px; background: #f0f1f3; padding: 1px 4px; border-radius: 3px;",
        ),
        ("em", "em", "font-style: italic;"),
        ("strong", "strong", "font-weight: bold;"),
        ("del", "del", "text-decoration: line-through;"),
        ("a", "a", "color: #576b95; text-decoration: none;"),
        (
            "img",
            "img",
            "display: block; max-width: 100%; height: auto; margin: 0 auto 16px; border-radius: 4px;",
        ),
        ("br", "br", ""),
        (
            "hr",
            "hr",
            "margin: 24px 0; border: 0; border-top: 1px solid #e5e5e5;",
        ),
        (
            "table",
            "table",
            "margin: 0 0 16px; border-collapse: collapse; width: 100%; font-size: 15px;",
        ),
        ("thead", "thead", ""),
        ("tbody", "tbody", ""),
        ("tr", "tr", ""),
        (
            "th",
            "th",
            "border: 1px solid #d9d9d9; padding: 8px 10px; background: #f7f7f7; font-weight: bold; text-align: left;",
        ),
        ("td", "td", "border: 1px solid #d9d9d9; padding: 8px 10px;"),
        (
            "section",
            "section",
            "margin: 28px 0 0; padding-top: 16px; border-top: 1px solid #e5e5e5; font-size: 14px; line-height: 1.7; color: #888888;",
        ),
        ("sup", "sup", "font-size: 12px;"),
    ],
    strip: &[
        "class",
        "id",
        "disabled",
        "data-footnotes",
        "data-footnote-ref",
        "data-footnote-backref",
        "aria-describedby",
        "aria-label",
    ],
    code_in_pre: "font-family: Menlo, Consolas, monospace; font-size: 14px; background: none; padding: 0;",
    checkboxes_as_text: true,
    task_list_style: "list-style: none; padding-left: 4px;",
};

/// 今日头条.
///
/// Takes real headings and real tables, so the structure is left as the author
/// wrote it and only the dress is added — a red rule under the two top levels,
/// which is the house look there.
static TOUTIAO: Recipe = Recipe {
    wrapper: "section",
    wrapper_style: "font-size: 17px; line-height: 1.8; color: #333333;",
    elements: &[
        (
            "p",
            "p",
            "margin: 0 0 18px; font-size: 17px; line-height: 1.8; color: #333333;",
        ),
        (
            "h1",
            "h1",
            "margin: 32px 0 18px; padding-bottom: 8px; border-bottom: 2px solid #f04142; font-size: 24px; line-height: 1.4; color: #222222;",
        ),
        (
            "h2",
            "h2",
            "margin: 28px 0 16px; padding-bottom: 8px; border-bottom: 2px solid #f04142; font-size: 21px; line-height: 1.4; color: #222222;",
        ),
        (
            "h3",
            "h3",
            "margin: 24px 0 14px; font-size: 19px; line-height: 1.4; color: #222222;",
        ),
        (
            "h4",
            "h4",
            "margin: 22px 0 12px; font-size: 17px; line-height: 1.4; color: #222222;",
        ),
        (
            "h5",
            "h5",
            "margin: 20px 0 12px; font-size: 17px; line-height: 1.4; color: #222222;",
        ),
        (
            "h6",
            "h6",
            "margin: 20px 0 12px; font-size: 17px; line-height: 1.4; color: #222222;",
        ),
        ("ul", "ul", "margin: 0 0 18px; padding-left: 24px;"),
        ("ol", "ol", "margin: 0 0 18px; padding-left: 24px;"),
        (
            "li",
            "li",
            "margin: 0 0 8px; font-size: 17px; line-height: 1.8; color: #333333;",
        ),
        (
            "blockquote",
            "blockquote",
            "margin: 0 0 18px; padding: 12px 16px; background: #f7f8fa; border-left: 4px solid #f04142; font-size: 16px; line-height: 1.8; color: #555555;",
        ),
        (
            "pre",
            "pre",
            "margin: 0 0 18px; padding: 14px 16px; background: #f5f6f7; border-radius: 4px; overflow-x: auto; font-size: 14px; line-height: 1.6;",
        ),
        (
            "code",
            "code",
            "font-family: Menlo, Consolas, monospace; font-size: 14px; background: #f5f6f7; padding: 1px 4px; border-radius: 3px;",
        ),
        ("em", "em", "font-style: italic;"),
        ("strong", "strong", "font-weight: bold;"),
        ("del", "del", "text-decoration: line-through;"),
        ("a", "a", "color: #1e6fd9; text-decoration: none;"),
        (
            "img",
            "img",
            "display: block; max-width: 100%; height: auto; margin: 0 auto 18px;",
        ),
        ("br", "br", ""),
        (
            "hr",
            "hr",
            "margin: 26px 0; border: 0; border-top: 1px solid #e5e5e5;",
        ),
        (
            "table",
            "table",
            "margin: 0 0 18px; border-collapse: collapse; width: 100%; font-size: 16px;",
        ),
        ("thead", "thead", ""),
        ("tbody", "tbody", ""),
        ("tr", "tr", ""),
        (
            "th",
            "th",
            "border: 1px solid #dcdcdc; padding: 8px 12px; background: #f5f6f7; font-weight: bold; text-align: left;",
        ),
        ("td", "td", "border: 1px solid #dcdcdc; padding: 8px 12px;"),
        (
            "section",
            "section",
            "margin: 28px 0 0; padding-top: 16px; border-top: 1px solid #e5e5e5; font-size: 15px; line-height: 1.7; color: #888888;",
        ),
        ("sup", "sup", "font-size: 12px;"),
    ],
    strip: &[
        "disabled",
        "data-footnotes",
        "data-footnote-ref",
        "data-footnote-backref",
        "aria-describedby",
        "aria-label",
    ],
    code_in_pre: "font-family: Menlo, Consolas, monospace; font-size: 14px; background: none; padding: 0;",
    checkboxes_as_text: true,
    task_list_style: "list-style: none; padding-left: 4px;",
};

/// 小红书.
///
/// The editor there takes a rich paste and then normalises it: it keeps the
/// shape of what it was given — the small headings, the indented quotes, the
/// lists — and rewrites the appearance into its own. Styling it hard would be
/// work thrown away, so this is the one recipe that mostly does not style at
/// all. The value of laying a document out for 小红书 is that the hierarchy
/// arrives intact, not that it arrives looking like anything in particular.
static XIAOHONGSHU: Recipe = Recipe {
    wrapper: "section",
    wrapper_style: "font-size: 16px; line-height: 1.8; color: #333333;",
    elements: &[
        (
            "p",
            "p",
            "margin: 0 0 16px; font-size: 16px; line-height: 1.8; color: #333333;",
        ),
        (
            "h1",
            "h2",
            "margin: 26px 0 14px; font-size: 20px; line-height: 1.4; font-weight: bold; color: #222222;",
        ),
        (
            "h2",
            "h2",
            "margin: 26px 0 14px; font-size: 20px; line-height: 1.4; font-weight: bold; color: #222222;",
        ),
        (
            "h3",
            "h3",
            "margin: 22px 0 12px; font-size: 18px; line-height: 1.4; font-weight: bold; color: #222222;",
        ),
        (
            "h4",
            "h3",
            "margin: 22px 0 12px; font-size: 18px; line-height: 1.4; font-weight: bold; color: #222222;",
        ),
        (
            "h5",
            "h3",
            "margin: 22px 0 12px; font-size: 18px; line-height: 1.4; font-weight: bold; color: #222222;",
        ),
        (
            "h6",
            "h3",
            "margin: 22px 0 12px; font-size: 18px; line-height: 1.4; font-weight: bold; color: #222222;",
        ),
        (
            "blockquote",
            "blockquote",
            "margin: 0 0 16px; padding: 10px 14px; background: #f6f6f6; border-left: 3px solid #dddddd; color: #666666;",
        ),
        (
            "img",
            "img",
            "display: block; max-width: 100%; height: auto; margin: 0 auto 16px;",
        ),
    ],
    strip: &[
        "class",
        "id",
        "disabled",
        "data-footnotes",
        "data-footnote-ref",
        "data-footnote-backref",
        "aria-describedby",
        "aria-label",
    ],
    // No inline code style to speak of: this recipe leaves `<pre>` and `<code>`
    // exactly as the compiler wrote them.
    code_in_pre: "",
    checkboxes_as_text: true,
    task_list_style: "list-style: none;",
};

/// 知乎.
///
/// The column editor takes a rich paste and leaves most of it alone — headings,
/// quotes, code, tables all arrive as themselves — so this is the closest of
/// the four to what the author wrote, with a thin rule under the top two
/// heading levels and not much else.
static ZHIHU: Recipe = Recipe {
    wrapper: "div",
    wrapper_style: "font-size: 16px; line-height: 1.7; color: #1a1a1a;",
    elements: &[
        (
            "p",
            "p",
            "margin: 0 0 16px; font-size: 16px; line-height: 1.7; color: #1a1a1a;",
        ),
        (
            "h1",
            "h1",
            "margin: 30px 0 16px; padding-bottom: 6px; border-bottom: 1px solid #e5e5e5; font-size: 24px; line-height: 1.4; color: #121212;",
        ),
        (
            "h2",
            "h2",
            "margin: 26px 0 14px; padding-bottom: 6px; border-bottom: 1px solid #e5e5e5; font-size: 21px; line-height: 1.4; color: #121212;",
        ),
        (
            "h3",
            "h3",
            "margin: 22px 0 12px; font-size: 18px; line-height: 1.4; color: #121212;",
        ),
        (
            "h4",
            "h4",
            "margin: 20px 0 12px; font-size: 16px; line-height: 1.4; color: #121212;",
        ),
        (
            "h5",
            "h5",
            "margin: 18px 0 10px; font-size: 16px; line-height: 1.4; color: #121212;",
        ),
        (
            "h6",
            "h6",
            "margin: 18px 0 10px; font-size: 16px; line-height: 1.4; color: #121212;",
        ),
        ("ul", "ul", "margin: 0 0 16px; padding-left: 24px;"),
        ("ol", "ol", "margin: 0 0 16px; padding-left: 24px;"),
        (
            "li",
            "li",
            "margin: 0 0 8px; font-size: 16px; line-height: 1.7; color: #1a1a1a;",
        ),
        (
            "blockquote",
            "blockquote",
            "margin: 0 0 16px; padding: 2px 0 2px 14px; border-left: 3px solid #d3d3d3; color: #646464;",
        ),
        (
            "pre",
            "pre",
            "margin: 0 0 16px; padding: 12px 16px; background: #f6f8fa; border-radius: 4px; overflow-x: auto; font-size: 14px; line-height: 1.6;",
        ),
        (
            "code",
            "code",
            "font-family: Menlo, Consolas, monospace; font-size: 14px; background: #f0f1f3; padding: 1px 4px; border-radius: 3px;",
        ),
        ("em", "em", "font-style: italic;"),
        ("strong", "strong", "font-weight: bold;"),
        ("del", "del", "text-decoration: line-through;"),
        ("a", "a", "color: #175199; text-decoration: none;"),
        (
            "img",
            "img",
            "display: block; max-width: 100%; height: auto; margin: 0 auto 16px;",
        ),
        ("br", "br", ""),
        (
            "hr",
            "hr",
            "margin: 24px 0; border: 0; border-top: 1px solid #e5e5e5;",
        ),
        (
            "table",
            "table",
            "margin: 0 0 16px; border-collapse: collapse; width: 100%; font-size: 15px;",
        ),
        ("thead", "thead", ""),
        ("tbody", "tbody", ""),
        ("tr", "tr", ""),
        (
            "th",
            "th",
            "border: 1px solid #dcdcdc; padding: 8px 12px; background: #f6f8fa; font-weight: bold; text-align: left;",
        ),
        ("td", "td", "border: 1px solid #dcdcdc; padding: 8px 12px;"),
        (
            "section",
            "section",
            "margin: 28px 0 0; padding-top: 16px; border-top: 1px solid #e5e5e5; font-size: 14px; line-height: 1.7; color: #888888;",
        ),
        ("sup", "sup", "font-size: 12px;"),
    ],
    strip: &[
        "disabled",
        "data-footnotes",
        "data-footnote-ref",
        "data-footnote-backref",
        "aria-describedby",
        "aria-label",
    ],
    code_in_pre: "font-family: Menlo, Consolas, monospace; font-size: 14px; background: none; padding: 0;",
    checkboxes_as_text: true,
    task_list_style: "list-style: none; padding-left: 4px;",
};

// ---------------------------------------------------------------------------
// The walk
// ---------------------------------------------------------------------------

/// Tags whose content, once it is text, has to end on a line of its own —
/// without these the plain-text fallback is one long paragraph.
const BLOCK: [&str; 15] = [
    "p",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "li",
    "blockquote",
    "pre",
    "tr",
    "ul",
    "ol",
    "section",
    "div",
];

/// Rewrite `fragment` as `recipe` asks, and read it back as text on the way.
fn lay_out(fragment: &str, recipe: &Recipe, style: Style, base_dir: Option<&Path>) -> Layout {
    let mut html = String::with_capacity(fragment.len() + fragment.len() / 2 + 64);
    let mut plain = String::with_capacity(fragment.len() / 2);
    let mut unresolved_images = Vec::new();
    // Reused per element: cleared, filled with whatever this element wears, and
    // written out. Named apart from the `style` argument, which is the dress
    // being applied to it.
    let mut rules = String::new();
    let mut pre_depth = 0usize;
    // The name of an element whose tags are being left out, if any. Only ever
    // one deep: the parser writes `sr-only` on a heading that holds nothing.
    let mut skipping: Option<&str> = None;

    if !recipe.wrapper.is_empty() {
        html.push('<');
        html.push_str(recipe.wrapper);
        push_style(
            &mut html,
            &style.dress(recipe.wrapper, recipe.wrapper_style),
        );
        html.push('>');
    }

    let mut at = 0;
    while at < fragment.len() {
        if fragment.as_bytes()[at] != b'<' {
            let next = fragment[at..]
                .find('<')
                .map_or(fragment.len(), |offset| at + offset);
            if skipping.is_none() {
                html.push_str(&fragment[at..next]);
                append_unescaped(&fragment[at..next], &mut plain);
            }
            at = next;
            continue;
        }

        let Some(close) = tag_end(fragment, at) else {
            // A `<` with no `>` after it, which the compiler never writes.
            // Copying the remainder as text is the reading that loses least.
            if skipping.is_none() {
                html.push_str(&fragment[at..]);
                append_unescaped(&fragment[at..], &mut plain);
            }
            break;
        };

        let raw = &fragment[at..=close];
        let tag = Tag::parse(raw);
        at = close + 1;

        // Leaving out an element and its matching close, together.
        if let Some(name) = skipping {
            if !tag.opening && tag.name == name {
                skipping = None;
            }
            continue;
        }
        // `class="sr-only"` is the parser's way of saying "for screen readers
        // only". It writes it on the label it puts on a footnote section,
        // which a laid-out document would then print as a heading — and under
        // a recipe that turns headings into banners, as a banner.
        if tag.opening && carries_class(&tag, "sr-only") && !tag.self_closing {
            skipping = Some(tag.name);
            continue;
        }

        // `<input>` has no identity worth keeping: it is the parser's task-list
        // checkbox, and `☑` says the same thing in a character that every
        // platform here will take.
        if tag.name == "input" {
            if recipe.checkboxes_as_text && is_checkbox(&tag) {
                let mark = if has_attribute(&tag, "checked") {
                    "☑"
                } else {
                    "☐"
                };
                html.push_str(mark);
                plain.push_str(mark);
            }
            continue;
        }

        rules.clear();
        let mapped = recipe.element(tag.name);

        if !tag.opening {
            let name = match mapped {
                Some((to, _)) => to,
                None => {
                    html.push_str(raw);
                    append_block_break(tag.name, &mut plain);
                    continue;
                }
            };
            if name == "pre" {
                pre_depth = pre_depth.saturating_sub(1);
            }
            html.push_str("</");
            html.push_str(name);
            html.push('>');
            append_block_break(tag.name, &mut plain);
            continue;
        }

        let name = match mapped {
            Some((to, element_style)) => {
                // `<code>` inside a `<pre>` is the block's own body, not a
                // phrase in a sentence: the inline pill's background and
                // padding would draw a second box inside the first.
                let element_style = if to == "code" && pre_depth > 0 {
                    recipe.code_in_pre
                } else {
                    element_style
                };
                rules.push_str(&style.dress(tag.name, element_style));
                to
            }
            None => tag.name,
        };

        // A task item's `<ul>`: the `☑` replaces the bullet, so the bullet has
        // to go. The parser marks nothing on the list itself — the checkbox is
        // the only sign — so the item is recognised by looking one tag ahead.
        //
        // Not dressed: how a list aligns is structure, not dress, and a style
        // that changed it would move the tick out from under the text.
        if name == "ul" && recipe.checkboxes_as_text && starts_a_task_item(fragment, at) {
            rules.push_str(recipe.task_list_style);
        }

        html.push('<');
        html.push_str(name);
        for (attribute, value) in &tag.attributes {
            if recipe.strip.contains(attribute) {
                continue;
            }
            html.push(' ');
            html.push_str(attribute);
            if let Some(value) = value {
                html.push_str("=\"");
                if tag.name == "img" && *attribute == "src" {
                    html.push_str(&embed_image(value, base_dir, &mut unresolved_images));
                } else {
                    html.push_str(value);
                }
                html.push('"');
            }
        }
        push_style(&mut html, &rules);
        if tag.self_closing {
            html.push_str(" /");
        }
        html.push('>');

        if name == "pre" {
            pre_depth += 1;
        }
        if tag.name == "img" {
            append_unescaped(tag.attribute("alt").unwrap_or_default(), &mut plain);
        }
        append_block_break(tag.name, &mut plain);
    }

    if !recipe.wrapper.is_empty() {
        html.push_str("</");
        html.push_str(recipe.wrapper);
        html.push('>');
        html.push('\n');
    }

    Layout {
        html,
        plain: tidy(&plain),
        unresolved_images,
    }
}

/// One tag as the compiler wrote it.
struct Tag<'a> {
    /// `<h2 …>` rather than `</h2>`.
    opening: bool,
    /// The trailing `/` of a void element, as in `<hr />`.
    self_closing: bool,
    name: &'a str,
    /// Attribute names and values, in the order written. A value is `None`
    /// only for an attribute written bare — which the compiler does not do,
    /// it writes `disabled=""` — and `Some("")` for one with nothing in it.
    attributes: Vec<(&'a str, Option<&'a str>)>,
}

impl<'a> Tag<'a> {
    /// Read the tag that `raw` holds whole, `<` to `>`.
    fn parse(raw: &'a str) -> Self {
        let mut inner = &raw[1..raw.len() - 1];
        let opening = !inner.starts_with('/');
        if !opening {
            inner = &inner[1..];
        }
        let self_closing = inner.ends_with('/');
        if self_closing {
            inner = &inner[..inner.len() - 1];
        }

        let mut rest = inner;
        let name_end = rest
            .find(|one: char| one.is_whitespace() || one == '/')
            .unwrap_or(rest.len());
        let name = &rest[..name_end];
        rest = &rest[name_end..];

        let mut attributes = Vec::new();
        loop {
            rest = rest.trim_start();
            if rest.is_empty() {
                break;
            }
            let name_end = rest
                .find(|one: char| one.is_whitespace() || one == '=')
                .unwrap_or(rest.len());
            let attribute = &rest[..name_end];
            rest = &rest[name_end..];

            let value = match rest.strip_prefix('=') {
                Some(after) => {
                    let after = after.trim_start();
                    match after.chars().next() {
                        Some(quote @ ('"' | '\'')) => {
                            let tail = &after[1..];
                            match tail.find(quote) {
                                Some(end) => {
                                    rest = &tail[end + 1..];
                                    Some(&tail[..end])
                                }
                                // Unterminated: the compiler writes both
                                // quotes or neither, so this is something
                                // else's HTML. Taking the rest of the value
                                // keeps the tag readable.
                                None => {
                                    rest = "";
                                    Some(tail)
                                }
                            }
                        }
                        _ => {
                            let end = after.find(char::is_whitespace).unwrap_or(after.len());
                            rest = &after[end..];
                            Some(&after[..end])
                        }
                    }
                }
                None => None,
            };
            attributes.push((attribute, value));
        }

        Self {
            opening,
            self_closing,
            name,
            attributes,
        }
    }

    fn attribute(&self, name: &str) -> Option<&'a str> {
        self.attributes
            .iter()
            .find(|attribute| attribute.0 == name)
            .and_then(|attribute| attribute.1)
    }
}

/// Where the tag starting at `start` ends, or `None` if it never does.
///
/// A quote is taken for the opening of a value only where one can actually
/// open — straight after an `=`. Counting quotes instead is the obvious way
/// and the wrong one here, because the compiler writes a link's `title`
/// through unescaped (`to_html.rs`: `context.push(&title)`, where every other
/// attribute value goes through `encode`). A title given in single quotes and
/// holding a double quote is valid CommonMark — `[x](u 'say "hi"')` — and comes
/// out as `title="say "hi""`, whose quotes a toggle pairs off into runs that do
/// not line up with the value, and whose odd-quote variant would run the scan
/// past this tag into the next. Anchoring to the `=` costs a comparison and
/// keeps the boundaries where they belong.
fn tag_end(fragment: &str, start: usize) -> Option<usize> {
    let bytes = fragment.as_bytes();
    let mut quote: Option<u8> = None;
    let mut at = start + 1;
    while at < bytes.len() {
        let byte = bytes[at];
        match quote {
            Some(open) if byte == open => quote = None,
            Some(_) => {}
            None => match byte {
                b'"' | b'\'' if bytes[at - 1] == b'=' => quote = Some(byte),
                b'>' => return Some(at),
                _ => {}
            },
        }
        at += 1;
    }
    None
}

fn has_attribute(tag: &Tag, name: &str) -> bool {
    tag.attributes.iter().any(|attribute| attribute.0 == name)
}

fn is_checkbox(tag: &Tag) -> bool {
    tag.attribute("type") == Some("checkbox")
}

/// Whether `tag` is one of the elements the parser marks with `class`.
fn carries_class(tag: &Tag, class: &str) -> bool {
    tag.attribute("class")
        .is_some_and(|value| value.split_whitespace().any(|written| written == class))
}

/// Whether the tag at `from` opens a task item's checkbox.
///
/// The parser marks nothing on the list to recognise a task list by: the
/// `<input>` is the only sign, and it is the first thing inside the item, not
/// inside the list. So the item's own `<li>` is stepped over and the tag after
/// it is the one asked about.
fn starts_a_task_item(fragment: &str, from: usize) -> bool {
    let mut rest = fragment[from..].trim_start();

    let Some(close) = rest.strip_prefix('<').and_then(|_| tag_end(rest, 0)) else {
        return false;
    };
    let tag = Tag::parse(&rest[..=close]);
    if tag.opening && tag.name == "li" {
        rest = rest[close + 1..].trim_start();
    }

    let Some(close) = rest.strip_prefix('<').and_then(|_| tag_end(rest, 0)) else {
        return false;
    };
    let tag = Tag::parse(&rest[..=close]);
    tag.opening && tag.name == "input" && is_checkbox(&tag)
}

/// Write ` style="…"`, or nothing at all when there is no style to write.
///
/// An empty style is not the same as an absent one: `<code>` under the 小红书
/// recipe has no style on purpose, and an empty `style=""` would be a claim
/// about it rather than a silence.
fn push_style(html: &mut String, style: &str) {
    if style.is_empty() {
        return;
    }
    html.push_str(" style=\"");
    html.push_str(style);
    html.push('"');
}

fn append_block_break(name: &str, plain: &mut String) {
    if BLOCK.contains(&name) {
        plain.push('\n');
    }
}

/// Collapse the text to one blank line between blocks and no trailing space.
fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blanks = 0;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            blanks += 1;
            if blanks > 1 {
                continue;
            }
        } else {
            blanks = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim().to_string()
}

// ---------------------------------------------------------------------------
// Escapes and images
// ---------------------------------------------------------------------------

/// The four entities the compiler's escaping produces, read back.
///
/// `markdown` escapes `&`, `<`, `>` and `"` and nothing else, so this is a
/// complete inverse for anything it wrote. An `&` that begins none of the four
/// is left where it is, which is what keeps this from mangling a document that
/// did not come from the compiler.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    append_unescaped(text, &mut out);
    out
}

fn append_unescaped(text: &str, out: &mut String) {
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let (decoded, width) = if tail.starts_with("&amp;") {
            ('&', 5)
        } else if tail.starts_with("&lt;") {
            ('<', 4)
        } else if tail.starts_with("&gt;") {
            ('>', 4)
        } else if tail.starts_with("&quot;") {
            ('"', 6)
        } else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        out.push(decoded);
        rest = &tail[width..];
    }
    out.push_str(rest);
}

/// The largest image that will be read and encoded. A photograph off a phone is
/// a few megabytes; past this something has gone wrong, and base64 makes the
/// result a third larger again.
const MAX_EMBEDDED_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

/// What a laid-out `<img>` should carry as its `src`.
///
/// The value arrives as the compiler escaped it, so an author's `a&b.png` is
/// written `a&amp;b.png`; [`image::resolve`] wants the URL that was typed, and
/// looks on disk for the file that name points at. Unescaping first is what
/// makes the two agree.
///
/// A URL that is not ours to resolve — one with a scheme, an absolute path, or
/// a document with no folder behind it — is left exactly as written. So is one
/// whose file cannot be read, which is also recorded, because a picture the
/// reader will not see is worth a word.
fn embed_image(value: &str, base_dir: Option<&Path>, unresolved: &mut Vec<String>) -> String {
    let Some(base_dir) = base_dir else {
        return value.to_string();
    };
    let url = unescape(value);
    let Some(path) = image::resolve(base_dir, &url) else {
        return value.to_string();
    };
    match read_image(&path) {
        Some(embedded) => embedded,
        None => {
            unresolved.push(url);
            value.to_string()
        }
    }
}

/// `data:image/png;base64,…`, or `None` when the file is not there to read.
fn read_image(path: &Path) -> Option<String> {
    if std::fs::metadata(path).ok()?.len() > MAX_EMBEDDED_IMAGE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some(format!(
        "data:{};base64,{}",
        media_type(path),
        base64(&bytes)
    ))
}

/// The type to label an image with, from its extension.
///
/// `application/octet-stream` for anything else is not a guess: it is the
/// truthful "unknown". A platform that refuses it is more use than one that
/// takes the bytes as something they are not.
fn media_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("bmp") => "image/bmp",
        _ => "application/octet-stream",
    }
}

/// Standard base64 with `=` padding, written out here rather than pulled in.
///
/// The same trade [`crate::image`] makes for its percent-decoder: thirty lines
/// of pure function over bytes, against a dependency tree. Being a pure
/// function over bytes is also what makes it testable against a reference.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = u32::from(chunk[0]);
        let second = u32::from(chunk.get(1).copied().unwrap_or(0));
        let third = u32::from(chunk.get(2).copied().unwrap_or(0));
        let triple = (first << 16) | (second << 8) | third;

        out.push(char::from(ALPHABET[(triple >> 18) as usize & 0x3f]));
        out.push(char::from(ALPHABET[(triple >> 12) as usize & 0x3f]));
        out.push(if chunk.len() > 1 {
            char::from(ALPHABET[(triple >> 6) as usize & 0x3f])
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            char::from(ALPHABET[triple as usize & 0x3f])
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Every construct the compiler knows how to write, so a recipe that
    /// forgets one fails here rather than in somebody's paste.
    const EVERYTHING: &str = r"
# Heading one

## Heading two

### Heading three

#### Heading four

##### Heading five

###### Heading six

A paragraph with `inline code`, *emphasis*, **strong**, ~~struck~~,
[a link](https://example.com), and ![a picture](shot.png).

> A quotation.

- one
- [ ] not done
- [x] done

1. first
2. second

```rust
let x = 1;
```

| a | b |
| - | - |
| 1 | 2 |

---

A footnote[^note].

[^note]: The note.
";

    /// A scratch directory private to one test, so the tests can run in
    /// parallel without treading on each other's files.
    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("md-core-typeset-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The `src` of the first `<img>` in `html`.
    fn src_of(html: &str) -> Option<&str> {
        let rest = &html[html.find("src=\"")? + 5..];
        Some(&rest[..rest.find('"')?])
    }

    /// A base64 decoder written from the definition, so the encoder above is
    /// checked against something other than itself.
    fn decode_base64(text: &str) -> Vec<u8> {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

        let value = |byte: u8| -> u32 {
            ALPHABET
                .iter()
                .position(|&one| one == byte)
                .expect("every digit to be in the alphabet") as u32
        };

        let digits: Vec<u8> = text.bytes().filter(|byte| *byte != b'=').collect();
        let mut out = Vec::with_capacity(digits.len() / 4 * 3);
        for chunk in digits.chunks(4) {
            let mut triple = 0u32;
            for (index, byte) in chunk.iter().enumerate() {
                triple |= value(*byte) << (18 - 6 * index);
            }
            out.push((triple >> 16) as u8);
            if chunk.len() > 2 {
                out.push((triple >> 8) as u8);
            }
            if chunk.len() > 3 {
                out.push(triple as u8);
            }
        }
        out
    }

    #[test]
    fn a_section_replaces_a_heading_the_platform_will_not_take() {
        let wechat = fragment(Platform::WeChat, Style::Default, "## H2\n", None).html;
        assert!(!wechat.contains("<h2"), "{wechat}");
        assert!(
            wechat.contains("border-left: 4px solid #07c160"),
            "{wechat}"
        );
        // The wrapper and the heading, both as `<section>`.
        assert_eq!(wechat.matches("<section style=").count(), 2, "{wechat}");

        // 今日头条 takes real headings, and dresses them its own way.
        let toutiao = fragment(Platform::Toutiao, Style::Default, "## H2\n", None).html;
        assert!(toutiao.contains("<h2 style="), "{toutiao}");
        assert!(
            toutiao.contains("border-bottom: 2px solid #f04142"),
            "{toutiao}"
        );

        // 小红书 keeps two levels rather than one: the top two headings arrive
        // as `<h2>`, the rest as `<h3>`.
        let xiaohongshu = fragment(
            Platform::Xiaohongshu,
            Style::Default,
            "# One\n\n### Three\n",
            None,
        )
        .html;
        assert_eq!(
            xiaohongshu.matches("<h2 style=").count(),
            1,
            "{xiaohongshu}"
        );
        assert_eq!(
            xiaohongshu.matches("<h3 style=").count(),
            1,
            "{xiaohongshu}"
        );
    }

    /// The opening tags of everything in `html`, in order. The first one is the
    /// wrapper this module writes; the rest came from the compiler.
    fn opening_tags(html: &str) -> Vec<Tag<'_>> {
        let mut tags = Vec::new();
        let mut at = 0;
        while let Some(offset) = html[at..].find('<') {
            let start = at + offset;
            let Some(close) = tag_end(html, start) else {
                break;
            };
            let tag = Tag::parse(&html[start..=close]);
            at = close + 1;
            if tag.opening {
                tags.push(tag);
            }
        }
        tags
    }

    /// The styling is the whole point of a layout: an element the compiler
    /// writes that the recipe does not name arrives bare, which is the state
    /// this module exists to keep documents out of.
    ///
    /// Only the three recipes that claim to dress every element are checked.
    /// 小红书's is sparse on purpose — see the test below it.
    #[test]
    fn every_element_the_compiler_writes_carries_its_style() {
        for platform in [Platform::WeChat, Platform::Toutiao, Platform::Zhihu] {
            let recipe = platform.recipe();
            let layout = fragment(platform, Style::Default, EVERYTHING, None);
            let tags = opening_tags(&layout.html);
            assert!(tags.len() > 1, "{}", layout.html);

            for tag in tags.iter().skip(1) {
                let Some((to, style)) = recipe.element(tag.name) else {
                    panic!(
                        "{platform:?}: <{}> is not in the recipe, so it arrives unstyled\n{}",
                        tag.name, layout.html
                    );
                };
                if style.is_empty() {
                    // A deliberate silence, written as one rather than
                    // overlooked: `<br>`, `<tr>`, the table's row groups.
                    continue;
                }
                assert!(
                    has_attribute(tag, "style"),
                    "{platform:?}: <{}> was written as <{to}> without a style\n{}",
                    tag.name,
                    layout.html
                );
            }
        }
    }

    /// 小红书's recipe is the sparse one: it keeps the paragraph, the heading
    /// and the quote in shape and hands the rest to the platform's own
    /// normaliser, which would rewrite anything it was given anyway.
    #[test]
    fn the_xiaohongshu_recipe_styles_only_what_it_names() {
        let layout = fragment(Platform::Xiaohongshu, Style::Default, EVERYTHING, None);
        assert!(layout.html.contains("<p style="), "{}", layout.html);
        assert!(
            layout.html.contains("<blockquote style="),
            "{}",
            layout.html
        );
        assert!(
            layout
                .html
                .contains("<img src=\"shot.png\" alt=\"a picture\" style="),
            "{}",
            layout.html
        );
        // Left alone, including their attributes: not a claim about them.
        assert!(layout.html.contains("<ul>"), "{}", layout.html);
        assert!(layout.html.contains("<hr />"), "{}", layout.html);
        assert!(layout.html.contains("<table>"), "{}", layout.html);
    }

    /// What a platform drops, it drops — carrying it across only produces a
    /// document that claims a styling it does not have.
    #[test]
    fn no_class_or_style_block_survives_the_wechat_layout() {
        let layout = fragment(Platform::WeChat, Style::Default, EVERYTHING, None);
        assert!(!layout.html.contains("class="), "{}", layout.html);
        assert!(!layout.html.contains("id="), "{}", layout.html);
        assert!(!layout.html.contains("<style"), "{}", layout.html);
        assert!(!layout.html.contains("<input"), "{}", layout.html);
    }

    /// The premise of the walk: the compiler escapes `<` and `>` everywhere but
    /// inside a tag, so a `<` in the output always begins one. If that ever
    /// stopped holding, this scanner would be reading a document as markup.
    #[test]
    fn raw_html_in_the_source_stays_escaped() {
        let layout = fragment(
            Platform::WeChat,
            Style::Default,
            "<script>alert(1)</script>\n",
            None,
        );
        assert!(!layout.html.contains("<script>"), "{}", layout.html);
        assert!(layout.html.contains("&lt;script&gt;"), "{}", layout.html);
    }

    /// `sr-only` is the parser saying "for screen readers". Printed, it is the
    /// word "Footnotes" above the notes, which reads as a heading the author
    /// never wrote.
    #[test]
    fn the_footnote_label_is_left_out_rather_than_shown() {
        // 知乎 keeps classes, so this is the recipe where leaving it in would
        // have been most visible.
        let layout = fragment(Platform::Zhihu, Style::Default, EVERYTHING, None);
        assert!(!layout.html.contains("sr-only"), "{}", layout.html);
        assert!(!layout.html.contains("Footnotes"), "{}", layout.html);
        // The note itself is the document, and stays.
        assert!(layout.html.contains("The note."), "{}", layout.html);
    }

    #[test]
    fn a_task_list_loses_its_input_and_keeps_its_marks() {
        let layout = fragment(
            Platform::WeChat,
            Style::Default,
            "- [x] done\n- [ ] todo\n",
            None,
        );
        assert!(layout.html.contains("☑ done"), "{}", layout.html);
        assert!(layout.html.contains("☐ todo"), "{}", layout.html);
        assert!(!layout.html.contains("<input"), "{}", layout.html);
        // `☑` replaces the bullet, so the bullet has to go.
        assert!(layout.html.contains("list-style: none"), "{}", layout.html);
    }

    /// An ordinary list is not a task list, and must keep its bullets.
    #[test]
    fn a_plain_list_keeps_its_bullets() {
        let layout = fragment(Platform::WeChat, Style::Default, "- one\n- two\n", None);
        assert!(!layout.html.contains("list-style: none"), "{}", layout.html);
    }

    /// Inside a block the pill's background and padding would draw a second box
    /// inside the first.
    #[test]
    fn code_inside_a_block_loses_the_inline_pill() {
        let block = fragment(
            Platform::WeChat,
            Style::Default,
            "```\nlet x = 1;\n```\n",
            None,
        )
        .html;
        assert!(block.contains("background: none; padding: 0;"), "{block}");
        assert!(!block.contains("background: #f0f1f3"), "{block}");

        // ...and code in a sentence keeps it.
        let inline = fragment(Platform::WeChat, Style::Default, "a `b` c\n", None).html;
        assert!(inline.contains("background: #f0f1f3"), "{inline}");

        // 小红书 leaves both alone, which is a silence and not an empty claim.
        let bare = fragment(
            Platform::Xiaohongshu,
            Style::Default,
            "```\nlet x = 1;\n```\n",
            None,
        )
        .html;
        assert!(bare.contains("<pre>"), "{bare}");
        assert!(bare.contains("<code>"), "{bare}");
        assert!(!bare.contains("<code style"), "{bare}");
    }

    #[test]
    fn a_relative_image_becomes_a_data_uri() {
        let dir = scratch("image");
        let bytes: Vec<u8> = vec![
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0xff, 0x7f,
        ];
        std::fs::write(dir.join("shot.png"), &bytes).unwrap();

        let layout = fragment(
            Platform::WeChat,
            Style::Default,
            "![a picture](shot.png)\n",
            Some(dir.as_path()),
        );
        assert!(
            layout.unresolved_images.is_empty(),
            "{:?}",
            layout.unresolved_images
        );

        let src = src_of(&layout.html).expect("an img carrying a src");
        let encoded = src
            .strip_prefix("data:image/png;base64,")
            .unwrap_or_else(|| panic!("{src}"));
        assert_eq!(decode_base64(encoded), bytes);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The image URL arrives as the compiler escaped it, so the file's name is
    /// not what the attribute says until the escaping is undone.
    #[test]
    fn an_escaped_ampersand_is_undone_before_the_file_is_looked_for() {
        let dir = scratch("ampersand");
        std::fs::write(dir.join("a&b.png"), b"x").unwrap();

        let mut unresolved = Vec::new();
        let embedded = embed_image("a&amp;b.png", Some(dir.as_path()), &mut unresolved);
        assert!(embedded.starts_with("data:image/png;base64,"), "{embedded}");
        assert!(unresolved.is_empty(), "{unresolved:?}");

        // The same thing end to end, whichever way the compiler spells it.
        let layout = fragment(
            Platform::WeChat,
            Style::Default,
            "![](a&b.png)\n",
            Some(dir.as_path()),
        );
        assert!(
            layout.unresolved_images.is_empty(),
            "{:?}",
            layout.unresolved_images
        );
        assert!(
            src_of(&layout.html).is_some_and(|src| src.starts_with("data:image/png;base64,")),
            "{}",
            layout.html
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_image_that_is_not_on_disk_is_left_as_written_and_reported() {
        let dir = scratch("missing-image");

        let layout = fragment(
            Platform::WeChat,
            Style::Default,
            "![](gone.png)\n",
            Some(dir.as_path()),
        );
        assert_eq!(layout.unresolved_images, vec!["gone.png".to_string()]);
        assert!(layout.html.contains("src=\"gone.png\""), "{}", layout.html);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_remote_image_is_never_read_from_disk() {
        let dir = scratch("remote-image");

        for url in ["https://example.com/a.png", "http://example.com/a.png"] {
            let layout = fragment(
                Platform::WeChat,
                Style::Default,
                &format!("![]({url})\n"),
                Some(dir.as_path()),
            );
            assert!(layout.unresolved_images.is_empty(), "{url}");
            assert!(
                layout.html.contains(&format!("src=\"{url}\"")),
                "{}",
                layout.html
            );
        }

        // A URL that means something on its own is not ours to resolve even
        // when it never survives the compiler's own protocol check.
        let mut unresolved = Vec::new();
        assert_eq!(
            embed_image(
                "data:image/png;base64,AAAA",
                Some(dir.as_path()),
                &mut unresolved
            ),
            "data:image/png;base64,AAAA"
        );
        assert!(unresolved.is_empty(), "{unresolved:?}");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// A document with no file behind it has no folder to resolve against, so
    /// images stay as their author wrote them rather than becoming a report of
    /// files that were never looked for.
    #[test]
    fn an_unsaved_document_reports_no_missing_images() {
        let layout = fragment(Platform::WeChat, Style::Default, "![](shot.png)\n", None);
        assert!(layout.unresolved_images.is_empty());
        assert!(layout.html.contains("src=\"shot.png\""), "{}", layout.html);
    }

    #[test]
    fn an_oversized_image_is_left_alone() {
        let dir = scratch("oversized");
        let path = dir.join("big.png");
        let file = std::fs::File::create(&path).unwrap();
        // Sparse, so a test does not have to write eight megabytes to check
        // that they would not have been read.
        file.set_len(MAX_EMBEDDED_IMAGE_BYTES + 1).unwrap();
        drop(file);

        let layout = fragment(
            Platform::WeChat,
            Style::Default,
            "![big](big.png)\n",
            Some(dir.as_path()),
        );
        assert_eq!(layout.unresolved_images, vec!["big.png".to_string()]);
        assert!(layout.html.contains("src=\"big.png\""), "{}", layout.html);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_plain_text_is_the_document_without_its_markup() {
        let layout = fragment(
            Platform::WeChat,
            Style::Default,
            "# 标题\n\nFish & chips\n",
            None,
        );
        assert!(!layout.plain.contains('<'), "{}", layout.plain);
        assert_eq!(layout.plain, "标题\n\nFish & chips");

        // The parser's escaping is read back, and an entity in the source
        // becomes the character it stands for.
        let entities = fragment(Platform::WeChat, Style::Default, "a &amp; b\n", None);
        assert_eq!(entities.plain, "a & b");
        let angle = fragment(Platform::WeChat, Style::Default, "1 &lt; 2\n", None);
        assert_eq!(angle.plain, "1 < 2");
    }

    #[test]
    fn an_image_carries_its_alt_text_into_the_plain_text() {
        let layout = fragment(
            Platform::WeChat,
            Style::Default,
            "![a diagram](shot.png)\n",
            None,
        );
        assert_eq!(layout.plain, "a diagram");
    }

    #[test]
    fn an_empty_document_is_just_the_wrapper() {
        let layout = fragment(Platform::WeChat, Style::Default, "", None);
        assert!(layout.plain.is_empty(), "{}", layout.plain);
        assert!(
            layout.html.starts_with("<section style="),
            "{}",
            layout.html
        );
        assert!(layout.html.ends_with("</section>\n"), "{}", layout.html);
    }

    /// The compiler writes a link's `title` without escaping it, so a title
    /// holding a quote produces an attribute value that a quote-counting scan
    /// would pair off wrongly — and an odd number of them would run the scan
    /// past this tag into the next.
    #[test]
    fn a_title_holding_a_quote_does_not_run_the_scan_past_the_tag() {
        let layout = fragment(
            Platform::WeChat,
            Style::Default,
            "[x](https://e.com 'say \"hi\"')\n\nA second paragraph.\n",
            None,
        );
        assert!(layout.html.contains(">x</a>"), "{}", layout.html);
        assert!(
            layout.html.contains("A second paragraph."),
            "{}",
            layout.html
        );
        assert_eq!(layout.html.matches("</p>").count(), 2, "{}", layout.html);
    }

    #[test]
    fn the_page_for_a_platform_is_a_complete_document() {
        let whole = page(Platform::Zhihu, Style::Default, "笔记", "# 标题\n", None);
        assert!(whole.starts_with("<!DOCTYPE html>"), "{whole}");
        assert!(whole.contains("<meta charset=\"utf-8\">"), "{whole}");
        assert!(whole.contains("<title>笔记</title>"), "{whole}");
        assert!(whole.contains("标题"), "{whole}");
        assert!(whole.ends_with("</html>\n"), "{whole}");

        // The title is the user's file name, so it is escaped like any other.
        let escaped = page(Platform::Zhihu, Style::Default, "a & b <c>", "", None);
        assert!(
            escaped.contains("<title>a &amp; b &lt;c&gt;</title>"),
            "{escaped}"
        );
    }

    /// `Plain` is the page the export has always written, and the escape hatch
    /// when a platform's editor mangles a layout. A file exported under it has
    /// to come out as it did before there were layouts at all.
    #[test]
    fn the_plain_platform_still_produces_what_the_export_always_did() {
        let markdown = "# 标题\n\nA paragraph.\n";
        assert_eq!(
            page(Platform::Plain, Style::Default, "note", markdown, None),
            export::to_html_page("note", markdown)
        );
        assert_eq!(
            fragment(Platform::Plain, Style::Default, markdown, None).html,
            export::to_html_fragment(markdown)
        );

        // Images included: inlining them is a layout's promise, not this one's.
        let layout = fragment(
            Platform::Plain,
            Style::Default,
            "![](a.png)\n",
            Some(Path::new("/tmp")),
        );
        assert!(layout.html.contains("src=\"a.png\""), "{}", layout.html);
        assert!(layout.unresolved_images.is_empty());
    }

    #[test]
    fn every_platform_has_a_label_of_its_own() {
        let labels: Vec<&str> = Platform::ALL
            .iter()
            .map(|platform| platform.label())
            .collect();
        let mut unique = labels.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), labels.len(), "{labels:?}");
        assert!(labels.iter().all(|label| !label.is_empty()));
    }

    #[test]
    fn base64_matches_the_reference_vectors() {
        // RFC 4648's own examples, which is the one thing a self-written
        // encoder has no way to agree with itself about.
        for (bytes, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(bytes.as_bytes()), encoded, "{bytes}");
        }

        // Every byte value, at every length modulo three.
        let all: Vec<u8> = (0..=255u8).collect();
        for length in 0..=all.len() {
            let bytes = &all[..length];
            assert_eq!(decode_base64(&base64(bytes)), bytes, "{length}");
        }
    }

    #[test]
    fn an_images_type_comes_from_its_extension() {
        assert_eq!(media_type(Path::new("a.png")), "image/png");
        assert_eq!(media_type(Path::new("a.JPG")), "image/jpeg");
        assert_eq!(media_type(Path::new("a.jpeg")), "image/jpeg");
        assert_eq!(media_type(Path::new("a.gif")), "image/gif");
        assert_eq!(media_type(Path::new("a.webp")), "image/webp");
        assert_eq!(media_type(Path::new("a.svg")), "image/svg+xml");
        assert_eq!(media_type(Path::new("a.bmp")), "image/bmp");
        // Not a guess, and not a lie either.
        assert_eq!(media_type(Path::new("a.tiff")), "application/octet-stream");
        assert_eq!(media_type(Path::new("a")), "application/octet-stream");
    }

    // -----------------------------------------------------------------------
    // Styles
    // -----------------------------------------------------------------------

    /// The inline style written on the first opening `<name …>` in `html`.
    ///
    /// Empty for an element that was written without one, which is a state
    /// 小红书's recipe puts things in on purpose.
    fn style_of(html: &str, name: &str) -> String {
        opening_tags(html)
            .into_iter()
            .find(|tag| tag.name == name)
            .and_then(|tag| tag.attribute("style"))
            .unwrap_or_default()
            .to_string()
    }

    /// Every inline style written on an opening `<name …>`, in document order.
    fn styles_of(html: &str, name: &str) -> Vec<String> {
        opening_tags(html)
            .into_iter()
            .filter(|tag| tag.name == name)
            .map(|tag| tag.attribute("style").unwrap_or_default().to_string())
            .collect()
    }

    /// The layout nobody asked to have styled is the layout this module
    /// produced before styles existed.
    ///
    /// A golden rather than a comparison against a second code path: the claim
    /// is not that `Default` agrees with something else, it is that `Default`
    /// is unchanged. A style leaking into the default dress changes this
    /// string, and nothing else would notice.
    #[test]
    fn the_default_style_is_the_platform_layout_byte_for_byte() {
        // 公众号 `# Title` and one paragraph, every byte of it.
        assert_eq!(
            fragment(Platform::WeChat, Style::Default, "# Title\n\nBody.\n", None).html,
            concat!(
                r#"<section style="font-size: 16px; line-height: 1.75; color: #3f3f3f;">"#,
                r#"<section style="margin: 32px 0 16px; padding-left: 10px; "#,
                r#"border-left: 4px solid #07c160; font-size: 22px; line-height: 1.4; "#,
                r#"font-weight: bold; color: #1a1a1a;">Title</section>"#,
                "\n",
                r#"<p style="margin: 0 0 16px; font-size: 16px; line-height: 1.75; "#,
                r#"color: #3f3f3f;">Body.</p>"#,
                "\n",
                "</section>\n",
            ),
        );

        // 今日头条, where the heading keeps its own tag and the wrapper is not
        // a `<section>` like the other three.
        assert_eq!(
            fragment(
                Platform::Toutiao,
                Style::Default,
                "# Title\n\nBody.\n",
                None
            )
            .html,
            concat!(
                r#"<section style="font-size: 17px; line-height: 1.8; color: #333333;">"#,
                r#"<h1 style="margin: 32px 0 18px; padding-bottom: 8px; "#,
                r#"border-bottom: 2px solid #f04142; font-size: 24px; line-height: 1.4; "#,
                r#"color: #222222;">Title</h1>"#,
                "\n",
                r#"<p style="margin: 0 0 18px; font-size: 17px; line-height: 1.8; "#,
                r#"color: #333333;">Body.</p>"#,
                "\n",
                "</section>\n",
            ),
        );

        // 知乎, whose wrapper is a `<div>` and whose recipe is the only one
        // that carries a heading rule at a level other than the top two.
        assert_eq!(
            fragment(Platform::Zhihu, Style::Default, "# Title\n\nBody.\n", None).html,
            concat!(
                r#"<div style="font-size: 16px; line-height: 1.7; color: #1a1a1a;">"#,
                r#"<h1 style="margin: 30px 0 16px; padding-bottom: 6px; "#,
                r#"border-bottom: 1px solid #e5e5e5; font-size: 24px; line-height: 1.4; "#,
                r#"color: #121212;">Title</h1>"#,
                "\n",
                r#"<p style="margin: 0 0 16px; font-size: 16px; line-height: 1.7; "#,
                r#"color: #1a1a1a;">Body.</p>"#,
                "\n",
                "</div>\n",
            ),
        );
    }

    /// A style is dress. The tag a thing is written as, and whether it carries
    /// a class, are structure — the platform decides those, and no style may
    /// touch them.
    #[test]
    fn a_style_never_changes_a_tag_name_or_a_class() {
        let shape = |html: &str| -> Vec<(String, Option<String>)> {
            opening_tags(html)
                .into_iter()
                .map(|tag| {
                    (
                        tag.name.to_string(),
                        tag.attribute("class").map(str::to_string),
                    )
                })
                .collect()
        };

        for platform in Platform::ALL {
            let default = fragment(platform, Style::Default, EVERYTHING, None).html;
            for style in [Style::Minimal, Style::Magazine] {
                let dressed = fragment(platform, style, EVERYTHING, None).html;
                assert_eq!(
                    shape(&default),
                    shape(&dressed),
                    "{platform:?} under {style:?} rewrote the structure"
                );
            }
        }
    }

    /// 简约 undecorates: what the platform hung on a heading comes off, and
    /// nothing goes back on.
    #[test]
    fn minimal_drops_every_heading_decoration() {
        // A platform whose recipe keeps real `<h*>` tags, so each level can be
        // found by name.
        for platform in [Platform::Toutiao, Platform::Zhihu] {
            let html = fragment(platform, Style::Minimal, EVERYTHING, None).html;
            for level in 1..=6 {
                let name = format!("h{level}");
                let style = style_of(&html, &name);
                assert!(!style.is_empty(), "{platform:?} <{name}>: {html}");
                for property in DECORATION {
                    assert!(
                        !style.contains(property),
                        "{platform:?} <{name}> kept {property}: {style}"
                    );
                }
            }
        }
    }

    /// 杂志 rules under the top three levels and stops there.
    #[test]
    fn magazine_puts_a_rule_under_the_top_three_headings() {
        let html = fragment(Platform::Zhihu, Style::Magazine, EVERYTHING, None).html;

        assert!(
            style_of(&html, "h1").contains("border-bottom: 3px solid #111111"),
            "{}",
            style_of(&html, "h1")
        );
        assert!(style_of(&html, "h2").contains("border-bottom: 2px solid #111111"));
        assert!(style_of(&html, "h3").contains("border-bottom: 1px solid #d8d8d8"));
        assert!(!style_of(&html, "h4").contains("border-bottom"));
        assert!(!style_of(&html, "h5").contains("border-bottom"));
        assert!(!style_of(&html, "h6").contains("border-bottom"));

        // The platform's own rule is replaced rather than left under this one.
        assert!(
            !style_of(&html, "h1").contains("#e5e5e5"),
            "知乎's own rule is still on the heading: {}",
            style_of(&html, "h1")
        );
    }

    /// `#1a1a1a` is 公众号's *heading* colour and 知乎's *body* colour. One
    /// flat colour table would read whichever it met first as the other.
    #[test]
    fn a_heading_colour_is_not_mistaken_for_a_body_colour() {
        // 公众号 writes headings as `<section>`, and the wrapper is one too, so
        // the heading is the second of them.
        let wechat = fragment(Platform::WeChat, Style::Minimal, "# Title\n\nBody.\n", None).html;
        let heading = styles_of(&wechat, "section")
            .get(1)
            .expect("the heading")
            .clone();
        assert!(heading.contains("color: #2b2b2b"), "{heading}");
        assert!(!heading.contains("#555555"), "{heading}");

        // 知乎's body text shares that source colour and must go the other way.
        let zhihu = fragment(Platform::Zhihu, Style::Minimal, "# Title\n\nBody.\n", None).html;
        assert!(style_of(&zhihu, "p").contains("color: #555555"));
        assert!(!style_of(&zhihu, "p").contains("#2b2b2b"));

        // And the same pair under 杂志, where the two ends are further apart.
        let zhihu = fragment(Platform::Zhihu, Style::Magazine, "# Title\n\nBody.\n", None).html;
        assert!(style_of(&zhihu, "h1").contains("color: #000000"));
        assert!(style_of(&zhihu, "p").contains("color: #1a1a1a"));
    }

    /// A size with a fraction in it is a size nobody chose, printed to a place
    /// no editor will show it.
    #[test]
    fn a_style_scales_every_font_size_to_whole_pixels() {
        for style in [Style::Minimal, Style::Magazine] {
            for platform in Platform::ALL {
                let html = fragment(platform, style, EVERYTHING, None).html;
                for tag in opening_tags(&html) {
                    let Some(rules) = tag.attribute("style") else {
                        continue;
                    };
                    for declaration in rules.split(';') {
                        let Some(size) = declaration.trim().strip_prefix("font-size:") else {
                            continue;
                        };
                        let size = size.trim();
                        let pixels = size.strip_suffix("px").unwrap_or_else(|| {
                            panic!("{platform:?} {style:?}: font-size: {size} is not in px")
                        });
                        assert!(
                            pixels.parse::<u32>().is_ok(),
                            "{platform:?} {style:?}: font-size: {size} has a fraction in it"
                        );
                    }
                }
            }
        }
    }

    /// The wrapper and inline code carry sizes of their own. A style that
    /// scaled the elements and not these would leave the document at one size
    /// and its frame at another.
    #[test]
    fn a_style_leaves_the_wrapper_and_inline_code_dressed_too() {
        let minimal = fragment(Platform::WeChat, Style::Minimal, EVERYTHING, None).html;
        let wrapper = opening_tags(&minimal)[0]
            .attribute("style")
            .unwrap_or_default()
            .to_string();
        assert!(wrapper.contains("font-size: 15px"), "{wrapper}");
        assert!(wrapper.contains("line-height: 1.65"), "{wrapper}");
        // 14px, the inline pill's own size, taken down with everything else.
        assert!(style_of(&minimal, "code").contains("font-size: 13px"));

        let magazine = fragment(Platform::WeChat, Style::Magazine, EVERYTHING, None).html;
        let wrapper = opening_tags(&magazine)[0]
            .attribute("style")
            .unwrap_or_default()
            .to_string();
        assert!(wrapper.contains("font-size: 18px"), "{wrapper}");
        assert!(wrapper.contains("line-height: 1.95"), "{wrapper}");
        assert!(style_of(&magazine, "code").contains("font-size: 15px"));
    }

    /// A heading's leading is tuned to its own size, so it is the one thing a
    /// style's `line-height` does not overwrite.
    #[test]
    fn a_style_leaves_a_headings_leading_alone() {
        let html = fragment(Platform::Zhihu, Style::Magazine, "# Title\n", None).html;
        let heading = style_of(&html, "h1");
        assert!(heading.contains("line-height: 1.4"), "{heading}");
        assert!(!heading.contains("1.95"), "{heading}");
    }

    /// No layout means nothing to dress: 知乎's page has its styling in a
    /// stylesheet, and a style here would have to become a second one.
    #[test]
    fn the_plain_platform_ignores_the_style() {
        for style in Style::ALL {
            assert_eq!(
                fragment(Platform::Plain, style, EVERYTHING, None).html,
                export::to_html_fragment(EVERYTHING)
            );
            assert_eq!(
                page(Platform::Plain, style, "note", EVERYTHING, None),
                export::to_html_page("note", EVERYTHING)
            );
        }
    }

    #[test]
    fn every_style_has_a_label_of_its_own() {
        let mut labels = Vec::new();
        for style in Style::ALL {
            assert!(!labels.contains(&style.label()), "{style:?}");
            labels.push(style.label());
        }
        assert_eq!(labels.len(), Style::ALL.len());
    }
}
