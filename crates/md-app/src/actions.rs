//! Application actions.
//!
//! Uses `gpui_kit::actions!` rather than GPUI's own macro: the derive it expands
//! to spells the trait as `gpui::Action`, which does not resolve when GPUI is
//! consumed through the `gpui-kit` facade. The parameterised `SelectTab` below
//! derives the same way by hand, for the same reason.

use gpui_kit::actions;

actions!(md, [Quit, CloseTab]);

/// Switch to the tab strip's `n`th tab, counting from zero.
///
/// `no_json` because the derive would otherwise demand `Deserialize` and
/// `JsonSchema`, and pulling in `serde`/`schemars` just to name a tab is not
/// worth it. The cost is that this one action cannot be built from a JSON
/// keymap; the unit actions above still can.
#[derive(Clone, PartialEq, Default, Debug, gpui_kit::Action)]
#[action(namespace = md, no_json)]
pub struct SelectTab(pub usize);
