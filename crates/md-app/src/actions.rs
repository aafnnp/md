//! Application actions.
//!
//! Uses `gpui_kit::actions!` rather than GPUI's own macro: the derive it expands
//! to spells the trait as `gpui::Action`, which does not resolve when GPUI is
//! consumed through the `gpui-kit` facade. The parameterised `SelectTab` below
//! derives the same way by hand, for the same reason.

use gpui_kit::actions;
use md_core::typeset::Platform;

actions!(
    md,
    [
        Quit,
        CloseTab,
        ToggleTheme,
        OpenSettings,
        OpenFile,
        Save,
        SaveAs,
        ExportHtml,
        CopyLayout
    ]
);

/// Switch to the tab strip's `n`th tab, counting from zero.
///
/// `no_json` because the derive would otherwise demand `Deserialize` and
/// `JsonSchema`, and pulling in `serde`/`schemars` just to name a tab is not
/// worth it. The cost is that this one action cannot be built from a JSON
/// keymap; the unit actions above still can.
#[derive(Clone, PartialEq, Default, Debug, gpui_kit::Action)]
#[action(namespace = md, no_json)]
pub struct SelectTab(pub usize);

/// Close the tab strip's `n`th tab, counting from zero.
///
/// A separate action from `SelectTab` because the close button on a tab has to
/// close *that* tab rather than whichever one is in front — clicking the cross
/// on a background tab should not first bring it forward, and on a dirty tab it
/// must not leave the user answering a question about the wrong document.
#[derive(Clone, PartialEq, Default, Debug, gpui_kit::Action)]
#[action(namespace = md, no_json)]
pub struct CloseTabAt(pub usize);

/// Lay the document out for a platform from now on, when exporting or copying.
///
/// A parameterised action for the same reason as the two above, and dispatched
/// rather than called directly so the status bar's menu and the settings panel
/// reach one handler between them. What it sets is a setting, so the choice
/// outlives the window it was made in.
#[derive(Clone, PartialEq, Default, Debug, gpui_kit::Action)]
#[action(namespace = md, no_json)]
pub struct SetTypesetting(pub Platform);
