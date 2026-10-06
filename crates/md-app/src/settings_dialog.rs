//! The settings dialog: the settings file, given a face.
//!
//! Every control here edits one field of [`md_core::settings::Settings`], the
//! same file the app has always read. The dialog exists because a file we write
//! and expect to be hand-edited is a file most people never find — the settings
//! were reachable and invisible at the same time.
//!
//! Edits take effect as they are made rather than on the way out. Both panes
//! behind the dialog are live — the preview's margin and the source column's
//! width are read on the frame they are drawn — so a setting that waited for
//! "Done" would ask the user to close the dialog to see the thing the dialog is
//! for. It also means there is nothing to cancel, which is why the dialog has
//! one button.
//!
//! Text that cannot be used is refused rather than repaired. [`Settings`]
//! clamps on load, because a file is read once and never watched, so a value
//! far out of range has to be quietly pulled back or it poisons every layout it
//! reaches. Somebody typing is watching, though, and can be told the range
//! instead of being shown a field that reads 400 while the setting holds 48.

use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme, WindowExt as _, v_flex};
use gpui_kit::*;
use md_core::settings::{
    EDITOR_WIDTH_RANGE, FONT_SIZE_RANGE, PREVIEW_PADDING_RANGE, Settings, ThemePreference,
};

use crate::settings::{self, AppSettings};

/// Open the settings dialog.
///
/// A free function taking the window rather than a method on the workspace:
/// the dialog belongs to the window and sits over whatever is in it, and the
/// settings are a global, so there is nothing here that needs a workspace to
/// reach them.
pub fn open(window: &mut Window, cx: &mut App) {
    let form = SettingsForm::new(AppSettings::current(cx), window, cx);

    window.open_dialog(cx, move |dialog, _, _| {
        // Cloned rather than moved, and cloned inside the builder rather than
        // outside it: the builder is an `Fn` that runs once per frame, so it
        // can neither give away its only handle nor be handed a fresh one per
        // frame from beyond its own body.
        let body_form = form.clone();
        let done_form = form.clone();
        dialog
            .title("Settings")
            .w(px(520.))
            // No Cancel button: every field has already been applied, so there
            // is nothing left for it to mean.
            .button_props(
                DialogButtonProps::default()
                    .show_cancel(false)
                    .ok_text("Done"),
            )
            .content(move |content, _, _| content.child(body_form.clone()))
            .on_ok(move |_, _, cx| done_form.update(cx, |form, cx| form.done(cx)))
    });
}

/// The dialog's contents.
///
/// An entity rather than a handful of values captured by the builder: the
/// builder runs every frame, so what each field shows and what is wrong with it
/// have to be read fresh. Captured values would freeze the dialog at the state
/// it opened in.
struct SettingsForm {
    /// The settings as edited. Written through on every change.
    settings: Settings,
    family: Entity<InputState>,
    mono_family: Entity<InputState>,
    /// One entry per numeric setting, in the order they are shown.
    numbers: Vec<NumberField>,
    /// The last field left holding something unusable, and what is wrong with
    /// it.
    ///
    /// One field rather than a set: the complaint belongs under the field that
    /// was just left, and replacing it each time is what keeps them from
    /// accumulating down the dialog.
    refused: Option<(Field, String)>,
    /// Why the last write to disk failed, if it did. The setting is in force
    /// either way; only the next launch disagrees.
    save_error: Option<String>,
    /// Held so the field subscriptions outlive the call that made them —
    /// dropping a [`Subscription`] stops the callback.
    _subscriptions: Vec<Subscription>,
}

/// A numeric setting: the ones with a name, a range, and a field to type in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Field {
    FontSize,
    MonoFontSize,
    EditorWidth,
    PreviewPadding,
}

impl Field {
    /// Every numeric setting, in the order the dialog shows them.
    const ALL: [Self; 4] = [
        Self::FontSize,
        Self::MonoFontSize,
        Self::EditorWidth,
        Self::PreviewPadding,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::FontSize => "Interface font size",
            Self::MonoFontSize => "Source font size",
            Self::EditorWidth => "Source column width",
            Self::PreviewPadding => "Preview padding",
        }
    }

    /// What the file will accept for this setting.
    fn range(self) -> (f32, f32) {
        match self {
            // One range for both sizes: they are the same kind of number, and
            // a second constant would only invite them to drift apart.
            Self::FontSize | Self::MonoFontSize => FONT_SIZE_RANGE,
            Self::EditorWidth => EDITOR_WIDTH_RANGE,
            Self::PreviewPadding => PREVIEW_PADDING_RANGE,
        }
    }

    /// What the setting holds, or `None` when it is left to a default.
    fn read(self, settings: &Settings) -> Option<f32> {
        match self {
            Self::FontSize => settings.font_size,
            Self::MonoFontSize => settings.mono_font_size,
            Self::EditorWidth => settings.editor_max_width,
            Self::PreviewPadding => settings.preview_padding,
        }
    }

    fn write(self, settings: &mut Settings, value: Option<f32>) {
        match self {
            Self::FontSize => settings.font_size = value,
            Self::MonoFontSize => settings.mono_font_size = value,
            Self::EditorWidth => settings.editor_max_width = value,
            Self::PreviewPadding => settings.preview_padding = value,
        }
    }

    /// What the field shows in place of a value.
    fn placeholder(self) -> &'static str {
        match self {
            Self::FontSize | Self::MonoFontSize => "Theme default",
            Self::EditorWidth => "Fill the pane",
            Self::PreviewPadding => "App default",
        }
    }

    /// The line under the field: what blank means, and what will be accepted.
    ///
    /// Both are things the user cannot guess. An empty field could plausibly
    /// mean zero, and nothing on screen suggests 4000 is past the limit.
    fn hint(self) -> String {
        let (min, max) = self.range();
        format!("{} · between {min} and {max}", self.blank_meaning())
    }

    fn blank_meaning(self) -> &'static str {
        match self {
            Self::FontSize | Self::MonoFontSize => "Blank uses the theme's size",
            Self::EditorWidth => "Blank fills the pane",
            Self::PreviewPadding => "Blank uses the app's margin",
        }
    }
}

/// One numeric setting's field.
struct NumberField {
    field: Field,
    input: Entity<InputState>,
}

/// What a field's text amounts to.
enum Number {
    /// A value the app can use. `None` for a field left empty, which every one
    /// of these settings has a meaning for.
    Value(Option<f32>),
    /// Text that is not a value, and the sentence to put under the field.
    Refused(String),
}

/// Read a numeric setting out of what the user typed.
///
/// Deliberately tolerant of exactly one thing — an empty field — because
/// clearing it is how the user asks for the default back.
fn read_number(text: &str, field: Field) -> Number {
    let text = text.trim();
    if text.is_empty() {
        return Number::Value(None);
    }
    let (min, max) = field.range();
    match text.parse::<f32>() {
        Ok(value) if value.is_finite() && (min..=max).contains(&value) => {
            Number::Value(Some(value))
        }
        // The range rather than the value: the text stays on screen exactly as
        // it was typed, so the thing to say is what would have been accepted.
        Ok(_) => Number::Refused(format!("{} must be between {min} and {max}.", field.name())),
        Err(_) => Number::Refused(format!("{} must be a number.", field.name())),
    }
}

/// The text to open a field with.
fn starting_text(field: Field, settings: &Settings) -> String {
    match field.read(settings) {
        Some(value) => value.to_string(),
        None => String::new(),
    }
}

/// Which of the two family fields an edit came from.
#[derive(Clone, Copy)]
enum Family {
    Interface,
    Mono,
}

impl SettingsForm {
    fn new(settings: Settings, window: &mut Window, cx: &mut App) -> Entity<Self> {
        let family = text_field(
            settings.font_family.as_deref().unwrap_or_default(),
            "System interface font",
            window,
            cx,
        );
        let mono_family = text_field(
            settings.mono_font_family.as_deref().unwrap_or_default(),
            "System monospaced font",
            window,
            cx,
        );
        let numbers = Field::ALL
            .into_iter()
            .map(|field| NumberField {
                field,
                input: text_field(
                    &starting_text(field, &settings),
                    field.placeholder(),
                    window,
                    cx,
                ),
            })
            .collect::<Vec<_>>();

        cx.new(|cx| {
            let mut subscriptions = Vec::new();
            subscriptions.push(cx.subscribe(
                &family,
                |this: &mut Self, _, event: &InputEvent, cx| {
                    this.family_edited(Family::Interface, event, cx)
                },
            ));
            subscriptions.push(cx.subscribe(
                &mono_family,
                |this: &mut Self, _, event: &InputEvent, cx| {
                    this.family_edited(Family::Mono, event, cx)
                },
            ));
            for entry in &numbers {
                let field = entry.field;
                subscriptions.push(cx.subscribe(
                    &entry.input,
                    move |this: &mut Self, _, event: &InputEvent, cx| {
                        this.number_edited(field, event, cx)
                    },
                ));
            }

            Self {
                settings,
                family,
                mono_family,
                numbers,
                refused: None,
                save_error: None,
                _subscriptions: subscriptions,
            }
        })
    }

    /// The "Done" button.
    ///
    /// Returns whether the dialog may close. A field holding nonsense is the one
    /// thing that keeps it open: the setting still holds whatever it held before
    /// that text was typed, and closing would leave the field and the setting
    /// disagreeing with nothing on screen to say so.
    fn done(&mut self, cx: &mut Context<Self>) -> bool {
        let refusal =
            self.number_texts(cx).into_iter().find_map(|(field, text)| {
                match read_number(&text, field) {
                    Number::Refused(message) => Some((field, message)),
                    Number::Value(_) => None,
                }
            });

        self.refused = refusal;
        match &self.refused {
            Some(_) => {
                cx.notify();
                false
            }
            None => true,
        }
    }

    fn set_theme(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(preference) = ThemePreference::ALL.get(index).copied() else {
            return;
        };
        if self.settings.theme == preference {
            return;
        }
        self.settings.theme = preference;

        AppSettings::set(self.settings.clone(), cx);
        // The mode itself changed, so the registered theme has to be reloaded
        // and not merely have its fonts edited.
        settings::apply(&self.settings, Some(window), cx);
        self.finish(cx);
    }

    fn family_edited(&mut self, which: Family, event: &InputEvent, cx: &mut Context<Self>) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        let entity = match which {
            Family::Interface => &self.family,
            Family::Mono => &self.mono_family,
        };
        let text = entity.read(cx).value().to_string();
        // Blank means the same as a missing key in the file: let the theme pick
        // the platform's own font. Trimming because a family that differs from
        // the theme's only by trailing spaces is a typo, not a choice.
        let value = Some(text.trim().to_owned()).filter(|family| !family.is_empty());

        let slot = match which {
            Family::Interface => &mut self.settings.font_family,
            Family::Mono => &mut self.settings.mono_font_family,
        };
        if *slot == value {
            return;
        }
        *slot = value;
        self.commit_fonts(cx);
    }

    fn number_edited(&mut self, field: Field, event: &InputEvent, cx: &mut Context<Self>) {
        match event {
            InputEvent::Change => {
                let text = self.text_of(field, cx);
                // Text that cannot be used yet is passed over in silence: "1" on
                // the way to "18" is not a mistake, and a complaint raised at
                // every keystroke would be wrong more often than it was right.
                // The value the setting already holds stays in force until the
                // text means something.
                if let Number::Value(value) = read_number(&text, field)
                    && field.read(&self.settings) != value
                {
                    field.write(&mut self.settings, value);
                    self.commit_fonts(cx);
                }
            }
            // Leaving the field is the moment to say so, one way or the other.
            InputEvent::Blur | InputEvent::PressEnter { .. } => self.check(field, cx),
            InputEvent::Focus => {}
        }
    }

    /// Say what is wrong with a field, or take back an earlier complaint.
    fn check(&mut self, field: Field, cx: &mut Context<Self>) {
        let text = self.text_of(field, cx);
        let complaint = match read_number(&text, field) {
            Number::Refused(message) => Some(message),
            Number::Value(_) => None,
        };

        let next = complaint.map(|message| (field, message));
        // Leaving a field that is still fine — or one whose complaint has not
        // changed — must not cost a frame.
        if self.refused == next {
            return;
        }
        self.refused = next;
        cx.notify();
    }

    /// Apply an edit to anything the theme's mode is not chosen by.
    fn commit_fonts(&mut self, cx: &mut Context<Self>) {
        AppSettings::set(self.settings.clone(), cx);
        // Only the fonts: going through `apply` would reload the whole
        // registered theme on every keystroke, to arrive at the same mode it
        // already had.
        settings::apply_fonts(&self.settings, cx);
        self.finish(cx);
    }

    /// Write the settings down, and record whether the write worked.
    ///
    /// A failed write is reported rather than undone. The setting is in force
    /// for this run; pretending the click did nothing would be a worse lie than
    /// saying the file could not be written.
    fn finish(&mut self, cx: &mut Context<Self>) {
        self.save_error = self.settings.save().err().map(|error| error.to_string());
        cx.refresh_windows();
        cx.notify();
    }

    /// What one field currently holds, as text.
    fn text_of(&self, field: Field, cx: &App) -> String {
        self.numbers
            .iter()
            .find(|entry| entry.field == field)
            .map(|entry| entry.input.read(cx).value().to_string())
            .unwrap_or_default()
    }

    /// What every field currently holds, paired with the setting it edits.
    fn number_texts(&self, cx: &App) -> Vec<(Field, String)> {
        self.numbers
            .iter()
            .map(|entry| (entry.field, entry.input.read(cx).value().to_string()))
            .collect()
    }

    fn theme_control(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let chosen = self.settings.theme;
        RadioGroup::horizontal("settings-theme")
            .selected_index(ThemePreference::ALL.iter().position(|one| *one == chosen))
            .children(
                ThemePreference::ALL
                    .into_iter()
                    .enumerate()
                    .map(|(index, preference)| {
                        Radio::new(("settings-theme", index))
                            .label(preference.label())
                            .checked(preference == chosen)
                    }),
            )
            // A controlled value: the callback is a request, and the radio only
            // moves once the value behind it has been written and the frame
            // asked for again — which `set_theme` does.
            .on_change(
                cx.listener(|this, index: &usize, window, cx| this.set_theme(*index, window, cx)),
            )
    }
}

impl Render for SettingsForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = v_flex().gap_4();

        body = body.child(setting("Theme", self.theme_control(cx)));
        body = body.child(setting(
            "Interface font",
            text_control(&self.family, "Blank uses the platform's own font", None, cx),
        ));
        body = body.child(setting(
            "Source font",
            text_control(
                &self.mono_family,
                "Blank uses the platform's monospaced font",
                None,
                cx,
            ),
        ));

        for entry in &self.numbers {
            let field = entry.field;
            // The complaint replaces the hint rather than joining it: the range
            // is in the sentence that says the value is outside it.
            let complaint = self
                .refused
                .as_ref()
                .filter(|(which, _)| *which == field)
                .map(|(_, message)| message.as_str());
            body = body.child(setting(
                field.name(),
                text_control(&entry.input, &field.hint(), complaint, cx),
            ));
        }

        if let Some(error) = &self.save_error {
            body = body.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(format!("The settings could not be saved: {error}")),
            );
        }

        // The dialog is a fixed width and the settings are more than fit a short
        // window, so the body scrolls rather than clipping the last field.
        div().max_h(px(460.)).overflow_y_scrollbar().child(body)
    }
}

/// One setting: its name above whatever edits it.
///
/// Above rather than beside, because two of the controls are full-width fields
/// and one is a row of radios; a label column wide enough for the longest name
/// would leave the fields looking half-drawn.
fn setting(name: &str, control: impl IntoElement) -> impl IntoElement {
    v_flex()
        .gap_1()
        .child(div().text_sm().child(name.to_string()))
        .child(control)
}

/// A field, the line under it, and — when there is one — a complaint in place
/// of that line.
fn text_control(
    input: &Entity<InputState>,
    hint: &str,
    complaint: Option<&str>,
    cx: &App,
) -> impl IntoElement {
    let note = match complaint {
        Some(message) => div()
            .text_sm()
            .text_color(cx.theme().danger)
            .child(message.to_string()),
        None => div()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(hint.to_string()),
    };

    v_flex()
        .gap_1()
        // Cleanable so a field can be emptied back to its default without the
        // user having to select the text and delete it.
        .child(Input::new(input).cleanable(true))
        .child(note)
}

/// A one-line text field.
fn text_field(
    value: &str,
    placeholder: &str,
    window: &mut Window,
    cx: &mut App,
) -> Entity<InputState> {
    let value = value.to_string();
    let placeholder = placeholder.to_string();
    cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(placeholder)
            .default_value(value)
    })
}

#[cfg(test)]
mod tests {
    // Imported narrowly: `use super::*` would drag in the `gpui_kit::*` glob,
    // whose `test` attribute macro shadows the built-in `#[test]`.
    use super::{Field, Number, read_number};

    fn value(number: Number) -> Option<Option<f32>> {
        match number {
            Number::Value(value) => Some(value),
            Number::Refused(_) => None,
        }
    }

    #[test]
    fn a_blank_field_asks_for_the_default() {
        assert_eq!(value(read_number("", Field::FontSize)), Some(None));
        assert_eq!(value(read_number("   ", Field::PreviewPadding)), Some(None));
    }

    #[test]
    fn a_value_inside_the_range_is_taken_as_written() {
        assert_eq!(value(read_number("17", Field::FontSize)), Some(Some(17.0)));
        assert_eq!(
            value(read_number(" 17.5 ", Field::FontSize)),
            Some(Some(17.5))
        );
        assert_eq!(
            value(read_number("0", Field::PreviewPadding)),
            Some(Some(0.0))
        );
    }

    #[test]
    fn the_ends_of_the_range_are_inside_it() {
        assert_eq!(value(read_number("8", Field::FontSize)), Some(Some(8.0)));
        assert_eq!(value(read_number("48", Field::FontSize)), Some(Some(48.0)));
        assert_eq!(
            value(read_number("4000", Field::EditorWidth)),
            Some(Some(4000.0))
        );
    }

    #[test]
    fn a_value_outside_the_range_is_refused_with_the_range_in_the_message() {
        let Number::Refused(message) = read_number("400", Field::FontSize) else {
            panic!("400 is not a usable font size");
        };
        assert!(message.contains('8'), "{message}");
        assert!(message.contains("48"), "{message}");
    }

    #[test]
    fn text_that_is_not_a_number_is_refused() {
        assert!(matches!(
            read_number("eighteen", Field::FontSize),
            Number::Refused(_)
        ));
        // Parsed as a float, but not one anything can be laid out with.
        assert!(matches!(
            read_number("inf", Field::FontSize),
            Number::Refused(_)
        ));
    }

    #[test]
    fn every_field_is_named_and_has_a_range_it_belongs_to() {
        for field in Field::ALL {
            assert!(!field.name().is_empty());
            let (min, max) = field.range();
            assert!(min < max);
            // What the hint promises has to be what `read_number` enforces.
            assert_eq!(value(read_number(&min.to_string(), field)), Some(Some(min)));
            assert_eq!(value(read_number(&max.to_string(), field)), Some(Some(max)));
        }
    }
}
