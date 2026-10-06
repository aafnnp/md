//! The settings the app is running with, and putting them into force.
//!
//! [`md_core::settings`] owns the model and the file. This is the half that has
//! to touch GPUI: it holds the loaded values where an action handler can reach
//! them, and translates them into a live [`Theme`].

use std::io;

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;
use md_core::settings::{Settings, ThemePreference};

/// The settings in force for this run.
///
/// A global rather than a field on the workspace because the theme toggle is an
/// action, and an action handler is handed the app context and nothing else.
pub struct AppSettings(pub Settings);

impl Global for AppSettings {}

impl AppSettings {
    /// The settings in force, or the defaults if startup never got to install
    /// them — as in a test that builds a view directly.
    pub fn current(cx: &App) -> Settings {
        cx.try_global::<AppSettings>()
            .map(|settings| settings.0.clone())
            .unwrap_or_default()
    }

    /// Remember `settings` without applying them.
    ///
    /// Separate from [`apply`] so the two can be read as the two things they
    /// are: what we believe, and what is on screen.
    pub fn set(settings: Settings, cx: &mut App) {
        cx.set_global(AppSettings(settings));
    }
}

/// The margin the preview leaves when the settings file does not name one.
///
/// Not zero. Rendered text set flush against the pane edge reads as though it
/// has been cropped, and the preview is the one pane in the window with nothing
/// else to hold the text off its border.
pub const DEFAULT_PREVIEW_PADDING: f32 = 24.0;

/// The space to leave around the rendered document, in pixels.
pub fn preview_padding(settings: &Settings) -> f32 {
    settings.preview_padding.unwrap_or(DEFAULT_PREVIEW_PADDING)
}

/// Put `settings` into force: the theme first, then the fonts.
///
/// Two steps on purpose. `Theme::change` loads the registered theme for the
/// mode, and that load re-resolves the default font families, so a font set in
/// the same call would be overwritten by the default it was meant to replace.
pub fn apply(settings: &Settings, window: Option<&mut Window>, cx: &mut App) {
    match settings.theme {
        ThemePreference::System => Theme::sync_system_appearance(window, cx),
        ThemePreference::Light => Theme::change(ThemeMode::Light, window, cx),
        ThemePreference::Dark => Theme::change(ThemeMode::Dark, window, cx),
    }

    apply_fonts(settings, cx);
}

/// Put the fonts into force, leaving the mode alone.
///
/// Split out of [`apply`] for the settings dialog, where a font size typed one
/// digit at a time would otherwise reload the whole registered theme on every
/// keystroke — and be handed no window to do it with, since the dialog's fields
/// are watched through subscriptions, which carry no window.
pub fn apply_fonts(settings: &Settings, cx: &mut App) {
    Theme::update(cx, |theme| {
        if let Some(family) = &settings.font_family {
            theme.font_family = family.clone().into();
        }
        if let Some(size) = settings.font_size {
            theme.font_size = px(size);
        }
        if let Some(family) = &settings.mono_font_family {
            theme.mono_font_family = family.clone().into();
        }
        if let Some(size) = settings.mono_font_size {
            theme.mono_font_size = px(size);
        }
    });
}

/// Keep the theme in step with the operating system's appearance.
///
/// Only while the preference is `System`: a deliberate light or dark choice is
/// meant to hold however the system changes. The returned [`Subscription`] must
/// be kept — dropping it stops the callback.
pub fn follow_system_appearance(window: &mut Window) -> Subscription {
    window.observe_window_appearance(|window, cx| {
        if AppSettings::current(cx).theme == ThemePreference::System {
            Theme::sync_system_appearance(Some(window), cx);
        }
    })
}

/// Advance the theme preference one step, apply it, and write it down.
///
/// Returns the new preference, and the reason the choice could not be saved if
/// it could not. A failed write is reported rather than refused: the theme did
/// change, and saying so is more use than pretending the click did nothing.
pub fn cycle_theme(window: &mut Window, cx: &mut App) -> (ThemePreference, Option<io::Error>) {
    let mut settings = AppSettings::current(cx);
    settings.theme = settings.theme.next();

    AppSettings::set(settings.clone(), cx);
    apply(&settings, Some(window), cx);

    let failure = settings.save().err();
    (settings.theme, failure)
}

#[cfg(test)]
mod tests {
    // Imported narrowly: `use super::*` would drag in the `gpui_kit::*` glob,
    // whose `test` attribute macro shadows the built-in `#[test]`.
    use super::{AppSettings, apply};
    use gpui_kit::component::{Theme, ThemeMode};
    use gpui_kit::{TestAppContext, px};
    use md_core::settings::{Settings, ThemePreference};

    /// Bring the test app up to the state the real one starts in — the
    /// components' `init` is what installs the theme global [`apply`] edits.
    fn init(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
    }

    fn with_theme<T>(cx: &TestAppContext, read: impl FnOnce(&Theme) -> T) -> T {
        cx.update(|cx| read(Theme::global(cx)))
    }

    fn settings(theme: ThemePreference) -> Settings {
        Settings {
            theme,
            ..Default::default()
        }
    }

    #[gpui_kit::test]
    fn the_global_is_the_defaults_until_something_is_installed(cx: &mut TestAppContext) {
        init(cx);
        assert_eq!(
            cx.update(|cx| AppSettings::current(cx)),
            Settings::default()
        );

        let dark = settings(ThemePreference::Dark);
        cx.update(|cx| AppSettings::set(dark.clone(), cx));
        assert_eq!(cx.update(|cx| AppSettings::current(cx)), dark);
    }

    #[gpui_kit::test]
    fn applying_a_preference_changes_the_live_theme(cx: &mut TestAppContext) {
        init(cx);

        cx.update(|cx| apply(&settings(ThemePreference::Dark), None, cx));
        assert!(with_theme(cx, |theme| theme.is_dark()));

        cx.update(|cx| apply(&settings(ThemePreference::Light), None, cx));
        assert!(!with_theme(cx, |theme| theme.is_dark()));
    }

    #[gpui_kit::test]
    fn following_the_system_takes_the_appearance_the_app_reports(cx: &mut TestAppContext) {
        init(cx);

        cx.update(|cx| {
            apply(&settings(ThemePreference::System), None, cx);

            // No window was handed in, so the app's own appearance is the
            // answer — and the mapping from that to a mode is what decides
            // which theme gets loaded.
            let expected = ThemeMode::from(cx.window_appearance());
            assert_eq!(Theme::global(cx).is_dark(), expected.is_dark());
        });
    }

    #[gpui_kit::test]
    fn the_fonts_survive_the_theme_change_they_travel_with(cx: &mut TestAppContext) {
        init(cx);

        cx.update(|cx| {
            let settings = Settings {
                theme: ThemePreference::Dark,
                font_family: Some("Helvetica".to_string()),
                font_size: Some(21.0),
                mono_font_family: Some("Iosevka".to_string()),
                mono_font_size: Some(19.0),
                ..Default::default()
            };
            apply(&settings, None, cx);
        });

        // Switching mode reloads the mode's registered theme, and that reload
        // is where the default font families come from. Setting the fonts in
        // the same call as the mode is what would lose them.
        with_theme(cx, |theme| {
            assert_eq!(theme.font_family.to_string(), "Helvetica");
            assert_eq!(theme.font_size, px(21.0));
            assert_eq!(theme.mono_font_family.to_string(), "Iosevka");
            assert_eq!(theme.mono_font_size, px(19.0));
        });
    }
}
