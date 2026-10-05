# md

A desktop Markdown editor built with [GPUI](https://gpui.rs/), the GPU-accelerated Rust UI
framework from Zed, using the [gpui-kit](https://github.com/longbridge/gpui-kit) component library.

Two panes: write Markdown on the left, see it rendered on the right, live as you type.

> **Status: early.** M0 (toolchain and skeleton) is done. The editor is not usable yet —
> see [Roadmap](#roadmap).

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
- [ ] **M1** — document core, two-pane layout, live preview with debounce
- [ ] **M2** — tabs, file tree sidebar, unsaved-close confirmation
- [ ] **M3** — light/dark themes, persisted settings
- [ ] **M4** — GFM tables, images, task lists, HTML export
- [ ] **M5** — packaging and tag-triggered multi-platform release

## License

Dual-licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.

Note that Zed's `editor`, `markdown`, `ui`, and `theme` crates are GPL-3.0 and not published, so
they are deliberately not used here. Only `gpui` itself and `gpui-kit` are, both Apache-2.0.
