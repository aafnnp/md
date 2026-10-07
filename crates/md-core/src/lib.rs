//! Pure, GPUI-free logic for the `md` editor.
//!
//! Nothing in this crate may depend on `gpui` or `gpui-kit`. That keeps
//! `cargo test -p md-core` runnable in CI without Xcode, Metal, or a display
//! server, which is what makes the logic here actually testable.

pub mod count;
pub mod document;
pub mod export;
pub mod fs;
pub mod image;
pub mod recent;
pub mod settings;
pub mod typeset;

pub use count::{Counts, counts};
pub use document::Document;
pub use export::to_html_page;
pub use recent::RecentFiles;
pub use settings::{Settings, ThemePreference};
pub use typeset::Platform;
