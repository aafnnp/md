# md

A desktop Markdown editor built with [GPUI](https://gpui.rs/), the GPU-accelerated Rust UI
framework from Zed, using the [gpui-kit](https://github.com/longbridge/gpui-kit) component library.

Two panes: write Markdown on the left, see it rendered on the right, live as you type.

> **Status: early.** M0–M3 are done. The editor opens, edits and renders Markdown in two panes,
> keeps tabs and a file tree, and remembers its theme. Export is still missing — see
> [Roadmap](#roadmap).

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

## Settings

The theme button at the right of the tab strip cycles **System → Light → Dark**. On *System* the
window follows the operating system and keeps following it when it changes; a light or dark choice
holds until it is changed back. <kbd>Cmd</kbd>+<kbd>Shift</kbd>+<kbd>T</kbd> does the same.

Everything else is a small JSON file in the platform's per-user configuration directory, which is
`~/Library/Application Support/dev.aafnnp.md/settings.json` on macOS. It is read once, at startup:

```json
{
  "theme": "system",
  "font_family": "Helvetica Neue",
  "font_size": 16.0,
  "mono_font_family": "JetBrains Mono",
  "mono_font_size": 14.0,
  "editor_max_width": 900.0
}
```

Every key but `theme` is optional, and leaving one out keeps the built-in default — so a file that
only says `{"theme": "dark"}` is complete. Values out of range are pulled back into range rather
than refused, an unreadable file falls back to the defaults, and unknown keys are ignored. Sizes
outside 8–48 points and widths outside 320–4000 points are clamped. The app writes the file when
the theme changes; it never rewrites a file it could not parse, so a mistake there is yours to fix
rather than one the app silently erases.

Setting the theme changes `"theme"` in the file, and writing it drops any key this build does not
know about.

**Not implemented: synchronised scrolling.** The plan called for a preview scroll ratio, but
`gpui-base`'s `TextViewState` keeps its scroll offset private (`scroll_offset` is `pub(super)`) and
offers no scroll handle, and `EditorState` exposes none at all. The two panes cannot be linked
without forking the component, so the setting is left out rather than shipped as a knob that does
nothing.

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
  md-core/     pure logic: documents, file I/O, settings, export — no GPUI
  md-app/      the `md` binary: GPUI views and layout
```

## Roadmap

- [x] **M0** — toolchain gate, workspace, GPUI smoke test
- [x] **M1** — document core, two-pane layout, live preview with debounce
- [x] **M2** — tabs, file tree sidebar, unsaved-close confirmation
- [x] **M3** — light/dark themes, persisted settings
- [ ] **M4** — GFM tables, images, task lists, HTML export
- [ ] **M5** — packaging and tag-triggered multi-platform release
- [ ] **Next** — save and save-as (there is no <kbd>Cmd</kbd>+<kbd>S</kbd> yet, so a dirty tab can
      only be discarded), a settings panel instead of a hand-edited file, open file… alongside open
      folder…, a native menu bar, and using `md-core`'s recent-files list

## License

Dual-licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.

Note that Zed's `editor`, `markdown`, `ui`, and `theme` crates are GPL-3.0 and not published, so
they are deliberately not used here. Only `gpui` itself and `gpui-kit` are, both Apache-2.0.
