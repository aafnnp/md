//! The root view: a tab strip over `sidebar | active tab`.

use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::base::{h_resizable, resizable_panel};
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::tab::{Tab as TabButton, TabBar};
use gpui_kit::component::{ActiveTheme, WindowExt as _, v_flex};
use gpui_kit::*;

use md_core::fs;
use md_core::recent::{self, RecentFiles};
use md_core::{Document, export};

use crate::actions::{CloseTab, ExportHtml, OpenFile, Quit, Save, SaveAs, SelectTab, ToggleTheme};
use crate::settings::{self, AppSettings};
use crate::sidebar::{Sidebar, SidebarEvent, SidebarRequest};
use crate::tab::Tab;

const STARTER_DOCUMENT: &str = "\
# md

A Markdown editor. Type on the left, read on the right.

## What works so far

- **Bold**, _italic_, `code`, and [links](https://gpui.rs)
- Lists, quotes, and rules

> The preview re-renders shortly after you stop typing.

```rust
fn main() {
    println!(\"hello\");
}
```
";

pub struct Workspace {
    tabs: Vec<Entity<Tab>>,
    /// Index into `tabs`. Meaningless while `tabs` is empty.
    active: usize,
    sidebar: Entity<Sidebar>,
    /// Source of `Tab` ids. Never reused, so a closed tab's splitter layout
    /// cannot leak into the tab that takes its place.
    next_tab_id: u64,
    /// The file the list of recently opened documents is kept in, or `None`
    /// when this platform gives the app no configuration directory to put one
    /// in — in which case the list is simply not kept.
    ///
    /// A field rather than a call to [`recent::recent_path`] at each use so a
    /// test can point it at a scratch file. The tests open real files, and
    /// opening one records it: without this every run would rewrite the
    /// developer's own list with paths out of `/tmp`.
    recents: Option<PathBuf>,
    /// Dropping a `Subscription` cancels it, so this has to be held: a
    /// subscription created and discarded would leave the tree's rows opening
    /// nothing at all.
    _sidebar_events: Subscription,
    /// Held for the same reason: while the theme follows the system, this is
    /// what notices the system changing.
    _appearance: Subscription,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sidebar = cx.new(Sidebar::new);
        // `subscribe_in` rather than `subscribe`: opening a file from the tree
        // has to focus the new tab's editor, and only the `_in` form hands the
        // callback a window to focus into.
        let _sidebar_events = cx.subscribe_in(
            &sidebar,
            window,
            |this, _, event: &SidebarEvent, window, cx| match event {
                SidebarEvent::Open(path) => this.open_path(path.clone(), window, cx),
                SidebarEvent::Request(request) => this.handle_request(request.clone(), window, cx),
            },
        );

        let _appearance = settings::follow_system_appearance(window);

        let mut workspace = Self {
            tabs: Vec::new(),
            active: 0,
            sidebar,
            next_tab_id: 0,
            recents: recent::recent_path(),
            _sidebar_events,
            _appearance,
        };

        // Before anything is opened this session, offer what was opened last
        // session. A read failure is an empty list, which is what a first run
        // looks like too, so there is nothing to report either way.
        workspace.publish_recents(cx);

        let mut starter = Document::new();
        starter.set_text(STARTER_DOCUMENT);
        workspace.open(starter, window, cx);
        workspace
    }

    /// Open a document in a new tab and focus it.
    pub fn open(&mut self, document: Document, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;

        let tab = cx.new(|cx| Tab::new(id, document, window, cx));
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        cx.notify();
    }

    /// Open the file at `path`, focusing it if a tab already shows it.
    ///
    /// The sidebar's tree is re-read from disk, so it can offer a row for a
    /// file that has since been removed or made unreadable; that failure is
    /// reported rather than silently swallowed, which would look like the click
    /// had been ignored.
    pub fn open_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let already_open = self
            .tabs
            .iter()
            .position(|tab| tab.read(cx).path() == Some(path.as_path()));
        if let Some(index) = already_open {
            self.select(index, cx);
            // Still recorded. This is the document being worked on now, and the
            // recent list is about what was opened, not what was new.
            self.remember(&path, window, cx);
            return;
        }

        match Document::open(&path) {
            Ok(document) => {
                self.open(document, window, cx);
                self.remember(&path, window, cx);
            }
            // Recorded from an earlier session, and gone since: a volume
            // unmounted, a file deleted outside the app. The failure is
            // reported — a click that did nothing looks broken — and the entry
            // is dropped, because leaving it there would offer it again every
            // time the sidebar is looked at.
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.forget(&path, window, cx);
                report_open_failure(&path, &error, window, cx);
            }
            Err(error) => report_open_failure(&path, &error, window, cx),
        }
    }

    /// Ask for a file and open it.
    ///
    /// Reaches the files that the sidebar's tree cannot, which is every file
    /// that is not under the folder it happens to be rooted at — and every file
    /// at all before a folder has been opened.
    fn open_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open file".into()),
        });
        // `Context::spawn` hands out an `AsyncApp`, which is not tied to a
        // window; opening a file needs one to focus the new tab's editor.
        let window = window.window_handle();

        cx.spawn(async move |this, cx| {
            // Cancelling needs no explanation, and a platform without a file
            // panel cannot be fixed by a message from here.
            let Ok(Ok(Some(mut paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.pop() else {
                return;
            };

            cx.update_window(window, move |_, window, cx| {
                this.update(cx, |workspace, cx| workspace.open_path(path, window, cx))
            })
            .ok();
        })
        .detach();
    }

    /// The recently opened files, newest first, as they stand on disk.
    ///
    /// A file that cannot be read or parsed is an empty list: the same bargain
    /// the settings file gets, and here even the failure is only a convenience.
    fn recents(&self) -> RecentFiles {
        match &self.recents {
            Some(path) => RecentFiles::load_from(path),
            None => RecentFiles::default(),
        }
    }

    /// Write the recent list back and show it.
    ///
    /// Every change goes through here, so the sidebar and the file cannot end
    /// up disagreeing about what was opened last.
    ///
    /// `window` is where a write failure would be reported, and there is one
    /// everywhere except a rename — a name dialog's callbacks are handed an
    /// `App` and no window. There the failure is passed over rather than
    /// swallowed: the list in memory is still correct, and the next file that
    /// is opened writes the whole of it again.
    fn store_recents(
        &mut self,
        files: RecentFiles,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        if let Some(path) = &self.recents
            && let Err(error) = files.save_to(path)
        {
            // Recording what was opened is not what opening it depends on, so
            // this costs the user nothing yet — but a recent list that silently
            // stops growing is otherwise unexplainable.
            if let Some(window) = window {
                window.push_notification(
                    Notification::warning(format!(
                        "Could not record recent files in “{}”: {error}",
                        file_label(path)
                    )),
                    cx,
                );
            }
        }
        self.sidebar.update(cx, |sidebar, cx| {
            sidebar.set_recents(files.entries().to_vec(), cx)
        });
    }

    /// Show the list as it stands on disk, without changing it.
    fn publish_recents(&mut self, cx: &mut Context<Self>) {
        let files = self.recents();
        self.sidebar.update(cx, |sidebar, cx| {
            sidebar.set_recents(files.entries().to_vec(), cx)
        });
    }

    /// Note that `path` was opened, putting it at the top of the list.
    fn remember(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let mut files = self.recents();
        files.push(path);
        self.store_recents(files, Some(window), cx);
    }

    /// Drop `path` from the list.
    fn forget(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let mut files = self.recents();
        let before = files.len();
        files.remove(path);
        if files.len() == before {
            // It was not in the list, and writing the file back to say nothing
            // had changed would be a write for nothing.
            return;
        }
        self.store_recents(files, Some(window), cx);
    }

    /// Point the recent list at where a rename just moved its files.
    ///
    /// The same job [`Self::repath_tabs`] does for the tabs, and for the same
    /// reason: an entry still naming the old path would offer a file that is
    /// no longer there.
    fn repath_recents(&mut self, from: &Path, to: &Path, cx: &mut Context<Self>) {
        let mut files = self.recents();
        let moved: Vec<PathBuf> = files
            .entries()
            .iter()
            .map(|current| match current.strip_prefix(from) {
                Ok(rest) => to.join(rest),
                Err(_) => current.clone(),
            })
            .collect();
        if moved == files.entries() {
            return;
        }

        // `push` puts each entry at the front, so rebuilding the list means
        // walking it backwards — the entries are already most-recent-first.
        files.clear();
        for path in moved.into_iter().rev() {
            files.push(path);
        }
        self.store_recents(files, None, cx);
    }

    /// Keep the recent list in a file of the caller's choosing.
    ///
    /// Only the tests need this; a workspace in the app always uses the
    /// platform's configuration directory. See the field for why.
    #[cfg(test)]
    pub(crate) fn use_recents_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.recents = Some(path);
        self.publish_recents(cx);
    }

    /// Carry out a change the tree's context menu asked for.
    ///
    /// The sidebar can see the filesystem but not the tabs, and every one of
    /// these changes can invalidate a tab. The checks that keep the two in step
    /// live here, because only the workspace can see both.
    fn handle_request(
        &mut self,
        request: SidebarRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match request {
            SidebarRequest::NewFile { directory } => {
                self.prompt_for_name(NameTarget::NewFile { directory }, window, cx);
            }
            SidebarRequest::Rename { path } => {
                self.prompt_for_name(NameTarget::Rename { path }, window, cx);
            }
            SidebarRequest::DeleteFile { path } => self.confirm_delete(path, window, cx),
        }
    }

    /// Ask for a name, then carry out `target` with it.
    ///
    /// Creating a file, renaming one and saving under a new name differ only in
    /// what they do with the name, so they share one prompt.
    fn prompt_for_name(&self, target: NameTarget, window: &mut Window, cx: &mut Context<Self>) {
        // The dialog's callbacks are handed `&mut App`, not `Context<Workspace>`,
        // so the workspace is reached through a weak handle — see
        // `confirm_discard` for why a strong one would be a cycle.
        let workspace = cx.weak_entity();
        let form = NameForm::new(target.caption(), target.initial_name(), window, cx);
        // Read out everything that has to survive the dialog being built: the
        // builder runs on every frame, and the form itself is only needed
        // afterwards to put the caret in the field.
        let focus = form.read(cx).input.read(cx).focus_handle(cx).clone();
        let title = target.title();
        let dialog_form = form.clone();

        window.open_dialog(cx, move |dialog, _, cx| {
            let workspace = workspace.clone();
            let form = dialog_form.clone();
            let target = target.clone();

            // `Dialog` carries the button labels in `DialogButtonProps` rather
            // than taking them directly, and its Cancel button is off by
            // default — the one that pairs with Escape has to be asked for.
            // Built here rather than once outside, because the label follows the
            // form: a name that turns out to be taken changes the button from
            // the target's own verb to what pressing it will actually do.
            let buttons = DialogButtonProps::default()
                .show_cancel(true)
                .ok_text(if form.read(cx).confirmed.is_some() {
                    "Replace"
                } else {
                    target.ok_text()
                })
                .cancel_text("Cancel");

            dialog
                .title(title)
                .w(px(380.))
                .button_props(buttons)
                .content({
                    let form = form.clone();
                    move |content, _, _| content.child(form.clone())
                })
                .on_ok(move |_, window, cx| {
                    let name = form.read(cx).value(cx);
                    // Only a path the user has already been told about counts
                    // as agreeing to replace it.
                    let confirmed = form.read(cx).confirmed.clone();
                    match workspace.update(cx, |workspace, cx| {
                        workspace.apply_name(&target, &name, confirmed.as_deref(), cx)
                    }) {
                        Ok(NameOutcome::Done) => true,
                        // A name the filesystem will not take keeps the dialog
                        // up with the reason under the field, so it can be
                        // corrected without retyping it.
                        Ok(NameOutcome::Refused(message)) => {
                            form.update(cx, |form, cx| form.reject(message, window, cx));
                            false
                        }
                        // Same, but the next press of the button goes through.
                        Ok(NameOutcome::Replace { path, message }) => {
                            form.update(cx, |form, cx| {
                                form.confirm_replace(path, message, window, cx)
                            });
                            false
                        }
                        // The workspace is gone; there is nobody left to name a
                        // file for, and nobody to tell.
                        Err(_) => true,
                    }
                })
        });

        // `open_dialog` moves focus to the dialog itself, so the field has to be
        // focused after it. Doing so bypasses the dialog's focus trap, which
        // only ever governs Tab and Shift-Tab. Enter and Escape still reach the
        // dialog rather than being swallowed by the field: a single-line input
        // propagates both.
        window.focus(&focus, cx);
    }

    /// Carry out a name prompt's request.
    ///
    /// The outcome is returned rather than reported here, because the dialog is
    /// what shows it — and it can only keep itself open for a name it was told
    /// about.
    fn apply_name(
        &mut self,
        target: &NameTarget,
        name: &str,
        confirmed: Option<&Path>,
        cx: &mut Context<Self>,
    ) -> NameOutcome {
        match target {
            NameTarget::NewFile { directory } => {
                if let Err(error) = fs::create_file(directory, name) {
                    return NameOutcome::Refused(error.to_string());
                }
            }
            NameTarget::Rename { path } => {
                // Decided here rather than carried in the request: whether a
                // path is a directory is a fact about the filesystem, and
                // reading it now is what makes it true at the moment it matters.
                match fs::rename(path, name, path.is_dir()) {
                    Ok(moved) => {
                        self.repath_tabs(path, &moved, cx);
                        self.repath_recents(path, &moved, cx);
                    }
                    Err(error) => return NameOutcome::Refused(error.to_string()),
                }
            }
            NameTarget::SaveAs { tab, directory, .. } => {
                let resolved = match fs::resolve_file_name(name) {
                    Ok(resolved) => resolved,
                    Err(error) => return NameOutcome::Refused(error.to_string()),
                };
                let outcome = self.save_tab_as(tab, directory.join(resolved), confirmed, cx);
                if !matches!(outcome, NameOutcome::Done) {
                    return outcome;
                }
            }
        }

        // The rows came from disk, and the disk just changed.
        self.sidebar.update(cx, |sidebar, cx| sidebar.refresh(cx));
        NameOutcome::Done
    }

    /// Write `tab` to `path`, refusing the two ways that could lose work.
    ///
    /// Split out of [`Self::apply_name`] because it is the one branch that has
    /// to consult the tabs as well as the filesystem, and that is the whole
    /// reason the prompt goes through the workspace rather than being carried
    /// out by the dialog.
    fn save_tab_as(
        &mut self,
        tab: &Entity<Tab>,
        path: PathBuf,
        confirmed: Option<&Path>,
        cx: &mut Context<Self>,
    ) -> NameOutcome {
        // Saving to the name it already has is an ordinary save. There is
        // nothing to warn about: the file being written over is this tab's own.
        if tab.read(cx).path() == Some(path.as_path()) {
            return match tab.update(cx, |tab, cx| tab.save_to(None, cx)) {
                Ok(()) => NameOutcome::Done,
                Err(error) => NameOutcome::Refused(error.to_string()),
            };
        }

        // Two tabs over one file means each save silently undoes the other,
        // whichever order they come in. Refused rather than confirmed, because
        // confirming would leave the other tab showing text that is no longer
        // there.
        if self.open_tab_for(&path, cx).is_some() {
            return NameOutcome::Refused(format!(
                "“{}” is open in another tab. Close that tab, or save under a different name.",
                file_label(&path)
            ));
        }

        // There is no undo for a file written over, so a name that is already
        // taken has to be asked about twice.
        if path.exists() && confirmed != Some(path.as_path()) {
            let message = format!(
                "“{}” already exists. Replacing it cannot be undone.",
                file_label(&path)
            );
            return NameOutcome::Replace { path, message };
        }

        match tab.update(cx, |tab, cx| tab.save_to(Some(path), cx)) {
            Ok(()) => NameOutcome::Done,
            Err(error) => NameOutcome::Refused(error.to_string()),
        }
    }

    /// `Cmd+S`: write the active tab to its file.
    ///
    /// A buffer that has never been saved has no file to write to, so it gets
    /// the same question Save As asks rather than a failure.
    fn save_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active).cloned() else {
            return;
        };
        if tab.read(cx).path().is_none() {
            self.save_as_active(window, cx);
            return;
        }

        if let Err(error) = tab.update(cx, |tab, cx| tab.save_to(None, cx)) {
            let name = tab.read(cx).title();
            window.push_notification(
                Notification::error(format!("Could not save “{name}”: {error}")),
                cx,
            );
        }
    }

    /// `Cmd+Shift+S`: write the active tab to a name the user chooses.
    fn save_as_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active).cloned() else {
            return;
        };
        let directory = self.naming_directory(&tab, cx);
        let suggested = tab.read(cx).path().map(file_label).unwrap_or_default();
        self.prompt_for_name(
            NameTarget::SaveAs {
                tab,
                directory,
                suggested,
            },
            window,
            cx,
        );
    }

    /// Where a save-as prompt should start from: the folder the file already
    /// lives in, otherwise the folder the tree is rooted at, otherwise the
    /// working directory — whichever of the three the app can actually name.
    fn naming_directory(&self, tab: &Entity<Tab>, cx: &App) -> PathBuf {
        if let Some(parent) = tab.read(cx).path().and_then(Path::parent) {
            return parent.to_path_buf();
        }
        self.sidebar
            .read(cx)
            .root()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    }

    /// `Cmd+Shift+E`: write the active tab out as a standalone HTML page.
    ///
    /// This uses the platform's own save panel rather than the in-app name
    /// prompt that Save As uses. The two are asking different questions: saving
    /// renames the document you are editing, so the folder it lives in is the
    /// right place to look, while an export is a *new* file that usually
    /// belongs somewhere else entirely. A save panel is built for exactly that,
    /// and it will not let the user overwrite a directory by accident.
    fn export_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active).cloned() else {
            return;
        };
        let directory = self.naming_directory(&tab, cx);
        let suggested = export::suggested_file_name(&tab.read(cx).title());
        let picked = cx.prompt_for_new_path(&directory, Some(&suggested));
        // `Context::spawn` hands out an `AsyncApp`, which is not tied to a
        // window, so the one to report into has to be carried across.
        let window = window.window_handle();

        cx.spawn(async move |this, cx| {
            // Cancelling needs no explanation, and a platform without a save
            // panel cannot be fixed by an error message of ours.
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };

            let written = this.update(cx, |_, cx| {
                let markdown = tab.read(cx).markdown(cx);
                let title = tab.read(cx).title();
                export::export_html(&path, &title, &markdown)
            });

            let note = match written {
                // The workspace is gone, so there is nowhere to show anything.
                Err(_) => return,
                Ok(Ok(())) => Notification::info(format!("Exported to {}", file_label(&path))),
                Ok(Err(error)) => Notification::error(format!(
                    "Could not export to “{}”: {error}",
                    file_label(&path)
                )),
            };
            cx.update_window(window, |_, window, cx| window.push_notification(note, cx))
                .ok();
        })
        .detach();
    }

    /// Delete a file, asking first.
    ///
    /// A file an open tab is showing is refused rather than deleted along with
    /// its tab: the tab's unsaved edits would go with it, and there is no undo.
    /// A refusal can always be retried; a discarded buffer cannot be recovered.
    fn confirm_delete(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.open_tab_for(&path, cx) {
            // Bring the tab up so the refusal points at something on screen.
            self.select(index, cx);
            let name = file_label(&path);
            window.push_notification(
                Notification::warning(format!(
                    "“{name}” is open in a tab. Close it before deleting the file."
                )),
                cx,
            );
            return;
        }

        let sidebar = self.sidebar.clone();
        let workspace = cx.weak_entity();
        let name = file_label(&path);
        window.open_alert_dialog(cx, move |alert, _, _| {
            let sidebar = sidebar.clone();
            let workspace = workspace.clone();
            let path = path.clone();
            alert
                .confirm()
                .title("Delete this file?")
                .description(format!(
                    "“{name}” will be removed from disk. This cannot be undone."
                ))
                .ok_text("Delete")
                .ok_variant(ButtonVariant::Danger)
                .cancel_text("Cancel")
                .on_ok(move |_, window, cx| {
                    // A tree row can outlive the file it names — it was read
                    // from disk, and something else may have removed it since.
                    // Reporting that is the difference between a stale row and a
                    // click that appeared to do nothing.
                    match fs::delete_file(&path) {
                        // Only once it is really gone: a delete that failed left
                        // the file where it was, and the recent entry pointing at
                        // it is still good.
                        Ok(()) => {
                            workspace
                                .update(cx, |workspace, cx| workspace.forget(&path, window, cx))
                                .ok();
                        }
                        Err(error) => {
                            let name = file_label(&path);
                            window.push_notification(
                                Notification::error(format!("Could not delete “{name}”: {error}")),
                                cx,
                            );
                        }
                    }
                    sidebar.update(cx, |sidebar, cx| sidebar.refresh(cx));
                    true
                })
        });
    }

    /// The index of the tab showing `path`, if one is.
    ///
    /// Separate from `confirm_delete` so the guard that protects an open buffer
    /// can be tested without going through a dialog.
    fn open_tab_for(&self, path: &Path, cx: &App) -> Option<usize> {
        self.tabs
            .iter()
            .position(|tab| tab.read(cx).path() == Some(path))
    }

    /// Point the tabs that were showing something a rename just moved at where
    /// it went.
    ///
    /// A tab left on its old path would show a stale name, and its next save
    /// would write that old path back into existence — recreating the very file
    /// that was renamed away. Renaming a directory moves everything inside it,
    /// so a tab under the renamed folder is retargeted too.
    fn repath_tabs(&mut self, from: &Path, to: &Path, cx: &mut Context<Self>) {
        for tab in &self.tabs {
            let Some(current) = tab.read(cx).path().map(Path::to_path_buf) else {
                continue;
            };
            let moved = if current == from {
                Some(to.to_path_buf())
            } else {
                current.strip_prefix(from).ok().map(|rest| to.join(rest))
            };
            if let Some(moved) = moved {
                tab.update(cx, |tab, cx| tab.repath(moved, cx));
            }
        }
    }

    /// Close the active tab, asking first when it has unsaved edits.
    pub fn request_close_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        if !tab.read(cx).is_dirty() {
            self.close_active(cx);
            return;
        }

        let name = tab.read(cx).title();
        self.confirm_discard(
            window,
            cx,
            format!("“{name}” has unsaved changes. Closing it will discard them."),
            "Discard",
            Self::close_active,
        );
    }

    /// Quit, asking first if any tab has unsaved edits.
    pub fn request_quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dirty = self
            .tabs
            .iter()
            .filter(|tab| tab.read(cx).is_dirty())
            .count();
        if dirty == 0 {
            cx.quit();
            return;
        }

        let what = if dirty == 1 {
            "1 tab has unsaved changes.".to_string()
        } else {
            format!("{dirty} tabs have unsaved changes.")
        };
        self.confirm_discard(
            window,
            cx,
            format!("{what} Quitting will discard them."),
            "Quit anyway",
            |_, cx| cx.quit(),
        );
    }

    /// Ask before discarding edits, then run `on_confirm`.
    ///
    /// The dialog's callbacks are handed `&mut App`, not `Context<Workspace>`,
    /// so the workspace is reached through a weak handle. A strong one would
    /// close a reference cycle through the window root, which owns the dialog.
    fn confirm_discard(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        message: String,
        ok_text: &'static str,
        on_confirm: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) {
        let workspace = cx.weak_entity();
        // The dialog's builder is an `Fn`, not an `FnOnce` — it may in principle
        // run more than once — so nothing may be moved out of it. Both captures
        // are therefore cloned into the innermost closure instead.
        let on_confirm = Rc::new(on_confirm);
        window.open_alert_dialog(cx, move |alert, _, _| {
            let workspace = workspace.clone();
            let on_confirm = on_confirm.clone();
            alert
                .confirm()
                .title("Discard unsaved changes?")
                .description(message.clone())
                .ok_text(ok_text)
                .cancel_text("Keep editing")
                .on_ok(move |_, _, cx| {
                    // Whatever happens next, the dialog has served its purpose;
                    // leaving it up would only invite a second confirm.
                    workspace
                        .update(cx, |workspace, cx| on_confirm(workspace, cx))
                        .ok();
                    true
                })
        });
    }

    /// Close the active tab and fall back to its left-hand neighbour.
    fn close_active(&mut self, cx: &mut Context<Self>) {
        if self.tabs.is_empty() {
            return;
        }
        self.tabs.remove(self.active);
        // `saturating_sub` covers the case where the last tab was closed and
        // `tabs` is now empty; `active` is simply not read again until a tab
        // is opened, and opening sets it.
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        cx.notify();
    }

    /// Focus the tab at `index`, ignoring an out-of-range request.
    pub fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.tabs.len() && index != self.active {
            self.active = index;
            cx.notify();
        }
    }

    fn render_tab_strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let dirty_dot = cx.theme().primary;
        let buttons = self
            .tabs
            .iter()
            .map(|tab| {
                let tab = tab.read(cx);
                let button = TabButton::new().label(tab.title());
                if tab.is_dirty() {
                    button.suffix(div().size(px(7.)).rounded_full().bg(dirty_dot))
                } else {
                    button
                }
            })
            .collect::<Vec<_>>();

        // `TabBar::on_click` hands the callback a plain `&mut App` rather than a
        // `Context<Workspace>`, so `cx.listener` cannot be used here. A weak
        // handle avoids the cycle a strong `Entity` would create.
        let workspace = cx.weak_entity();
        let tabs = TabBar::new("tabs")
            .selected_index(self.active)
            .children(buttons)
            .on_click(move |index, _, cx| {
                workspace
                    .update(cx, |workspace, cx| workspace.select(*index, cx))
                    .ok();
            });

        // The theme toggle rides on the right of the tab strip: it is the one
        // control that applies to the whole window, and the strip is the only
        // row that is always there. It dispatches rather than calling in, so
        // the key binding and the click take the same path.
        let preference = AppSettings::current(cx).theme;
        div()
            .flex()
            .items_center()
            .child(div().flex_1().min_w_0().child(tabs))
            .child(
                Button::new("theme-toggle")
                    .ghost()
                    .label(preference.label())
                    .tooltip("Theme: follow the system, light, or dark")
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleTheme), cx)),
            )
    }

    /// Advance the theme preference and write it down.
    ///
    /// The theme changes even when saving fails; only the button label and the
    /// next launch are affected, and a notification says so.
    fn toggle_theme(&mut self, _: &ToggleTheme, window: &mut Window, cx: &mut Context<Self>) {
        let (preference, failure) = settings::cycle_theme(window, cx);
        if let Some(error) = failure {
            window.push_notification(
                Notification::warning(format!(
                    "The theme is {} for now, but saving the setting failed: {error}",
                    preference.label()
                )),
                cx,
            );
        }
    }

    fn render_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(tab) = self.tabs.get(self.active) else {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child("No open documents")
                .into_any_element();
        };

        h_resizable("workspace")
            .child(resizable_panel().size(px(220.)).child(self.sidebar.clone()))
            .child(resizable_panel().child(tab.clone()))
            .into_any_element()
    }
}

/// What a name prompt will do with the name it collects.
#[derive(Clone, Debug, PartialEq, Eq)]
enum NameTarget {
    /// Create a new file inside this directory.
    NewFile { directory: PathBuf },
    /// Move this file or directory to a new name, within the folder it is in.
    Rename { path: PathBuf },
    /// Write this tab to a file named here, inside this directory.
    ///
    /// The tab is named rather than looked up when the name arrives, so the
    /// prompt cannot end up saving whichever tab happens to be active later.
    /// `suggested` is read out when the prompt opens: a copy usually differs
    /// from its original by a word, and the field is easier to correct than to
    /// fill in.
    SaveAs {
        tab: Entity<Tab>,
        directory: PathBuf,
        suggested: String,
    },
}

impl NameTarget {
    fn title(&self) -> &'static str {
        match self {
            Self::NewFile { .. } => "New file",
            // Whether this is a file or a folder is what the user has to be
            // told first, since the two take different naming rules.
            Self::Rename { path } if path.is_dir() => "Rename folder",
            Self::Rename { .. } => "Rename file",
            Self::SaveAs { .. } => "Save as",
        }
    }

    fn ok_text(&self) -> &'static str {
        match self {
            Self::NewFile { .. } => "Create",
            Self::Rename { .. } => "Rename",
            Self::SaveAs { .. } => "Save",
        }
    }

    /// The name to start the field with: empty for a new file, and the current
    /// name for a rename or a save-as, so correcting one character does not mean
    /// retyping the whole name.
    fn initial_name(&self) -> String {
        match self {
            Self::NewFile { .. } => String::new(),
            Self::Rename { path } => file_label(path),
            Self::SaveAs { suggested, .. } => suggested.clone(),
        }
    }

    /// A line naming the folder the change lands in, so the field is not asking
    /// for a name without saying where it will go.
    fn caption(&self) -> String {
        let directory = match self {
            Self::NewFile { directory } | Self::SaveAs { directory, .. } => {
                Some(directory.as_path())
            }
            Self::Rename { path } => path.parent(),
        };
        match directory {
            Some(directory) if !directory.as_os_str().is_empty() => {
                format!("in {}", directory.display())
            }
            _ => String::new(),
        }
    }
}

/// What carrying out a name prompt's request did, as far as the dialog is
/// concerned.
#[derive(Clone, Debug, PartialEq, Eq)]
enum NameOutcome {
    /// Done, and the dialog can close.
    Done,
    /// Refused. The dialog stays up with this message under the field, so the
    /// name can be corrected without retyping it.
    Refused(String),
    /// The name belongs to a file that is already there. The dialog stays up,
    /// and its OK button becomes Replace — sending this same path back as the
    /// confirmed one is what makes the replacement happen.
    ///
    /// Held as a path rather than a flag so that editing the name afterwards
    /// withdraws the confirmation on its own: a different name resolves to a
    /// different path, and a different path has not been agreed to.
    Replace { path: PathBuf, message: String },
}

/// The dialog body that collects a file name.
///
/// An entity rather than a few fields on the workspace because the dialog's
/// content builder is an `Fn` that runs on every frame: the message under the
/// field has to be read fresh each time, and a captured `String` would freeze
/// it at whatever it was when the dialog opened.
struct NameForm {
    input: Entity<InputState>,
    caption: String,
    /// Why the last submitted name was refused, or `None` before any was.
    error: Option<String>,
    /// The exact path the user has agreed to overwrite, if any.
    ///
    /// A path and not a flag: it is what the OK button's label is read from,
    /// and it stops a confirmation given for one name from carrying over to
    /// another one typed afterwards.
    confirmed: Option<PathBuf>,
}

impl NameForm {
    fn new(
        caption: String,
        initial_name: String,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("notes.md")
                .default_value(initial_name)
        });
        cx.new(|_| Self {
            input,
            caption,
            error: None,
            confirmed: None,
        })
    }

    fn value(&self, cx: &App) -> String {
        self.input.read(cx).value().to_string()
    }

    /// Record a refusal, and put the caret back in the field so the name can be
    /// corrected without another click.
    fn reject(&mut self, message: String, window: &mut Window, cx: &mut Context<Self>) {
        // A refusal withdraws any earlier agreement to replace: the name it was
        // given for is not the one being refused now.
        self.confirmed = None;
        self.show(message, window, cx);
    }

    /// Record what the name would replace, and ask again more explicitly.
    fn confirm_replace(
        &mut self,
        path: PathBuf,
        message: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirmed = Some(path);
        self.show(message, window, cx);
    }

    /// Show `message` under the field and put the caret back in it.
    ///
    /// The window is refreshed because the OK button's label is built by the
    /// dialog, not by this form — this entity notifying on its own would update
    /// the message and leave the button reading "Save".
    fn show(&mut self, message: String, window: &mut Window, cx: &mut Context<Self>) {
        self.error = Some(message);
        let focus = self.input.read(cx).focus_handle(cx).clone();
        window.focus(&focus, cx);
        window.refresh();
        cx.notify();
    }
}

impl Render for NameForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = v_flex().gap_2();
        if !self.caption.is_empty() {
            body = body.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.caption.clone()),
            );
        }
        body = body.child(Input::new(&self.input));
        if let Some(error) = &self.error {
            body = body.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(error.clone()),
            );
        }
        body
    }
}

/// The name to show in a message for `path`.
///
/// A file name rather than a whole path, because a message about a file the
/// user just clicked should name it the way the tree does.
fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Tell the user a file could not be opened.
///
/// A free function because it needs nothing from the workspace: the dialog is
/// owned by the window, and by the time this runs the tab was never created.
fn report_open_failure(path: &Path, error: &io::Error, window: &mut Window, cx: &mut App) {
    let message = format!("{}\n\n{error}", file_label(path));

    // `AlertDialog` already offers an OK button and no cancel, so this needs
    // neither `.confirm()` nor `.show_cancel(false)`.
    window.open_alert_dialog(cx, move |alert, _, _| {
        alert
            .title("Could not open file")
            .description(message.clone())
            .ok_text("OK")
    });
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(|this, _: &Quit, window, cx| this.request_quit(window, cx)))
            .on_action(
                cx.listener(|this, _: &CloseTab, window, cx| this.request_close_active(window, cx)),
            )
            .on_action(cx.listener(|this, action: &SelectTab, _, cx| this.select(action.0, cx)))
            .on_action(cx.listener(Self::toggle_theme))
            .on_action(cx.listener(|this, _: &Save, window, cx| this.save_active(window, cx)))
            .on_action(cx.listener(|this, _: &SaveAs, window, cx| this.save_as_active(window, cx)))
            .on_action(
                cx.listener(|this, _: &ExportHtml, window, cx| this.export_active(window, cx)),
            )
            .on_action(cx.listener(|this, _: &OpenFile, window, cx| this.open_file(window, cx)))
            .child(self.render_tab_strip(cx))
            .child(div().flex_1().min_h_0().child(self.render_body(cx)))
    }
}

#[cfg(test)]
mod tests {
    // Imported narrowly: `use super::*` would drag in the `gpui_kit::*` glob,
    // whose `test` attribute macro shadows the built-in `#[test]`.
    use super::{NameOutcome, NameTarget, STARTER_DOCUMENT, Workspace};
    use crate::actions::{CloseTab, ExportHtml, OpenFile, SelectTab};
    use crate::sidebar::SidebarRequest;
    use gpui_kit::base::Root;
    use gpui_kit::component::WindowExt as _;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AppContext as _, Bounds, Entity, Point, TestAppContext, WindowBounds, WindowHandle,
        WindowOptions, px, size,
    };
    use md_core::Document;
    use md_core::RecentFiles;
    use md_core::fs::NameError;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A path no other test is using.
    ///
    /// The pid is not enough on its own: the tests run in parallel, so a single
    /// shared name would have them reading each other's lists. The counter makes
    /// every call distinct within the run.
    fn private_recents() -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        std::env::temp_dir().join(format!(
            "md-app-recents-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn open_workspace(cx: &mut TestAppContext) -> (WindowHandle<Root>, Entity<Workspace>) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            let bounds = Bounds {
                origin: Point::default(),
                size: size(px(1200.), px(820.)),
            };
            let (window, workspace) = gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| Workspace::new(window, cx)),
            )
            .expect("open test window");
            // `Workspace::new` points the recent list at the developer's real
            // configuration directory. Several tests below open real files, and
            // opening a file records it — left alone, a `cargo test` would
            // overwrite the list of whoever ran it with paths out of `/tmp`.
            workspace.update(cx, |workspace, cx| {
                workspace.use_recents_file(private_recents(), cx);
            });
            (window.downcast::<Root>().expect("base Root"), workspace)
        })
    }

    fn titles(cx: &TestAppContext, workspace: &Entity<Workspace>) -> Vec<String> {
        workspace.read_with(cx, |workspace, cx| {
            workspace
                .tabs
                .iter()
                .map(|tab| tab.read(cx).title())
                .collect()
        })
    }

    fn active(workspace: &Entity<Workspace>, cx: &TestAppContext) -> usize {
        workspace.read_with(cx, |workspace, _| workspace.active)
    }

    fn dispatch<A: gpui_kit::Action>(
        window: WindowHandle<Root>,
        action: A,
        cx: &mut TestAppContext,
    ) {
        cx.update_window(window.into(), |_, window, cx| {
            window.dispatch_action(Box::new(action), cx);
        })
        .unwrap();
    }

    /// A document with no unsaved edits, backed by a real file so that
    /// `is_dirty` is false and closing it needs no confirmation.
    fn clean_document(dir: &Path, name: &str) -> Document {
        let path = dir.join(name);
        std::fs::write(&path, format!("# {name}")).unwrap();
        Document::open(&path).unwrap()
    }

    fn open_document(
        window: WindowHandle<Root>,
        workspace: &Entity<Workspace>,
        document: Document,
        cx: &mut TestAppContext,
    ) {
        cx.update_window(window.into(), |_, window, cx| {
            workspace.update(cx, |workspace, cx| workspace.open(document, window, cx));
        })
        .unwrap();
    }

    /// A scratch directory private to one test, so the tests can run in
    /// parallel without treading on each other's files.
    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("md-app-workspace-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn has_dialog(window: WindowHandle<Root>, cx: &mut TestAppContext) -> bool {
        cx.update_window(window.into(), |_, window, cx| window.has_active_dialog(cx))
            .unwrap()
    }

    /// A fresh workspace holds one tab, showing the starter document.
    #[gpui_kit::test]
    fn a_new_workspace_opens_one_tab(cx: &mut TestAppContext) {
        let (_, workspace) = open_workspace(cx);
        assert_eq!(active(&workspace, cx), 0);
        assert_eq!(titles(cx, &workspace), ["Untitled"]);
    }

    /// Opening focuses the new tab; closing falls back to the previous one.
    #[gpui_kit::test]
    fn opening_and_closing_tabs_tracks_the_active_index(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);
        let dir = scratch("index");
        open_document(window, &workspace, clean_document(&dir, "a.md"), cx);
        open_document(window, &workspace, clean_document(&dir, "b.md"), cx);

        assert_eq!(titles(cx, &workspace), ["Untitled", "a.md", "b.md"]);
        assert_eq!(
            active(&workspace, cx),
            2,
            "opening should focus the new tab"
        );

        dispatch(window, CloseTab, cx);
        assert_eq!(titles(cx, &workspace), ["Untitled", "a.md"]);
        assert_eq!(active(&workspace, cx), 1, "should fall back to the left");

        // The remaining tab is the starter document, which has never been
        // saved and so is dirty; `CloseTab` would prompt rather than close it.
        // Drive the close directly to exercise the empty-strip edge case; the
        // prompt itself is covered by `closing_a_dirty_tab_asks_before_discarding`.
        workspace.update(cx, |workspace, cx| workspace.close_active(cx));
        workspace.update(cx, |workspace, cx| workspace.close_active(cx));
        assert!(titles(cx, &workspace).is_empty());
        // Closing an already-empty strip is a no-op, not a panic.
        workspace.update(cx, |workspace, cx| workspace.close_active(cx));
        assert!(titles(cx, &workspace).is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// `SelectTab` moves between tabs and rejects an out-of-range index.
    #[gpui_kit::test]
    fn select_tab_focuses_a_known_tab_only(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);
        let dir = scratch("select");
        open_document(window, &workspace, clean_document(&dir, "a.md"), cx);
        assert_eq!(active(&workspace, cx), 1);

        dispatch(window, SelectTab(0), cx);
        assert_eq!(active(&workspace, cx), 0);

        dispatch(window, SelectTab(7), cx);
        assert_eq!(active(&workspace, cx), 0, "an unknown index is ignored");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Two open documents do not share a buffer: each tab's editor holds the
    /// text it was opened with.
    #[gpui_kit::test]
    fn each_tab_keeps_its_own_buffer(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);
        let dir = scratch("buffers");
        open_document(window, &workspace, clean_document(&dir, "a.md"), cx);

        let buffers = workspace.read_with(cx, |workspace, cx| {
            workspace
                .tabs
                .iter()
                .map(|tab| tab.read(cx).text(cx))
                .collect::<Vec<_>>()
        });
        assert_eq!(buffers, [STARTER_DOCUMENT, "# a.md"]);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// `Cmd+W` on a tab with unsaved edits must not throw them away. It puts up
    /// a dialog and leaves the tab alone until the user answers.
    #[gpui_kit::test]
    fn closing_a_dirty_tab_asks_before_discarding(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);

        // Opening a tab focuses its editor, so this types into the starter
        // document and leaves it dirty.
        cx.update_window(window.into(), |_, window, cx| window.input("edited", cx))
            .unwrap();
        assert!(
            workspace.read_with(cx, |workspace, cx| workspace.tabs[0].read(cx).is_dirty()),
            "typing should have made the tab dirty"
        );

        dispatch(window, CloseTab, cx);
        assert_eq!(
            titles(cx, &workspace),
            ["Untitled"],
            "the tab must survive until the discard is confirmed"
        );
        assert!(has_dialog(window, cx), "a confirmation should be on screen");

        // Dismissing keeps both the tab and the edits.
        cx.update_window(window.into(), |_, window, cx| window.close_dialog(cx))
            .unwrap();
        assert_eq!(titles(cx, &workspace), ["Untitled"]);
        assert!(workspace.read_with(cx, |workspace, cx| workspace.tabs[0].read(cx).is_dirty()));
    }

    /// Opening a file from the sidebar adds a tab; opening it a second time
    /// brings its tab forward instead of opening the same file twice.
    #[gpui_kit::test]
    fn opening_a_file_from_the_sidebar_uses_one_tab(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);
        let dir = scratch("sidebar-open");
        let path = dir.join("from-tree.md");
        std::fs::write(&path, "# hello").unwrap();

        let sidebar = workspace.read_with(cx, |workspace, _| workspace.sidebar.clone());
        sidebar.update(cx, |sidebar, cx| sidebar.open(path.clone(), cx));

        assert_eq!(titles(cx, &workspace), ["Untitled", "from-tree.md"]);
        assert_eq!(active(&workspace, cx), 1, "the opened file is focused");

        // Move away, then ask for the same file again: the sidebar's request is
        // what is being tested, so it must not depend on which tab is active.
        dispatch(window, SelectTab(0), cx);
        sidebar.update(cx, |sidebar, cx| sidebar.open(path.clone(), cx));

        assert_eq!(
            titles(cx, &workspace),
            ["Untitled", "from-tree.md"],
            "a file already open must not be opened a second time"
        );
        assert_eq!(active(&workspace, cx), 1, "its existing tab comes forward");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// A tree row for a file that is gone reports the failure instead of doing
    /// nothing, which would read as a click that was ignored.
    #[gpui_kit::test]
    fn opening_a_missing_file_is_reported(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);
        let dir = scratch("sidebar-missing");

        let sidebar = workspace.read_with(cx, |workspace, _| workspace.sidebar.clone());
        sidebar.update(cx, |sidebar, cx| sidebar.open(dir.join("gone.md"), cx));

        assert_eq!(titles(cx, &workspace), ["Untitled"], "no tab was opened");
        assert!(has_dialog(window, cx), "the failure should be on screen");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The recent list the sidebar is showing.
    ///
    /// Read from the sidebar rather than the file because this is the list a
    /// click would actually open from, and it is not re-read from disk between
    /// a change and this call — so it is where a broken update would show.
    fn shown_recents(cx: &TestAppContext, workspace: &Entity<Workspace>) -> Vec<PathBuf> {
        let sidebar = workspace.read_with(cx, |workspace, _| workspace.sidebar.clone());
        sidebar.read_with(cx, |sidebar, _| sidebar.recents().to_vec())
    }

    /// The recent list as it stands in the workspace's own file.
    fn stored_recents(cx: &TestAppContext, workspace: &Entity<Workspace>) -> Vec<PathBuf> {
        let store = workspace.read_with(cx, |workspace, _| {
            workspace
                .recents
                .clone()
                .expect("every test workspace is pointed at a scratch file")
        });
        RecentFiles::load_from(&store).entries().to_vec()
    }

    /// `Cmd+O` asks the platform for a file, and opening the one it names gives
    /// a tab and a record of it.
    ///
    /// The whole round trip, because the await is what ships: a test that called
    /// `open_path` directly would not notice if the answer never came back.
    #[gpui_kit::test]
    fn opening_a_file_through_the_dialog_opens_its_tab(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);
        let dir = scratch("open-dialog");
        let picked = dir.join("picked.md");
        std::fs::write(&picked, "# picked").unwrap();

        dispatch::<OpenFile>(window, OpenFile, cx);
        assert!(
            cx.did_prompt_for_paths(),
            "Cmd+O has to ask which file to open"
        );

        cx.simulate_path_prompt_response(|_| Some(vec![picked.clone()]));
        cx.run_until_parked();

        assert_eq!(titles(cx, &workspace), ["Untitled", "picked.md"]);
        assert_eq!(shown_recents(cx, &workspace), vec![picked.clone()]);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Cancelling the file dialog opens nothing and records nothing.
    #[gpui_kit::test]
    fn cancelling_the_file_dialog_changes_nothing(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);

        dispatch::<OpenFile>(window, OpenFile, cx);
        assert!(cx.did_prompt_for_paths());
        cx.simulate_path_prompt_response(|_| None);
        cx.run_until_parked();

        assert_eq!(titles(cx, &workspace), ["Untitled"]);
        assert!(shown_recents(cx, &workspace).is_empty());
    }

    /// Opening a file records it, newest first, in the file as well as on
    /// screen.
    #[gpui_kit::test]
    fn opening_a_file_records_it_as_recent(cx: &mut TestAppContext) {
        let (_, workspace) = open_workspace(cx);
        let dir = scratch("recents");
        let first = dir.join("first.md");
        let second = dir.join("second.md");
        std::fs::write(&first, "# first").unwrap();
        std::fs::write(&second, "# second").unwrap();

        let sidebar = workspace.read_with(cx, |workspace, _| workspace.sidebar.clone());
        sidebar.update(cx, |sidebar, cx| sidebar.open(first.clone(), cx));
        sidebar.update(cx, |sidebar, cx| sidebar.open(second.clone(), cx));

        let expected = vec![second.clone(), first.clone()];
        assert_eq!(
            stored_recents(cx, &workspace),
            expected,
            "the newest file comes first, and the list is written out"
        );
        assert_eq!(
            shown_recents(cx, &workspace),
            expected,
            "the sidebar is shown what was written, not a list of its own"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Re-opening the file that is already in front still counts as opening
    /// it: the recent list is about what is being worked on, not what is new.
    #[gpui_kit::test]
    fn opening_the_active_file_again_still_records_it(cx: &mut TestAppContext) {
        let (_, workspace) = open_workspace(cx);
        let dir = scratch("recents-again");
        let first = dir.join("first.md");
        let second = dir.join("second.md");
        std::fs::write(&first, "# first").unwrap();
        std::fs::write(&second, "# second").unwrap();

        let sidebar = workspace.read_with(cx, |workspace, _| workspace.sidebar.clone());
        sidebar.update(cx, |sidebar, cx| sidebar.open(first.clone(), cx));
        sidebar.update(cx, |sidebar, cx| sidebar.open(second.clone(), cx));
        // `first` has a tab already, so this brings that tab forward rather
        // than creating one — and must still move it to the top of the list.
        sidebar.update(cx, |sidebar, cx| sidebar.open(first.clone(), cx));

        assert_eq!(
            titles(cx, &workspace),
            ["Untitled", "first.md", "second.md"]
        );
        assert_eq!(stored_recents(cx, &workspace), vec![first, second]);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// A remembered file that has since been removed is dropped when the click
    /// fails, so the sidebar stops offering a row that cannot work.
    #[gpui_kit::test]
    fn opening_a_vanished_recent_forgets_it(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);
        let dir = scratch("recents-gone");
        let path = dir.join("vanished.md");
        std::fs::write(&path, "# here").unwrap();

        let sidebar = workspace.read_with(cx, |workspace, _| workspace.sidebar.clone());
        sidebar.update(cx, |sidebar, cx| sidebar.open(path.clone(), cx));
        assert_eq!(shown_recents(cx, &workspace), vec![path.clone()]);

        // The tab has to go first. While it is open the file is not gone as far
        // as the app is concerned — the row brings that tab forward and never
        // touches the disk — so the entry would rightly survive.
        dispatch(window, CloseTab, cx);
        assert_eq!(titles(cx, &workspace), ["Untitled"]);

        // Removed behind the app's back — another program, or a volume that was
        // unmounted. The row is still on screen, so the click must say so.
        std::fs::remove_file(&path).unwrap();
        sidebar.update(cx, |sidebar, cx| sidebar.open(path.clone(), cx));

        assert!(has_dialog(window, cx), "the failure should be reported");
        assert!(
            shown_recents(cx, &workspace).is_empty(),
            "the entry should go, or every later glance at the sidebar offers it again"
        );
        assert!(
            stored_recents(cx, &workspace).is_empty(),
            "and the file must agree, or it comes back next launch"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Submit `name` to the workspace, exactly as the name dialog's OK button
    /// does once it has the text.
    fn apply(
        window: WindowHandle<Root>,
        workspace: &Entity<Workspace>,
        target: &NameTarget,
        name: &str,
        cx: &mut TestAppContext,
    ) -> NameOutcome {
        apply_confirmed(window, workspace, target, name, None, cx)
    }

    /// The same, for the second press of a button whose first press was turned
    /// down to ask about replacing something. `confirmed` is the path the form
    /// was told about, and only that path counts as agreed to.
    fn apply_confirmed(
        window: WindowHandle<Root>,
        workspace: &Entity<Workspace>,
        target: &NameTarget,
        name: &str,
        confirmed: Option<&Path>,
        cx: &mut TestAppContext,
    ) -> NameOutcome {
        cx.update_window(window.into(), |_, _, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.apply_name(target, name, confirmed, cx)
            })
        })
        .unwrap()
    }

    /// Type `text` into the active tab's source pane, as the user would.
    ///
    /// Going through the window rather than calling `Document::set_text` is the
    /// point: the editor is what a save reads, so a test that did not type into
    /// it would pass without proving anything about the text that gets written.
    fn type_into_active_tab(
        window: WindowHandle<Root>,
        workspace: &Entity<Workspace>,
        text: &str,
        cx: &mut TestAppContext,
    ) {
        let focus = workspace.read_with(cx, |workspace, cx| {
            workspace.tabs[workspace.active].read(cx).editor_focus(cx)
        });
        cx.update_window(window.into(), |_, window, cx| {
            window.focus(&focus, cx);
            window.input(text, cx);
        })
        .unwrap();
    }

    /// The path a tab is editing, read back through the entity.
    fn tab_path(
        workspace: &Entity<Workspace>,
        index: usize,
        cx: &TestAppContext,
    ) -> Option<PathBuf> {
        workspace.read_with(cx, |workspace, cx| {
            workspace.tabs[index].read(cx).path().map(Path::to_path_buf)
        })
    }

    /// Naming a new file writes it into the directory the menu was opened on,
    /// and adds the Markdown extension the name left off.
    #[gpui_kit::test]
    fn naming_a_new_file_writes_it_into_the_directory(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);
        let dir = scratch("new-file");

        let outcome = apply(
            window,
            &workspace,
            &NameTarget::NewFile {
                directory: dir.clone(),
            },
            "notes",
            cx,
        );
        assert_eq!(outcome, NameOutcome::Done);

        assert_eq!(std::fs::read_to_string(dir.join("notes.md")).unwrap(), "");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// A name that would land outside the directory is refused by the time it
    /// reaches the workspace, and nothing is written.
    #[gpui_kit::test]
    fn a_name_that_would_escape_the_directory_is_refused(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);
        let dir = scratch("escape");

        let outcome = apply(
            window,
            &workspace,
            &NameTarget::NewFile {
                directory: dir.clone(),
            },
            "../escape",
            cx,
        );

        assert_eq!(
            outcome,
            NameOutcome::Refused(NameError::NotAFileName.to_string()),
            "the refusal must reach the dialog, which is what shows it"
        );
        assert!(!dir.parent().unwrap().join("escape.md").exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Renaming a file from the tree moves it on disk and brings its open tab
    /// along, so the tab does not later save the old name back into existence.
    #[gpui_kit::test]
    fn renaming_a_file_moves_it_and_its_tab(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);
        let dir = scratch("rename");
        let before = dir.join("before.md");
        std::fs::write(&before, "# body").unwrap();

        let sidebar = workspace.read_with(cx, |workspace, _| workspace.sidebar.clone());
        sidebar.update(cx, |sidebar, cx| sidebar.open(before.clone(), cx));
        assert_eq!(titles(cx, &workspace), ["Untitled", "before.md"]);

        let outcome = apply(
            window,
            &workspace,
            &NameTarget::Rename {
                path: before.clone(),
            },
            "after",
            cx,
        );
        assert_eq!(outcome, NameOutcome::Done, "the rename should succeed");

        let after = dir.join("after.md");
        assert!(after.exists(), "the file should be at its new name");
        assert!(!before.exists(), "and gone from the old one");
        assert_eq!(titles(cx, &workspace), ["Untitled", "after.md"]);
        assert_eq!(
            tab_path(&workspace, 1, cx),
            Some(after),
            "the tab must follow the file, or its next save recreates the old one"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// A renamed file is renamed in the recent list too.
    ///
    /// An entry still naming the old path would offer a file that is no longer
    /// there — and the click would fail, which is how a rename would look like
    /// a bug in the recent list rather than a stale entry.
    #[gpui_kit::test]
    fn renaming_a_file_moves_its_recent_entry(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);
        let dir = scratch("rename-recent");
        let before = dir.join("before.md");
        std::fs::write(&before, "# body").unwrap();

        let sidebar = workspace.read_with(cx, |workspace, _| workspace.sidebar.clone());
        sidebar.update(cx, |sidebar, cx| sidebar.open(before.clone(), cx));
        assert_eq!(shown_recents(cx, &workspace), vec![before.clone()]);

        let outcome = apply(
            window,
            &workspace,
            &NameTarget::Rename {
                path: before.clone(),
            },
            "after",
            cx,
        );
        assert_eq!(outcome, NameOutcome::Done);

        let after = dir.join("after.md");
        assert_eq!(stored_recents(cx, &workspace), vec![after.clone()]);
        assert_eq!(shown_recents(cx, &workspace), vec![after]);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Renaming a directory moves everything inside it, so a tab under that
    /// directory is retargeted too.
    #[gpui_kit::test]
    fn renaming_a_directory_brings_the_tabs_inside_it_along(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);
        let dir = scratch("rename-dir");
        let folder = dir.join("drafts");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("note.md"), "# note").unwrap();

        let sidebar = workspace.read_with(cx, |workspace, _| workspace.sidebar.clone());
        sidebar.update(cx, |sidebar, cx| sidebar.open(folder.join("note.md"), cx));

        let outcome = apply(
            window,
            &workspace,
            &NameTarget::Rename {
                path: folder.clone(),
            },
            "notes",
            cx,
        );
        assert_eq!(outcome, NameOutcome::Done, "the rename should succeed");

        let moved = dir.join("notes").join("note.md");
        assert!(moved.exists());
        assert_eq!(
            tab_path(&workspace, 1, cx),
            Some(moved.clone()),
            "a tab under the renamed folder must move with it"
        );
        assert_eq!(
            shown_recents(cx, &workspace),
            vec![moved],
            "and so must a recent entry, for the same reason"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Deleting a file an open tab is showing is refused outright: the tab's
    /// unsaved edits would go with the file, and there is no undo.
    #[gpui_kit::test]
    fn deleting_a_file_that_is_open_is_refused(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);
        let dir = scratch("delete-open");
        let path = dir.join("open.md");
        std::fs::write(&path, "# open").unwrap();

        let sidebar = workspace.read_with(cx, |workspace, _| workspace.sidebar.clone());
        sidebar.update(cx, |sidebar, cx| sidebar.open(path.clone(), cx));

        cx.update_window(window.into(), |_, window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.handle_request(
                    SidebarRequest::DeleteFile { path: path.clone() },
                    window,
                    cx,
                );
            });
        })
        .unwrap();

        assert!(path.exists(), "the file must survive");
        assert!(
            !has_dialog(window, cx),
            "and no delete prompt should even be offered"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Forgetting a path drops it and leaves the rest; forgetting a path that
    /// was never recorded changes nothing.
    ///
    /// The delete dialog calls this once the file is really gone, and
    /// `RecentFiles::remove` reports nothing, so the only way to tell the two
    /// cases apart is by counting — which is what this pins down.
    #[gpui_kit::test]
    fn forgetting_a_recent_removes_only_that_entry(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);
        let dir = scratch("forget");
        let kept = dir.join("kept.md");
        let gone = dir.join("gone.md");
        std::fs::write(&kept, "# kept").unwrap();
        std::fs::write(&gone, "# gone").unwrap();

        let sidebar = workspace.read_with(cx, |workspace, _| workspace.sidebar.clone());
        sidebar.update(cx, |sidebar, cx| sidebar.open(kept.clone(), cx));
        sidebar.update(cx, |sidebar, cx| sidebar.open(gone.clone(), cx));

        cx.update_window(window.into(), |_, window, cx| {
            workspace.update(cx, |workspace, cx| workspace.forget(&kept, window, cx));
        })
        .unwrap();
        assert_eq!(shown_recents(cx, &workspace), vec![gone.clone()]);

        let absent = dir.join("absent.md");
        cx.update_window(window.into(), |_, window, cx| {
            workspace.update(cx, |workspace, cx| workspace.forget(&absent, window, cx));
        })
        .unwrap();
        assert_eq!(
            shown_recents(cx, &workspace),
            vec![gone],
            "a path that was never in the list is not a reason to touch it"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// A file that no tab is showing still has to be confirmed before it goes.
    #[gpui_kit::test]
    fn deleting_a_closed_file_asks_before_removing_it(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);
        let dir = scratch("delete-closed");
        let path = dir.join("closed.md");
        std::fs::write(&path, "# closed").unwrap();

        cx.update_window(window.into(), |_, window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.handle_request(
                    SidebarRequest::DeleteFile { path: path.clone() },
                    window,
                    cx,
                );
            });
        })
        .unwrap();

        assert!(has_dialog(window, cx), "a confirmation should be on screen");
        assert!(
            path.exists(),
            "nothing may be removed before the confirmation is answered"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The prompt adapts to what it is naming, and starts a rename from the
    /// current name rather than an empty field.
    #[test]
    fn the_rename_prompt_starts_from_the_current_name() {
        let rename = NameTarget::Rename {
            path: PathBuf::from("/tmp/note.md"),
        };
        assert_eq!(rename.initial_name(), "note.md");
        assert_eq!(rename.ok_text(), "Rename");
        assert_eq!(rename.caption(), "in /tmp");

        let create = NameTarget::NewFile {
            directory: PathBuf::from("/tmp"),
        };
        assert_eq!(create.initial_name(), "");
        assert_eq!(create.ok_text(), "Create");
        assert_eq!(create.title(), "New file");
    }

    /// The text the active tab's editor holds, read back through the entity.
    fn active_tab_text(workspace: &Entity<Workspace>, cx: &TestAppContext) -> String {
        workspace.read_with(cx, |workspace, cx| {
            workspace.tabs[workspace.active].read(cx).text(cx)
        })
    }

    fn active_tab_is_dirty(workspace: &Entity<Workspace>, cx: &TestAppContext) -> bool {
        workspace.read_with(cx, |workspace, cx| {
            workspace.tabs[workspace.active].read(cx).is_dirty()
        })
    }

    /// `Cmd+S` puts what is on screen into the file, and the tab stops being
    /// dirty once it lands.
    #[gpui_kit::test]
    fn saving_writes_the_editors_text_and_clears_the_dirty_dot(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);
        let dir = scratch("save");
        let path = dir.join("note.md");
        std::fs::write(&path, "# old\n").unwrap();

        let sidebar = workspace.read_with(cx, |workspace, _| workspace.sidebar.clone());
        sidebar.update(cx, |sidebar, cx| sidebar.open(path.clone(), cx));
        assert!(
            !active_tab_is_dirty(&workspace, cx),
            "a fresh open is clean"
        );

        type_into_active_tab(window, &workspace, "\n# new", cx);
        assert!(active_tab_is_dirty(&workspace, cx), "the typing lands");

        cx.update_window(window.into(), |_, window, cx| {
            workspace.update(cx, |workspace, cx| workspace.save_active(window, cx));
        })
        .unwrap();

        // Compared against the editor rather than a hard-coded string: where
        // the caret sat when the typing started is not what this is testing.
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            active_tab_text(&workspace, cx),
            "the bytes on disk must be the ones on screen"
        );
        assert!(
            !active_tab_is_dirty(&workspace, cx),
            "the dirty dot goes out"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// An untitled buffer has no file to write to, so Cmd+S asks for a name
    /// instead of failing.
    #[gpui_kit::test]
    fn saving_an_untitled_tab_asks_for_a_name(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);
        type_into_active_tab(window, &workspace, "\n# draft", cx);

        cx.update_window(window.into(), |_, window, cx| {
            workspace.update(cx, |workspace, cx| workspace.save_active(window, cx));
        })
        .unwrap();

        assert!(
            has_dialog(window, cx),
            "a buffer with no name must be asked for one"
        );
        assert!(
            active_tab_is_dirty(&workspace, cx),
            "and nothing may have been written in the meantime"
        );
    }

    /// Save-as to a name nothing is using writes the file and moves the tab
    /// onto it, so the next Cmd+S writes there rather than asking again.
    #[gpui_kit::test]
    fn save_as_creates_the_file_and_retargets_the_tab(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);
        let dir = scratch("save-as-new");

        type_into_active_tab(window, &workspace, "\n# draft", cx);
        let tab = workspace.read_with(cx, |workspace, _| workspace.tabs[0].clone());

        let outcome = apply(
            window,
            &workspace,
            &NameTarget::SaveAs {
                tab: tab.clone(),
                directory: dir.clone(),
                suggested: String::new(),
            },
            "draft",
            cx,
        );
        assert_eq!(outcome, NameOutcome::Done);

        let written = dir.join("draft.md");
        assert_eq!(
            std::fs::read_to_string(&written).unwrap(),
            active_tab_text(&workspace, cx)
        );
        assert_eq!(tab_path(&workspace, 0, cx), Some(written));
        assert!(!tab.read_with(cx, |tab, _| tab.is_dirty()));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// A name that is already taken is asked about before it is written over.
    /// The first press writes nothing; only answering it does.
    #[gpui_kit::test]
    fn save_as_asks_before_replacing_a_file_that_is_already_there(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);
        let dir = scratch("save-as-existing");
        let taken = dir.join("taken.md");
        std::fs::write(&taken, "# original").unwrap();

        let tab = workspace.read_with(cx, |workspace, _| workspace.tabs[0].clone());
        let target = NameTarget::SaveAs {
            tab,
            directory: dir.clone(),
            suggested: String::new(),
        };

        let outcome = apply(window, &workspace, &target, "taken", cx);
        let NameOutcome::Replace { path, .. } = outcome else {
            panic!("the first press has to ask, got {outcome:?}");
        };
        assert_eq!(path, taken);
        assert_eq!(
            std::fs::read_to_string(&taken).unwrap(),
            "# original",
            "nothing may be written before the question is answered"
        );

        // Answering yes is the same press again, carrying the path the form was
        // told about. A different path would be a different question.
        let outcome = apply_confirmed(window, &workspace, &target, "taken", Some(&path), cx);
        assert_eq!(outcome, NameOutcome::Done);
        assert_eq!(
            std::fs::read_to_string(&taken).unwrap(),
            active_tab_text(&workspace, cx),
            "and now it really is written over"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Save-as onto a file a second tab is showing is refused rather than
    /// confirmed: two tabs over one file means each save silently undoes the
    /// other, and agreeing to that would not make it less true.
    #[gpui_kit::test]
    fn save_as_onto_a_file_another_tab_is_showing_is_refused(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);
        let dir = scratch("save-as-open");
        let taken = dir.join("shared.md");
        std::fs::write(&taken, "# shared").unwrap();

        // The tree opens the file in a second tab, which becomes the active one.
        let sidebar = workspace.read_with(cx, |workspace, _| workspace.sidebar.clone());
        sidebar.update(cx, |sidebar, cx| sidebar.open(taken.clone(), cx));
        assert_eq!(tab_path(&workspace, 1, cx), Some(taken.clone()));

        // The untitled first tab is the one being saved under that name.
        let tab = workspace.read_with(cx, |workspace, _| workspace.tabs[0].clone());
        let outcome = apply(
            window,
            &workspace,
            &NameTarget::SaveAs {
                tab,
                directory: dir.clone(),
                suggested: String::new(),
            },
            "shared",
            cx,
        );

        let NameOutcome::Refused(message) = outcome else {
            panic!("two tabs over one file must be refused, got {outcome:?}");
        };
        assert!(message.contains("another tab"), "got {message}");
        assert_eq!(
            std::fs::read_to_string(&taken).unwrap(),
            "# shared",
            "the other tab's file must be untouched"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// `Cmd+Shift+E` asks the platform for a path and writes the page there.
    ///
    /// This drives the whole round trip rather than the write alone: the
    /// platform prompt is stubbed by the test platform, and the answer is fed
    /// back the way a real save panel would. What ships is the await, so a test
    /// that skipped it would be testing something the app never does.
    #[gpui_kit::test]
    fn exporting_writes_an_html_page_where_the_save_panel_says(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);
        let dir = scratch("export");
        let source = dir.join("note.md");
        std::fs::write(&source, "# Heading\n\n| a | b |\n| - | - |\n| 1 | 2 |\n").unwrap();

        let sidebar = workspace.read_with(cx, |workspace, _| workspace.sidebar.clone());
        sidebar.update(cx, |sidebar, cx| sidebar.open(source.clone(), cx));

        dispatch::<ExportHtml>(window, ExportHtml, cx);
        assert!(
            cx.did_prompt_for_new_path(),
            "export has to ask where to put the file"
        );

        let exported = dir.join("note.html");
        cx.simulate_new_path_selection(|_| Some(exported.clone()));
        cx.run_until_parked();

        let page = std::fs::read_to_string(&exported).unwrap();
        assert!(page.contains("<h1>Heading</h1>"), "{page}");
        // The export uses the same parser as the preview, so a GFM table has
        // to arrive as a table rather than as literal pipes.
        assert!(page.contains("<table>"), "{page}");
        assert!(page.contains("<title>note.md</title>"), "{page}");

        // Exporting is not saving: the Markdown file is left exactly as it was.
        assert_eq!(
            std::fs::read_to_string(&source).unwrap(),
            "# Heading\n\n| a | b |\n| - | - |\n| 1 | 2 |\n"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The save panel opens in the document's own folder: the common case is
    /// an export landing beside the file it came from, so that is where the
    /// dialog should already be looking.
    #[gpui_kit::test]
    fn export_offers_the_documents_folder(cx: &mut TestAppContext) {
        let (window, workspace) = open_workspace(cx);
        let dir = scratch("export-name");
        let source = dir.join("notes.md");
        std::fs::write(&source, "# Notes").unwrap();

        let sidebar = workspace.read_with(cx, |workspace, _| workspace.sidebar.clone());
        sidebar.update(cx, |sidebar, cx| sidebar.open(source, cx));

        dispatch::<ExportHtml>(window, ExportHtml, cx);
        assert!(cx.did_prompt_for_new_path());

        // Cancelling writes nothing and leaves the workspace as it was.
        cx.simulate_new_path_selection(|offered| {
            assert_eq!(offered, dir.as_path(), "the panel starts where the file is");
            None
        });
        cx.run_until_parked();
        assert!(
            !dir.join("notes.html").exists(),
            "cancelling must not write a file"
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
