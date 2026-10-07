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
//! Everything that can be offered as a list is offered as a list. A font is a
//! name the user would otherwise have to already know, and a wrong one is not
//! refused by anything — it goes to the text system and comes back as whatever
//! the platform falls back to, so the only feedback is that nothing changed.
//! The two families and the two sizes are therefore chosen from what the system
//! and the settings' own range can actually produce.
//!
//! What is still typed is refused rather than repaired. [`Settings`] clamps on
//! load, because a file is read once and never watched, so a value far out of
//! range has to be quietly pulled back or it poisons every layout it reaches.
//! Somebody typing is watching, though, and can be told the range instead of
//! being shown a field that reads 400 while the setting holds 48.

use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_kit::component::{ActiveTheme, IndexPath, WindowExt as _, v_flex};
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
            // `.child`, not `.content`. The content area sits beside the
            // dialog's own scrolling body, not inside it, and it has no
            // overflow of its own — so it refuses to shrink, and the fields
            // past the bottom of a short window are clipped with nothing to
            // scroll them into view. `.child` hands the form to the body that
            // does scroll.
            .child(body_form.clone())
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
    /// One entry per list-backed setting, in the order they are shown.
    choices: Vec<ChoiceField>,
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

/// A setting chosen from a list rather than typed.
///
/// The counterpart to [`Field`]: these are the settings whose values can be
/// enumerated, so the dialog can offer them instead of asking the user to spell
/// them. A wrong font name is the reason the enum exists at all — nothing
/// downstream rejects one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Choice {
    FontFamily,
    MonoFontFamily,
    FontSize,
    MonoFontSize,
}

impl Choice {
    /// Every list-backed setting, in the order the dialog shows them.
    const ALL: [Self; 4] = [
        Self::FontFamily,
        Self::MonoFontFamily,
        Self::FontSize,
        Self::MonoFontSize,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::FontFamily => "Interface font",
            Self::MonoFontFamily => "Source font",
            Self::FontSize => "Interface font size",
            Self::MonoFontSize => "Source font size",
        }
    }

    /// What the setting holds, as the text the dropdown lists it under.
    ///
    /// `None` is the theme's own choice, the same thing an absent key in the
    /// file means — which is why the clear button can be offered as the way
    /// back to it.
    fn read(self, settings: &Settings) -> Option<String> {
        match self {
            Self::FontFamily => settings.font_family.clone(),
            Self::MonoFontFamily => settings.mono_font_family.clone(),
            Self::FontSize => settings.font_size.map(|size| size.to_string()),
            Self::MonoFontSize => settings.mono_font_size.map(|size| size.to_string()),
        }
    }

    /// Take a value back from the dropdown.
    ///
    /// A size that will not parse is treated as `None` rather than refused.
    /// Every entry the dropdown offers came from [`options`], so text that is
    /// not a number can only be a bug here, and a size the app cannot lay out
    /// with is better dropped than stored.
    fn write(self, settings: &mut Settings, value: Option<&str>) {
        let value = value.map(str::trim).filter(|text| !text.is_empty());
        match self {
            Self::FontFamily => settings.font_family = value.map(str::to_owned),
            Self::MonoFontFamily => settings.mono_font_family = value.map(str::to_owned),
            Self::FontSize => settings.font_size = value.and_then(|text| text.parse().ok()),
            Self::MonoFontSize => {
                settings.mono_font_size = value.and_then(|text| text.parse().ok())
            }
        }
    }

    /// The line under the dropdown.
    ///
    /// Neither half is guessable: an empty dropdown could plausibly mean the
    /// first entry, and nothing on screen says the source font list is not
    /// restricted to monospaced families.
    fn hint(self) -> &'static str {
        match self {
            Self::FontFamily => "Every family the system has · blank uses the platform's own",
            Self::MonoFontFamily => {
                "Every family the system has, not only the monospaced ones · blank uses the platform's monospaced font"
            }
            Self::FontSize | Self::MonoFontSize => "Whole points · blank uses the theme's size",
        }
    }
}

/// The values a dropdown offers.
///
/// The setting's current value is in the list even when it is not one of the
/// ones offered. A file can hold a size of `13.5`, or the name of a font that
/// has since been uninstalled, and both are values the app is honouring — a
/// dropdown that could not show one would sit blank above a setting that still
/// holds it, and would replace it the first time anything else was chosen.
fn options(choice: Choice, fonts: &[String], current: Option<&str>) -> Vec<String> {
    let mut items: Vec<String> = match choice {
        // The text system's own list, already sorted and deduplicated. Which
        // of these are monospaced is not something it reports, and asking
        // would mean laying out text in several hundred families to compare
        // their widths.
        Choice::FontFamily | Choice::MonoFontFamily => fonts.to_vec(),
        Choice::FontSize | Choice::MonoFontSize => size_options()
            .into_iter()
            .map(|size| size.to_string())
            .collect(),
    };

    let Some(current) = current.filter(|value| !items.iter().any(|item| item == value)) else {
        return items;
    };

    // In its place rather than at the end: the list is something to read down,
    // and a value out of order reads as a mistake.
    let at = match choice {
        Choice::FontFamily | Choice::MonoFontFamily => items
            .binary_search_by(|item| item.as_str().cmp(current))
            .unwrap_or_else(|at| at),
        // By number, not by text: 13.5 belongs between 13 and 14, and "13.5"
        // sorts before "13" as a string.
        Choice::FontSize | Choice::MonoFontSize => match current.parse::<f32>() {
            Ok(value) => size_options().partition_point(|size| *size < value),
            Err(_) => items.len(),
        },
    };
    items.insert(at, current.to_owned());
    items
}

/// Every whole point the file will keep.
///
/// The rendered list and the position a hand-edited value is inserted at have
/// to agree, so both come from here rather than one of them being written out
/// again.
fn size_options() -> Vec<f32> {
    let (min, max) = FONT_SIZE_RANGE;
    (min.ceil() as i32..=max.floor() as i32)
        .map(|size| size as f32)
        .collect()
}

/// One list-backed setting's dropdown.
struct ChoiceField {
    choice: Choice,
    select: Entity<SelectState<SearchableVec<String>>>,
}

impl ChoiceField {
    /// The dropdown and the line under it.
    ///
    /// No complaint can appear here, unlike under a typed field: everything the
    /// list offers is a value the app can use, and the control cannot be made
    /// to hold anything else.
    fn control(&self, cx: &App) -> impl IntoElement {
        v_flex()
            .gap_1()
            .child(
                Select::new(&self.select)
                    // The clear button is the way back to the theme's own font or
                    // size. It is drawn only once something is chosen, so a
                    // dropdown nobody has touched shows no ✕ that could be misread
                    // as meaning nothing was chosen.
                    .cleanable(true)
                    // The label sits above the control rather than beside it, so
                    // without this the select would announce itself as an unnamed
                    // button holding a font name.
                    .accessibility_label(self.choice.name())
                    .search_placeholder("Search")
                    // Tall enough to scroll through a run of families, short
                    // enough to leave the dialog looking like a dialog — and the
                    // search box is what makes the difference, since a menu that
                    // can be filtered does not need to be long.
                    .menu_max_h(px(320.)),
            )
            .child(note(self.choice.hint(), None, cx))
    }
}

/// A numeric setting: the ones with a name, a range, and a field to type in.
///
/// Only the two free-form settings are left. A column width and a margin are
/// numbers with no list worth offering — any value in the range is a real
/// choice — whereas the font sizes were whole points drawn from a fixed range,
/// and are [`Choice`]s now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Field {
    EditorWidth,
    PreviewPadding,
}

impl Field {
    /// Every numeric setting, in the order the dialog shows them.
    const ALL: [Self; 2] = [Self::EditorWidth, Self::PreviewPadding];

    fn name(self) -> &'static str {
        match self {
            Self::EditorWidth => "Source column width",
            Self::PreviewPadding => "Preview padding",
        }
    }

    /// What the file will accept for this setting.
    fn range(self) -> (f32, f32) {
        match self {
            Self::EditorWidth => EDITOR_WIDTH_RANGE,
            Self::PreviewPadding => PREVIEW_PADDING_RANGE,
        }
    }

    /// What the setting holds, or `None` when it is left to a default.
    fn read(self, settings: &Settings) -> Option<f32> {
        match self {
            Self::EditorWidth => settings.editor_max_width,
            Self::PreviewPadding => settings.preview_padding,
        }
    }

    fn write(self, settings: &mut Settings, value: Option<f32>) {
        match self {
            Self::EditorWidth => settings.editor_max_width = value,
            Self::PreviewPadding => settings.preview_padding = value,
        }
    }

    /// What the field shows in place of a value.
    fn placeholder(self) -> &'static str {
        match self {
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

impl SettingsForm {
    fn new(settings: Settings, window: &mut Window, cx: &mut App) -> Entity<Self> {
        // Asked for once, here, rather than in `render`: the builder runs every
        // frame, and putting several hundred names to the text system on each
        // of them to throw them away again is not a thing to do. The dialog is
        // built afresh each time it is opened, so the list is as current as the
        // moment it appeared.
        let fonts = cx.text_system().all_font_names();

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
            let mut choices = Vec::new();

            for choice in Choice::ALL {
                let current = choice.read(&settings);
                let items = options(choice, &fonts, current.as_deref());
                // Opens on what the setting holds, which is either what the
                // file said or what was last chosen here.
                let selected = current
                    .and_then(|value| items.iter().position(|item| *item == value))
                    .map(IndexPath::new);
                let select = cx.new(|cx| {
                    SelectState::new(SearchableVec::new(items), selected, window, cx)
                        // Off by default, and the font list is unusable
                        // without it: hundreds of families and no way to
                        // narrow them but the scroll wheel.
                        .searchable(true)
                });
                subscriptions.push(cx.subscribe(
                    &select,
                    move |this: &mut Self, _, event: &SelectEvent<SearchableVec<String>>, cx| {
                        this.chose(choice, event, cx)
                    },
                ));
                choices.push(ChoiceField { choice, select });
            }

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
                choices,
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

    /// A dropdown was used.
    ///
    /// Two things arrive here and mean different things: a value, and the clear
    /// button, which confirms `None` and asks for the theme's own choice back —
    /// the same thing an absent key in the file means. Closing the menu without
    /// choosing anything is `DismissEvent`, which is deliberately not
    /// subscribed to, so walking away from an open menu is not read as having
    /// cleared it.
    fn chose(
        &mut self,
        choice: Choice,
        event: &SelectEvent<SearchableVec<String>>,
        cx: &mut Context<Self>,
    ) {
        let SelectEvent::Confirm(value) = event;
        let before = choice.read(&self.settings);
        choice.write(&mut self.settings, value.as_deref());
        // Picking what is already picked confirms it again. Nothing has moved,
        // and reloading the theme and rewriting the file to arrive back where
        // we started is work nobody asked for.
        if choice.read(&self.settings) == before {
            return;
        }
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

        for entry in &self.choices {
            body = body.child(setting(entry.choice.name(), entry.control(cx)));
        }

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

        // The form goes to the dialog's own scrolling body, which takes over
        // once the dialog has grown to the window and can grow no further — so
        // no scrolling is set up here. A height or an overflow of our own could
        // only ever be a second, competing scroll region.
        body
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
    v_flex()
        .gap_1()
        // Cleanable so a field can be emptied back to its default without the
        // user having to select the text and delete it.
        .child(Input::new(input).cleanable(true))
        .child(note(hint, complaint, cx))
}

/// The line under a control: what blank means, or the complaint that has taken
/// its place.
///
/// The complaint replaces the hint rather than joining it, because the range is
/// already in the sentence that says the value is outside it.
fn note(hint: &str, complaint: Option<&str>, cx: &App) -> impl IntoElement {
    match complaint {
        Some(message) => div()
            .text_sm()
            .text_color(cx.theme().danger)
            .child(message.to_string()),
        None => div()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(hint.to_string()),
    }
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
    use super::{Choice, Field, Number, options, read_number};
    use md_core::settings::{FONT_SIZE_RANGE, Settings};

    fn value(number: Number) -> Option<Option<f32>> {
        match number {
            Number::Value(value) => Some(value),
            Number::Refused(_) => None,
        }
    }

    /// A font list shaped like the text system's: sorted, and standing in for
    /// the several hundred names a real one holds.
    fn fonts() -> Vec<String> {
        ["Arial", "Menlo", "Zed Mono"].map(str::to_owned).to_vec()
    }

    #[test]
    fn a_blank_field_asks_for_the_default() {
        assert_eq!(value(read_number("", Field::EditorWidth)), Some(None));
        assert_eq!(value(read_number("   ", Field::PreviewPadding)), Some(None));
    }

    #[test]
    fn a_value_inside_the_range_is_taken_as_written() {
        assert_eq!(
            value(read_number("600", Field::EditorWidth)),
            Some(Some(600.0))
        );
        assert_eq!(
            value(read_number(" 600.5 ", Field::EditorWidth)),
            Some(Some(600.5))
        );
        assert_eq!(
            value(read_number("0", Field::PreviewPadding)),
            Some(Some(0.0))
        );
    }

    #[test]
    fn the_ends_of_the_range_are_inside_it() {
        assert_eq!(
            value(read_number("320", Field::EditorWidth)),
            Some(Some(320.0))
        );
        assert_eq!(
            value(read_number("4000", Field::EditorWidth)),
            Some(Some(4000.0))
        );
        assert_eq!(
            value(read_number("200", Field::PreviewPadding)),
            Some(Some(200.0))
        );
    }

    #[test]
    fn a_value_outside_the_range_is_refused_with_the_range_in_the_message() {
        let Number::Refused(message) = read_number("80", Field::EditorWidth) else {
            panic!("80 is a column too narrow for the window to hold");
        };
        // The numbers rather than a single digit: `contains('0')` would be
        // answered by almost any sentence at all.
        assert!(message.contains("320"), "{message}");
        assert!(message.contains("4000"), "{message}");
    }

    #[test]
    fn text_that_is_not_a_number_is_refused() {
        assert!(matches!(
            read_number("wide", Field::EditorWidth),
            Number::Refused(_)
        ));
        // Parsed as a float, but not one anything can be laid out with.
        assert!(matches!(
            read_number("inf", Field::EditorWidth),
            Number::Refused(_)
        ));
    }

    #[test]
    fn the_size_list_is_every_whole_point_the_file_keeps() {
        let sizes = options(Choice::FontSize, &[], None);
        let (min, max) = FONT_SIZE_RANGE;
        assert_eq!(sizes.len(), (max - min) as usize + 1);
        assert_eq!(sizes.first().map(String::as_str), Some("8"));
        assert_eq!(sizes.last().map(String::as_str), Some("48"));
        // Nothing offered is something `Settings` would pull back on load.
        for size in &sizes {
            let size: f32 = size.parse().expect("a size is a number");
            assert!((min..=max).contains(&size), "{size} is outside the range");
        }
    }

    #[test]
    fn a_size_the_list_does_not_hold_is_shown_where_it_belongs() {
        // What a hand-edited file can hold: in range, so `Settings` keeps it,
        // and not one of the whole points the list offers.
        let sizes = options(Choice::FontSize, &[], Some("13.5"));
        let at = sizes
            .iter()
            .position(|size| size == "13.5")
            .expect("the value the file holds is in the list");
        assert_eq!(sizes[at - 1], "13");
        assert_eq!(sizes[at + 1], "14");
        // Inserted, not appended, and not duplicated.
        assert_eq!(sizes.len(), 42);
    }

    #[test]
    fn a_size_survives_the_round_trip_through_the_dropdown() {
        let mut settings = Settings::default();
        Choice::FontSize.write(&mut settings, Some("13.5"));
        assert_eq!(settings.font_size, Some(13.5));
        assert_eq!(Choice::FontSize.read(&settings).as_deref(), Some("13.5"));

        // A whole point comes back as one, without a trailing ".0" that would
        // match no entry in the list.
        Choice::FontSize.write(&mut settings, Some("18"));
        assert_eq!(Choice::FontSize.read(&settings).as_deref(), Some("18"));
    }

    #[test]
    fn clearing_a_dropdown_asks_for_the_theme_default() {
        let mut settings = Settings::default();
        Choice::FontFamily.write(&mut settings, Some("Inter"));
        assert_eq!(settings.font_family.as_deref(), Some("Inter"));

        // What the clear button confirms.
        Choice::FontFamily.write(&mut settings, None);
        assert_eq!(settings.font_family, None);
        assert_eq!(Choice::FontFamily.read(&settings), None);

        // Whitespace is a cleared field someone typed a space into.
        Choice::MonoFontFamily.write(&mut settings, Some("   "));
        assert_eq!(settings.mono_font_family, None);
    }

    #[test]
    fn a_font_the_system_no_longer_has_is_still_offered() {
        let listed = options(Choice::MonoFontFamily, &fonts(), Some("Glyph Sans"));
        assert_eq!(listed, ["Arial", "Glyph Sans", "Menlo", "Zed Mono"]);

        // A family that is installed is not offered twice, and the list comes
        // back exactly as the text system gave it.
        let listed = options(Choice::MonoFontFamily, &fonts(), Some("Menlo"));
        assert_eq!(listed, fonts());
        assert_eq!(options(Choice::MonoFontFamily, &fonts(), None), fonts());
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
