//! The preferences the app remembers between runs, and where they are kept.
//!
//! The file is plain JSON in the platform's per-user configuration directory,
//! and it is meant to be read and edited by hand: every field but the theme is
//! optional, and a missing one means "use the built-in default" rather than
//! "zero". Writing it drops keys this build does not know about, so a setting
//! added by a newer version survives only until an older one saves.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::typeset::Platform;

/// The file name inside the platform's configuration directory.
pub const FILE_NAME: &str = "settings.json";

/// Font sizes outside this range are pulled back into it on load.
///
/// The file is hand-editable, and a wrong size does not fail loudly: it lays
/// out text that cannot be read, or that does not fit anywhere. Clamping turns
/// a typo into a slightly wrong size rather than an unusable window.
pub const FONT_SIZE_RANGE: (f32, f32) = (8.0, 48.0);

/// The same, for the width the source column is held to.
pub const EDITOR_WIDTH_RANGE: (f32, f32) = (320.0, 4000.0);

/// The same, for the space left around the preview.
///
/// Zero is allowed — it is the one setting here whose minimum is a real choice
/// rather than a mistake — and the upper bound is generous enough to push the
/// text into a narrow column on a wide window, which is what the setting is for.
pub const PREVIEW_PADDING_RANGE: (f32, f32) = (0.0, 200.0);

/// Which theme the app should use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    /// Follow the operating system's appearance, and keep following it when it
    /// changes.
    #[default]
    System,
    /// Light, whatever the system says.
    Light,
    /// Dark, whatever the system says.
    Dark,
}

impl ThemePreference {
    /// Every preference, in the order the toggle walks them.
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    /// The next preference in the cycle.
    ///
    /// The cycle comes back to `System`, which is what makes the toggle a way
    /// back to following the operating system rather than a one-way switch
    /// away from it.
    pub fn next(self) -> Self {
        match self {
            Self::System => Self::Light,
            Self::Light => Self::Dark,
            Self::Dark => Self::System,
        }
    }

    /// The name to show for this preference.
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

/// Everything the app remembers between runs.
///
/// `Default` is "follow the system and leave everything else to the theme",
/// which is also what a field missing from the file becomes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Light, dark, or whatever the system says.
    pub theme: ThemePreference,
    /// Which platform's typesetting an export or a copy is dressed for.
    ///
    /// App-wide rather than per document: it is a property of where the writer
    /// is publishing, and that does not change from one tab to the next. A
    /// field missing from the file becomes the default, `Plain`.
    pub typesetting: Platform,
    /// Family for everything but the source pane. `None` keeps the one the
    /// theme resolved, which is the platform's own interface font.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    /// Base size in pixels, for the interface as well as the text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f32>,
    /// Family for the source pane, which is monospaced.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mono_font_family: Option<String>,
    /// Size in pixels for the source pane.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mono_font_size: Option<f32>,
    /// The width in pixels the source column is held to, however wide the
    /// window gets. Long lines are hard to read, and a maximised window gives
    /// them nothing to stop at. `None` lets the column fill its pane.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editor_max_width: Option<f32>,
    /// Space in pixels left between the rendered document and the edges of the
    /// preview pane. `None` uses the app's own margin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_padding: Option<f32>,
}

impl Settings {
    /// The settings with every value the app cannot use dropped or pulled back
    /// into range.
    ///
    /// Applied once, on load, rather than where each value is used: a
    /// hand-edited file has exactly one place it can go wrong, and this is it.
    pub fn sanitized(mut self) -> Self {
        self.font_family = family(self.font_family);
        self.font_size = size(self.font_size, FONT_SIZE_RANGE);
        self.mono_font_family = family(self.mono_font_family);
        self.mono_font_size = size(self.mono_font_size, FONT_SIZE_RANGE);
        self.editor_max_width = size(self.editor_max_width, EDITOR_WIDTH_RANGE);
        self.preview_padding = size(self.preview_padding, PREVIEW_PADDING_RANGE);
        self
    }

    /// Read the settings, falling back to the defaults.
    ///
    /// A missing file is an ordinary first run, and a file that cannot be read
    /// or parsed must not keep the app from starting, so both cases become the
    /// defaults. Nothing is reported: the only way to learn what a corrupt
    /// file said is to look at it, and there is no better advice than that.
    pub fn load() -> Self {
        match settings_path() {
            Some(path) => Self::load_from(&path),
            None => Self::default(),
        }
    }

    /// Read the settings from `path`.
    ///
    /// Split from [`Self::load`] so the tests do not have to write into the
    /// user's real configuration directory.
    pub fn load_from(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Self>(&text).ok())
            .unwrap_or_default()
            .sanitized()
    }

    /// Write the settings to the platform's configuration file.
    ///
    /// The directory is created if it is not there yet; on a first run it
    /// never is.
    pub fn save(&self) -> io::Result<()> {
        let path = settings_path().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "this platform has no per-user configuration directory",
            )
        })?;
        self.save_to(&path)
    }

    /// Write the settings to `path`.
    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut text = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        text.push('\n');
        fs::write(path, text)
    }
}

/// The folder the app keeps its files in.
///
/// The platform's per-user configuration directory, in a folder named for the
/// app. The qualifier and organisation match the identifier the app is
/// packaged under, so macOS gets `~/Library/Application Support/dev.aafnnp.md/`.
/// `None` when the platform cannot name a directory for the current user — on
/// Linux that needs a `$HOME` — in which case the app runs with the defaults
/// and saves nothing.
///
/// Shared rather than repeated per file, so the settings and the recent-files
/// list cannot end up in two different folders with the same name.
pub fn config_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("dev", "aafnnp", "md")
        .map(|dirs| dirs.config_dir().to_path_buf())
}

/// Where the settings file lives.
pub fn settings_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join(FILE_NAME))
}

/// Treat a blank family as unset.
///
/// An empty name resolves to no font at all, which is worse than the default
/// the user gets by leaving the setting out.
fn family(family: Option<String>) -> Option<String> {
    family
        .map(|family| family.trim().to_owned())
        .filter(|family| !family.is_empty())
}

/// Pull a value into `range`, or drop it.
///
/// JSON cannot spell a NaN or an infinity, but a value far outside the range is
/// perfectly representable and would poison every layout it reached, so
/// anything that is not finite is treated as unset.
fn size(value: Option<f32>, (min, max): (f32, f32)) -> Option<f32> {
    value
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(min, max))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory private to one test, so the tests can run in
    /// parallel without treading on each other's files.
    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("md-core-settings-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_defaults_follow_the_system_and_leave_the_rest_to_the_theme() {
        let settings = Settings::default();
        assert_eq!(settings.theme, ThemePreference::System);
        assert_eq!(settings.typesetting, Platform::Plain);
        assert_eq!(settings.font_family, None);
        assert_eq!(settings.font_size, None);
        assert_eq!(settings.mono_font_family, None);
        assert_eq!(settings.mono_font_size, None);
        assert_eq!(settings.editor_max_width, None);
        assert_eq!(settings.preview_padding, None);
    }

    #[test]
    fn the_theme_toggle_visits_every_preference_and_comes_back() {
        let mut theme = ThemePreference::System;
        let mut visited = vec![theme];
        for _ in 1..ThemePreference::ALL.len() {
            theme = theme.next();
            visited.push(theme);
        }
        assert_eq!(visited, ThemePreference::ALL);
        // One more step is the start again, which is what makes the toggle a
        // way back to following the system.
        assert_eq!(theme.next(), ThemePreference::System);
    }

    #[test]
    fn settings_round_trip_through_a_file() {
        let dir = scratch("round-trip");
        let path = dir.join(FILE_NAME);
        let settings = Settings {
            theme: ThemePreference::Dark,
            typesetting: Platform::WeChat,
            font_family: Some("Iosevka".to_string()),
            font_size: Some(17.0),
            mono_font_family: Some("JetBrains Mono".to_string()),
            mono_font_size: Some(15.5),
            editor_max_width: Some(720.0),
            preview_padding: Some(32.0),
        };

        settings.save_to(&path).unwrap();

        assert_eq!(Settings::load_from(&path), settings);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_or_unreadable_file_loads_as_the_defaults() {
        let dir = scratch("missing");
        assert_eq!(
            Settings::load_from(&dir.join("nothing-here.json")),
            Settings::default()
        );

        let corrupt = dir.join("corrupt.json");
        std::fs::write(&corrupt, "{ this is not json").unwrap();
        assert_eq!(Settings::load_from(&corrupt), Settings::default());

        // Valid JSON that is not an object at all.
        std::fs::write(&corrupt, "[1, 2, 3]").unwrap();
        assert_eq!(Settings::load_from(&corrupt), Settings::default());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_file_naming_one_setting_leaves_the_others_alone() {
        let dir = scratch("partial");
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, r#"{"theme": "dark"}"#).unwrap();

        let settings = Settings::load_from(&path);
        assert_eq!(settings.theme, ThemePreference::Dark);
        assert_eq!(settings.typesetting, Platform::Plain);
        assert_eq!(settings.font_size, None);
        assert_eq!(settings.editor_max_width, None);
        assert_eq!(settings.preview_padding, None);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_key_this_build_does_not_know_is_ignored() {
        let dir = scratch("unknown-key");
        let path = dir.join(FILE_NAME);
        // A setting from a newer version, spelled as one: it may not stop the
        // rest of the file from loading.
        std::fs::write(&path, r#"{"theme": "light", "future_thing": 3}"#).unwrap();

        assert_eq!(Settings::load_from(&path).theme, ThemePreference::Light);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The slugs are the on-disk spelling, so they are an interface: a file
    /// written by this build has to load in the next one.
    ///
    /// `WeChat` is the one that does not fall out of the enum's name — the
    /// snake-case conversion sees two words and writes `we_chat`. Pinning it
    /// here is what keeps a later tidy-up from quietly renaming it.
    #[test]
    fn every_platform_has_a_stable_slug_in_the_file() {
        let dir = scratch("slugs");
        let path = dir.join(FILE_NAME);

        for (platform, slug) in [
            (Platform::Plain, "plain"),
            (Platform::WeChat, "wechat"),
            (Platform::Toutiao, "toutiao"),
            (Platform::Xiaohongshu, "xiaohongshu"),
            (Platform::Zhihu, "zhihu"),
        ] {
            let settings = Settings {
                typesetting: platform,
                ..Default::default()
            };
            settings.save_to(&path).unwrap();

            let text = std::fs::read_to_string(&path).unwrap();
            assert!(text.contains(&format!("\"{slug}\"")), "{text}");
            assert_eq!(Settings::load_from(&path).typesetting, platform);
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn out_of_range_sizes_are_pulled_back_into_range() {
        let dir = scratch("range");
        let path = dir.join(FILE_NAME);
        std::fs::write(
            &path,
            r#"{"font_size": 400, "mono_font_size": 1, "editor_max_width": 40,
                "preview_padding": 900}"#,
        )
        .unwrap();

        let settings = Settings::load_from(&path);
        assert_eq!(settings.font_size, Some(FONT_SIZE_RANGE.1));
        assert_eq!(settings.mono_font_size, Some(FONT_SIZE_RANGE.0));
        assert_eq!(settings.editor_max_width, Some(EDITOR_WIDTH_RANGE.0));
        assert_eq!(settings.preview_padding, Some(PREVIEW_PADDING_RANGE.1));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Zero padding is a setting, not a mistake, so it has to survive the clamp
    /// that pulls everything else back into range.
    #[test]
    fn no_preview_padding_is_kept_rather_than_dropped() {
        let dir = scratch("zero-padding");
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, r#"{"preview_padding": 0}"#).unwrap();

        assert_eq!(Settings::load_from(&path).preview_padding, Some(0.0));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_blank_font_family_is_treated_as_unset() {
        let dir = scratch("blank-family");
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, r#"{"font_family": "   ", "mono_font_family": ""}"#).unwrap();

        let settings = Settings::load_from(&path);
        assert_eq!(settings.font_family, None);
        assert_eq!(settings.mono_font_family, None);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn saving_creates_the_directory_it_writes_into() {
        let dir = scratch("create-dir");
        // A path whose parents do not exist yet, as on a first run.
        let path = dir.join("nested").join("deeper").join(FILE_NAME);

        Settings::default().save_to(&path).unwrap();

        assert!(path.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn only_the_settings_that_were_set_are_written_out() {
        let dir = scratch("sparse");
        let path = dir.join(FILE_NAME);
        Settings {
            theme: ThemePreference::Dark,
            ..Default::default()
        }
        .save_to(&path)
        .unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("dark"));
        // A setting left at its default is a line the user does not have to
        // read past to find the ones they changed.
        assert!(!text.contains("font_family"));
        assert!(!text.contains("editor_max_width"));
        assert!(!text.contains("preview_padding"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_settings_file_sits_in_a_directory_named_for_the_app() {
        // The platform cannot always name a directory, so this checks the
        // shape rather than a path that differs from machine to machine.
        if let Some(path) = settings_path() {
            assert!(path.is_absolute());
            assert!(path.parent().is_some());
            assert_eq!(
                path.file_name().and_then(|name| name.to_str()),
                Some(FILE_NAME)
            );
        }
    }
}
