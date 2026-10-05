//! Application actions.
//!
//! Uses `gpui_kit::actions!` rather than GPUI's own macro: the derive it expands
//! to spells the trait as `gpui::Action`, which does not resolve when GPUI is
//! consumed through the `gpui-kit` facade.

use gpui_kit::actions;

actions!(md, [Quit]);
