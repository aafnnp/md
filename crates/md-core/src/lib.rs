//! Pure, GPUI-free logic for the `md` editor.
//!
//! Nothing in this crate may depend on `gpui` or `gpui-kit`. That keeps
//! `cargo test -p md-core` runnable in CI without Xcode, Metal, or a display
//! server, which is what makes the logic here actually testable.

pub mod document;
pub mod fs;
pub mod recent;

pub use document::Document;
pub use recent::RecentFiles;
