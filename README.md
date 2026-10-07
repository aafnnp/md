# md

A desktop Markdown editor built with [GPUI](https://gpui.rs/), the GPU-accelerated Rust UI
framework from Zed, using the [gpui-kit](https://github.com/longbridge/gpui-kit) component library.

Two panes: write Markdown on the left, see it rendered on the right, live as you type.

> **Status: early.** M0–M5 are done, apart from clickable task-list checkboxes and synchronised
> scrolling (see [Known gaps](#known-gaps)). The editor opens, edits, finds, renders, saves and
> exports Markdown in two panes, keeps tabs and a file tree alongside a list of recently opened
> files, resolves images relative to the document, and remembers its theme. Tagged pushes are
> packaged for macOS, Windows and Linux — see [Releases](#releases).

## Stack

| Layer | Choice | Why |
|---|---|---|
| UI framework | `gpui-pre` 0.3.8, via `gpui-kit` 0.7 | Zed's GPU-accelerated framework |
| Components | `gpui-kit` 0.7 | Apache-2.0 component library on top of GPUI |
| Markdown | `markdown` (wooorm/markdown-rs) | CommonMark + GFM, same parser for preview and export |

### One dependency rule, and it matters

**Depend on `gpui-kit`, never on `gpui` directly.**

`gpui-kit` pins GPUI to its own snapshot: `gpui = { package = "gpui-pre", version = "=0.3.8" }`.
Upstream `gpui` on crates.io is a *different, incompatible* crate that lags roughly a year behind.
Adding both puts two copies of the framework's types in the build, and nothing that crosses the
boundary between them will compile.

`Cargo.lock` is committed for the same reason: a GPUI bump is a deliberate change, not something
a fresh `cargo update` should decide.

`md-app` is the only crate allowed to touch GPUI. All logic lives in `md-core`, which has no GPUI
dependency at all — so its tests run in CI without Xcode, Metal, or a display server.

## Building

Requires a Rust toolchain (1.99+, pinned in `rust-toolchain.toml`) and, on macOS, **full Xcode with
the Metal toolchain**. Command Line Tools alone is not enough — GPUI compiles Metal shaders at build
time.

```sh
xcode-select -p                      # must be /Applications/Xcode.app/Contents/Developer
xcrun --find metal                   # must resolve
xcodebuild -downloadComponent MetalToolchain   # macOS 26+: separate download
```

Then:

```sh
cargo test -p md-core                # pure logic tests, no Xcode needed
cargo run -p md-app                  # opens the window
```

## Saving

<kbd>Cmd</kbd>+<kbd>S</kbd> writes the active tab. A buffer that has never been saved has no file to
write to, so it gets the save-as prompt instead of an error. <kbd>Cmd</kbd>+<kbd>Shift</kbd>+<kbd>S</kbd>
always asks: the field starts on the current name, in the file's own folder, falling back to the
folder the tree is open on.

A name that is already taken is asked about twice — the button changes from *Save* to *Replace*, and
only the second press writes. Editing the name withdraws that agreement, so the question is always
about the path that was actually named. Saving onto a file that a *second tab* is showing is refused
outright rather than confirmed: two tabs over one file means each save silently undoes the other, and
agreeing to it would not make that less true. Closing that tab, or saving under another name, is the
way forward.

A tab's dirty dot clears when its text reaches the disk, and the tab is retargeted by save-as, so the
next <kbd>Cmd</kbd>+<kbd>S</kbd> writes there rather than asking again.

## Opening files

<kbd>Cmd</kbd>+<kbd>O</kbd> asks the platform for a file and opens it in a tab. It reaches everything
the sidebar's tree cannot: any file outside the folder that tree is rooted at, and every file at all
before a folder has been opened.

When no folder is open, the sidebar shows the files opened recently instead, newest first, each row
naming the folder it sits in — the list is mostly the same few names, and `notes.md` in two projects
would otherwise be two identical rows. Opening a folder puts the tree back in its place.

Choosing a recent file that has since moved or been deleted says why rather than doing nothing, and
drops the entry: one that no longer resolves would otherwise be offered every time the sidebar was
drawn. Renaming or deleting a file from the tree keeps the list in step, so it never points at where
a file used to be.

The list is `recent.json` in the configuration directory, beside the settings file, and holds the
twenty most recent paths. Opening a file that is already open still records it — the list is about
what was opened, not what was new.

## Finding and replacing

<kbd>Cmd</kbd>+<kbd>F</kbd> opens a find bar over the source pane, and
<kbd>Cmd</kbd>+<kbd>Shift</kbd>+<kbd>F</kbd> opens it with the replace field already showing. Both
come from the editor component: the state it is built from is set to the library's code-editor mode,
and that mode is what turns searching on, with the shortcuts bound by the library itself. There is
no find-and-replace code in this repository.

## Exporting

<kbd>Cmd</kbd>+<kbd>Shift</kbd>+<kbd>E</kbd> writes the active tab out as a standalone HTML page. It
is the same parser the preview renders with, so a table, a task list or a strikethrough comes out of
the export as the same construct you were looking at — the two cannot drift apart without the parser
changing under both.

The page is a complete document, not a fragment: it carries the UTF-8 declaration (without it a
Chinese document opens as mojibake), a small stylesheet that covers light and dark, and no external
files or network requests. Raw HTML in the source is escaped rather than emitted, so a document that
merely *mentions* `<script>` does not run it when the exported file is opened.

Exporting uses the platform's own save panel rather than the in-app name prompt that save-as uses.
They are asking different questions: saving renames the document you are editing, so it starts in
that file's folder, while an export is a *new* file that usually belongs somewhere else. The
suggested name is the document's own with an `.html` extension — `notes.md` exports to `notes.html`,
and an untitled buffer to `untitled.html`.

## Images

`![](diagram.png)` is resolved against the folder the document is in, so an image next to the note
is found without any path fiddling. A URL that already means something — `https:`, `data:`, `file:`,
an absolute path — is passed through untouched.

Following CommonMark, a URL may be wrapped in `<...>`, and a `#fragment` or `?query` is a position
inside the file rather than part of its name. A percent-encoded name is decoded, since `my%20diagram.png`
is the file `my diagram.png`. An untitled buffer has no folder to be relative to, so nothing is
guessed on its behalf: the document's images resolve when the document has somewhere to live.

## Known gaps

Two things the plan called for are missing, both because the component underneath stops short of
what they need. They are left out rather than shipped as knobs that do nothing.

**Clickable task-list checkboxes.** `- [x]` renders as a checkbox, but clicking it does nothing.
`gpui-base` draws that checkbox as a static `div` with no id, no click handler and no hook
(`text/node.rs`, in `render_list_item`); `on_link_click` is the only interactive callback a
`TextView` has. Making it clickable means replacing the built-in list rendering with a custom block
plugin that redraws every item, which would look different from every other list in the document and
would stop tracking future fixes upstream. Writing back to the source is a further piece of work on
top of that.

**Synchronised scrolling.** The preview half is reachable: `TextViewState::list_state()` reports the
current and maximum scroll, so a position can be read and a position can be set. The source half is
not. `EditorState` can be *told* and *told about* a scroll offset (`scroll_offset`,
`set_scroll_offset`), but nothing public reports how far it can scroll, and nothing public reports
that it scrolled at all — `InputBaseState::on_scroll_wheel` is `pub(super)`. A ratio needs both ends,
and the one hook that is reachable, `InteractiveElement::on_scroll_wheel`, sees wheel events only: a
link built on it would quietly come apart the moment either pane was scrolled by keyboard. Left out
rather than shipped as half a link, and revivable — if a later `gpui-kit` exposes the editor's scroll
extent, this is a subscription and a division away.

## Settings

The theme button at the right of the tab strip cycles **System → Light → Dark**. On *System* the
window follows the operating system and keeps following it when it changes; a light or dark choice
holds until it is changed back. <kbd>Cmd</kbd>+<kbd>Shift</kbd>+<kbd>T</kbd> does the same.

<kbd>Cmd</kbd>+<kbd>,</kbd> — or the settings button at the right of the status bar — opens a panel
over the window holding every setting described below, and each one applies the moment it is chosen.
The four font settings are dropdowns. The families are the ones the system actually has, asked of the
text system as the panel opens; the sizes are every whole point from 8 to 48, the range the file
below accepts. Both are searchable, because several hundred families is not a list to scroll. Once
something is chosen an ✕ appears beside it, and clearing it is how you ask for the theme's own font
or size back — the same thing a missing key in the file means.

The source column's width and the preview's padding stay typed: any value in their ranges is a real
choice, and no list of them would help. Those two apply the moment their text is usable, so a number
typed digit by digit takes effect at `1` and then again at `18`, and the panel says nothing in
between — `1` is a step on the way to a number, not a mistake. Text that never becomes usable, `abc`
or a width well outside the range, is refused on the way out of the field with the range spelled out,
and the setting keeps the value it had. Nothing is silently clamped: a number you did not ask for is
worse than one you are told about.

A dropdown can also be showing you a value it does not normally offer. A file may hold a size of
`13.5`, or the name of a font that has since been uninstalled, and the app has been honouring it —
so it is listed, in its place in the order, rather than dropped. Opening the panel never quietly
changes a setting that was already in force.

Everything else is a small JSON file in the platform's per-user configuration directory, which is
`~/Library/Application Support/dev.aafnnp.md/settings.json` on macOS. It is read once, at startup:

```json
{
  "theme": "system",
  "font_family": "Helvetica Neue",
  "font_size": 16.0,
  "mono_font_family": "JetBrains Mono",
  "mono_font_size": 14.0,
  "editor_max_width": 900.0,
  "preview_padding": 24.0
}
```

Every key but `theme` is optional, and leaving one out keeps the built-in default — so a file that
only says `{"theme": "dark"}` is complete. Values out of range are pulled back into range rather
than refused, an unreadable file falls back to the defaults, and unknown keys are ignored. Sizes
outside 8–48 points, widths outside 320–4000 points and padding outside 0–200 points are clamped.

`preview_padding` is the margin the rendered document is set in. The default is 24 points rather
than none: text flush against the pane's edge reads as though it has been cropped, and the preview
is the one pane with nothing else to hold it off its border.

The file is written when the theme changes, and that write is the whole of what the app knows: a file
it could not parse was loaded as the defaults, and a key this build does not recognise was dropped on
the way in. So the next theme change replaces the file with one holding just the settings this build
understands. A hand-edit that broke the JSON is therefore not preserved for you to fix — it is
overwritten. Keep a copy if anything in it mattered.

## The status bar

Along the bottom of the window: how much the active document holds, on the left, and the settings
button on the right.

The count reads `245 characters · 12 lines`, and it is taken from the editor's buffer rather than
from a copy made when the file was opened — so it keeps up as you type, and re-counts the document
that comes forward when you switch tabs. With every tab closed it says nothing at all rather than
`0 characters`: a document that does not exist has no size, and a zero would suggest one that is
merely empty.

Characters are counted as Unicode scalar values, which is what `chars()` gives. Not bytes, which
would report three for `中`, and not grapheme clusters, which need a Unicode property table to get
right. The one thing to know is that an emoji built from a base and a modifier counts as two.

There is deliberately no word count. Splitting on whitespace reports `一篇文章` as one word, so the
number would be badly wrong for exactly the documents this editor is built to write, and a number
that is wrong in a way the reader cannot see is worse than no number.

## Continuous integration

`.github/workflows/ci.yml` runs on every push to `master` and every pull request:

- **rustfmt** — `cargo fmt --all -- --check`, on Linux alone, since formatting needs no compiler.
- **clippy** — `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- **test** — `cargo test --workspace --locked` on Linux, macOS and Windows, with `fail-fast` off so
  one platform failing does not hide the other two.
- **changelog** — `scripts/changelog.sh --current`, which fails if the version in `Cargo.toml` has
  no section in `CHANGELOG.md`. The release workflow runs the same script for the tag it is
  publishing, so a version cannot be released without one either.

`--locked` everywhere, because `Cargo.lock` is committed and a GPUI bump is meant to be a deliberate
change rather than something a fresh resolve decides. There is deliberately no `build` job: `cargo
test` already compiles every crate on every platform, and a CI runner has no display server and no
GPU, so the app cannot be launched there either way. Release-only concerns — thin LTO under a
memory-capped runner, Windows needing `fxc.exe` on `PATH` — belong to the release workflow, where
they will actually surface.

Linux builds are pinned to `ubuntu-22.04` (glibc 2.35) rather than `ubuntu-latest` (24.04, glibc
2.39): a binary built against the newer glibc will not run on Debian 12 or Ubuntu 22.04.

## Releases

`.github/workflows/release.yml` runs on a `v*` tag. Four runners build in parallel:

| Runner | Package |
|---|---|
| `macos-latest` (Apple silicon) | `md_0.1.0_aarch64.dmg` |
| `macos-15-intel` (Intel) | `md_0.1.0_x64.dmg` |
| `ubuntu-22.04` | `md_0.1.0_amd64.deb`, `md_0.1.0_x86_64.AppImage` |
| `windows-latest` | `md_0.1.0_x64-setup.exe` |

Each is a tagged commit built with `--locked`, packaged by a pinned `cargo-packager 0.11.8`, and
attached to a **draft** release. Nothing is public until someone reads that page and publishes it,
which is the one step here that is deliberately not automatic.

The release notes are the tagged version's section of `CHANGELOG.md`, lifted out by
`scripts/changelog.sh` and handed to the release step as the body. They are written by a person
rather than generated from the commits, because the history here is prose — `Refuse a rooted image
URL on Windows too`, not `fix: reject rooted paths` — and there are no pull requests or labels to
group. A tag whose version has no section fails the job before the release is created, so the page
can never come out empty. [CHANGELOG.md](CHANGELOG.md) ends with the steps for cutting a release.

Two macOS builds rather than one universal binary: a universal build needs a `lipo` merge step on
top of two compiles, and shipping both architectures is a smaller thing to get right. The macOS
package is a `.dmg` and the Windows one is an NSIS installer — the `.app` bundle itself is not
published separately, because `upload-artifact` does not preserve the file modes and symlinks
inside one and it would arrive unsigned. The `.dmg` wraps the same bundle.

The workflow also takes `workflow_dispatch`, which runs the whole matrix without publishing
anything — useful for changing this file without spending a version number on finding out whether
it worked. The publish job is gated on the ref actually being a tag.

Packaging is configured in `crates/md-app/Cargo.toml` under `[package.metadata.packager]` rather
than in a `Packager.toml` beside it, because the Cargo-metadata path is the one that fills in the
version, the binary directory and the output directory from the workspace. Paths in that config are
read relative to *that file's* directory — `cargo packager` changes directory to it before
packaging — which is why the `icons` list there climbs back out with `../..`:

```sh
cargo build --release --locked -p md-app
cargo packager --release --formats dmg
```

Two things about that config are worth knowing before editing it, because neither failure announces
itself. A pattern in `icons` that matches nothing is not an error — it is silently no icon at all,
and the app ships with a blank one. And an `.icns` cannot hold a 1024-pixel image under its own
name: at that size the format has only "512 at 2x", so the file has to be called `icon@2x.png`. The
name is how the density is communicated, and a 1024 named any other way is refused outright.

`assets/make-icon.py` draws every one of those, needing nothing but the standard library. Each size
is rasterised at its own resolution rather than scaled down from one master, because the 16- and
32-pixel entries fall back to a simpler mark — the M is an illegible smudge at that size — and a
downsampled copy would never reach the branch that knows it. It writes `assets/icon.ico` as well,
since nothing here generates one, and that is what the NSIS installer wears. All of it is checked
in, so a build never has to run Python.

## Installing a release

Release builds are **unsigned**. That has real consequences:

- **macOS** — Gatekeeper will refuse the first launch. After the build is ad-hoc signed, the dialog
  says the developer cannot be verified, and you can still open it from
  System Settings → Privacy & Security → *Open Anyway*. If it instead says the app is damaged:
  ```sh
  xattr -cr /Applications/md.app
  ```
- **Windows** — SmartScreen warns on first run. *More info* → *Run anyway*.
- **Linux** — no signing involved; install the `.deb` or run the `.AppImage`.

## Layout

```
crates/
  md-core/     pure logic: documents, file I/O, settings, the recent list, images, export — no GPUI
  md-app/      the `md` binary: GPUI views and layout, and the packaging config
assets/
  make-icon.py  draws every icon below; only the standard library
  icon-16.png   also icon-32, icon-128 and icon-256 — one per size an `.icns`
  icon.png      the 512, and the one the Linux packagers take as the app icon
  icon@2x.png   the 1024, which an `.icns` holds only as 512 at 2x
  icon.ico      used as-is — nothing here generates one
scripts/
  changelog.sh  lifts one version's section out of the changelog
CHANGELOG.md   what each version changed — the release notes, not a summary of them
.github/workflows/
  ci.yml       fmt, clippy and tests on every push and pull request
  release.yml  the four-platform package build, on a version tag
```

## Roadmap

- [x] **M0** — toolchain gate, workspace, GPUI smoke test
- [x] **M1** — document core, two-pane layout, live preview with debounce
- [x] **M2** — tabs, file tree sidebar, unsaved-close confirmation
- [x] **M3** — light/dark themes, persisted settings
- [x] **M4** — GFM tables, images, HTML export
- [x] **M5** — packaging and tag-triggered multi-platform release
- [ ] **M4** — clickable task-list checkboxes (see [Known gaps](#known-gaps)), and inserting
      `![]()` by dropping an image onto the editor
- [x] **Save** — <kbd>Cmd</kbd>+<kbd>S</kbd> / <kbd>Cmd</kbd>+<kbd>Shift</kbd>+<kbd>S</kbd>, with
      an overwrite confirmation and a guard against two tabs over one file
- [x] **Find and replace** — <kbd>Cmd</kbd>+<kbd>F</kbd> and
      <kbd>Cmd</kbd>+<kbd>Shift</kbd>+<kbd>F</kbd>, from the editor component's own search
- [x] **Open file and recent files** — <kbd>Cmd</kbd>+<kbd>O</kbd>, and the recent list the sidebar
      shows in place of the tree while no folder is open
- [x] **A settings panel and a status bar** — <kbd>Cmd</kbd>+<kbd>,</kbd> opens a panel over every
      setting the file holds, the preview gains a margin, and the size of the active document is
      reported along the bottom of the window
- [ ] **Next** — a native menu bar, and synchronised scrolling if the editor's scroll extent ever
      becomes reachable (see [Known gaps](#known-gaps))

## License

Dual-licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.

Note that Zed's `editor`, `markdown`, `ui`, and `theme` crates are GPL-3.0 and not published, so
they are deliberately not used here. Only `gpui` itself and `gpui-kit` are, both Apache-2.0.
