# Changelog

Notable changes to `md`, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to follow
[semantic versioning](https://semver.org/spec/v2.0.0.html) from `0.1.0` onward.

A version's section here is not a summary written after the fact — it is the release notes.
`scripts/changelog.sh` lifts the section for the tag being pushed, and the release job refuses to
publish a version that has no section. See [Releasing](#releasing) at the bottom.

## [Unreleased]

### Added

- **Open file.** <kbd>Cmd</kbd>+<kbd>O</kbd> picks a file through the platform's dialog, for the
  ones the sidebar's tree cannot reach: anything outside the folder it is rooted at, and everything
  at all before a folder has been opened.
- **Recently opened files.** With no folder open, the sidebar lists the twenty documents opened most
  recently, newest first, each row naming the folder it sits in. Choosing one that has since moved or
  been deleted says why and drops the entry, and renaming or deleting a file from the tree keeps the
  list pointing at where the file actually is. The list is `recent.json`, beside `settings.json` in
  the configuration directory.
- **Find and replace.** <kbd>Cmd</kbd>+<kbd>F</kbd> and
  <kbd>Cmd</kbd>+<kbd>Shift</kbd>+<kbd>F</kbd> over the source pane, from the editor component's
  built-in search.
- **Layouts for self-media.** A document can be laid out for **微信公众号**, **今日头条**,
  **小红书** or **知乎**, and then exported as a page or copied to the clipboard as rich text ready
  to paste into that platform's editor. The four are not themes: each is the same document rendered
  the way its destination will actually accept it — everything inline, in pixels and hex, with no
  stylesheet, no classes and no elements the platform's own editor would drop on the way in. The
  layout is chosen from the status bar, remembered in the settings file, and it applies to the export
  and the copy alone; the preview beside the source is the same renderer it always was. With no layout
  chosen, nothing about either is different from before. **Relative images are inlined as base64 data
  URIs** so a paste carries its own pictures, and any that could not be read are named rather than
  silently left behind.
- **Typography styles.** A layout says how a document is *written*; a style says what it *looks
  like* when it arrives. **默认 / 简约 / 杂志** scale the type, set the leading, recolour the text and
  decide what a heading is decorated with — and nothing else. A style never changes a tag name,
  whether a `class` survives, or where anything sits: those are the platform's cleaning rules, and a
  style that reached them would quietly undo the layout it was meant to dress. **默认 is
  byte-for-byte what the layouts produced before styles existed** — it is the absence of a style
  rather than a fourth set of numbers. The style is chosen from the status bar, beside the layout,
  and remembered in the settings file.
- **Copy the document as rich text.** <kbd>Cmd</kbd>+<kbd>Shift</kbd>+<kbd>C</kbd>, or the **Copy**
  button in the status bar, puts the laid-out document on the system clipboard as HTML, with its plain
  text alongside it for anywhere that cannot take the rich form. This is the path a self-media editor
  wants: pasting the Markdown would arrive as literal punctuation, and pasting the exported page would
  arrive as one long paragraph, because its styling lives in a `<head>` those editors discard.
- **A settings panel.** <kbd>Cmd</kbd>+<kbd>,</kbd>, or the settings button at the right of the
  status bar, edits every setting the file holds — theme, the layout above and its style, both font
  families and sizes, the source column's width, the preview's padding and whether the preview
  follows the source pane. Each one applies the moment it is chosen, and the two
  fields that are still typed refuse text that does not mean a usable number, with the range spelled
  out, rather than clamping it to something you did not ask for.
- **A status bar.** Along the bottom of the window: how much the active document holds, and the
  button that opens the settings above.
- **New documents.** <kbd>Cmd</kbd>+<kbd>N</kbd>, the <kbd>+</kbd> at the right of the tab strip, or
  **New file** in the middle of the window once every tab has closed. What it makes is a buffer in
  memory: nothing touches the disk until <kbd>Cmd</kbd>+<kbd>S</kbd>, which asks for the name and
  the folder then. So a new document needs no folder open, and it starts clean, which means
  <kbd>Cmd</kbd>+<kbd>W</kbd> closes it without asking about changes nobody made. Untitled buffers
  are numbered — `Untitled`, `Untitled 2`, `Untitled 3` — by the lowest number not in use, so
  closing `Untitled 2` and starting another gives `2` back, and saving retires the number so a new
  buffer cannot take one a named file is still holding.

### Fixed

- **The source and the preview did not scroll together.** The two panes were independent, so reading
  a long document meant moving both by hand and losing your place in one of them. Scrolling the
  source now carries the preview with it. The follow is **proportional** rather than line for line:
  the editor does not publish how far it can scroll — its scroll extent and scroll handle are
  `pub(crate)` with no getter — so its range is estimated from the document's line count and is
  matched to the preview's by fraction. Soft-wrapped lines make that estimate run short, so the
  preview reaches the end slightly before the source does. It is one-way, source to preview:
  following in both directions would need a way to tell a scroll the app caused from one you made,
  and without it the panes chase each other and jitter. It can be turned off — see **Sync scroll** in
  the settings panel.
- **The starter tab could not be closed.** A fresh window opened on an `Untitled` buffer that was
  already dirty — its text was seeded into an empty buffer, and the two did not match — so
  <kbd>Cmd</kbd>+<kbd>W</kbd> asked whether to discard edits nobody had made. It is now a scratch
  document, clean from the moment it appears, and it closes like any other tab.
- **Closing the last tab left nothing to start from.** `render_body` returned early when no tab was
  in front, and what it returned early *from* was the whole split — sidebar included. So an empty
  strip took the **Open file** and **Open folder** buttons down with it and left one line of grey
  text, with the only remaining way to create a file being the tree's right-click menu, which needs
  a folder open to exist in the first place. The sidebar is now mounted whether or not a tab is
  open, and the panel beside it names the two ways forward.
- **Closing or switching tabs left the caret behind.** GPUI delivers a key binding through the
  focused element, so a caret left on a tab that had just been closed did not only swallow the
  typing: it stranded every shortcut in the window, <kbd>Cmd</kbd>+<kbd>O</kbd> included, because
  the element that held the focus was no longer rendered. The caret now follows the document that
  comes forward, and the workspace can hold it itself when the last tab closes.

### Changed

- **The preview has a margin.** Rendered text used to sit flush against the pane edge, which reads
  as though it has been cropped. The default is 24 points, and it is one of the settings above.
- **Fonts and sizes are chosen, not typed.** Both font families and both sizes in the settings panel
  were text fields, which asked you to know a name before you could pick it and said nothing when
  the name did not exist — the text system falls back to whatever it has, so a typo looked exactly
  like a font that had not applied. They are dropdowns now, searchable, listing the families the
  system actually reports and every whole point from 8 to 48. A value the list does not hold, such
  as a size of `13.5` edited into the file by hand, is listed in its place rather than dropped, so
  opening the panel cannot change a setting that was already in force.

## [0.1.0] - 2026-10-05

The first release. An editor with two panes — Markdown source on the left, rendered output on the
right, updating as you type — packaged for macOS, Windows and Linux.

### Added

- **The workspace.** A sidebar, an editor pane and a preview pane, separated by draggable
  dividers, with the preview re-rendering after a short pause rather than on every keystroke.
- **Editing.** A source editor with syntax highlighting, and keys for the things you would expect:
  <kbd>Cmd</kbd>+<kbd>S</kbd>, <kbd>Cmd</kbd>+<kbd>Shift</kbd>+<kbd>S</kbd>,
  <kbd>Cmd</kbd>+<kbd>Shift</kbd>+<kbd>E</kbd>, <kbd>Cmd</kbd>+<kbd>W</kbd>,
  <kbd>Cmd</kbd>+<kbd>Q</kbd> and <kbd>Cmd</kbd>+<kbd>1</kbd>–<kbd>Cmd</kbd>+<kbd>9</kbd>.
- **Tabs.** Several documents open at once, each with a dot while it has unsaved changes, and a
  confirmation before closing one that is not saved.
- **Saving.** Save, save-as, and an overwrite prompt that has to be answered twice. Saving over a
  file a second tab is showing is refused outright, because two tabs writing one file means each
  save silently undoes the other.
- **A file tree.** Open a folder, expand it, and create, rename or delete files from it.
- **Markdown.** CommonMark and GFM — tables, task lists, strikethrough, images — parsed by the same
  `markdown` crate in the preview and in an export, so the two cannot disagree.
- **Images.** A relative `![](diagram.png)` resolves against the folder the document is in, with
  percent-encoding, fragments and queries handled, and URLs that already mean something
  (`https:`, `data:`, an absolute path) passed through untouched.
- **HTML export.** A complete standalone page with a stylesheet covering light and dark, an
  explicit UTF-8 declaration, and no network requests. Raw HTML in the source is escaped rather
  than emitted.
- **Themes.** System, light and dark, with fonts, sizes and editor width, persisted as JSON in the
  platform's per-user configuration directory.
- **Packaging.** Tagged pushes build a `.dmg` for Apple silicon and one for Intel, a `.deb` and an
  `.AppImage` for Linux, and an NSIS `.exe` for Windows, and attach all five to a draft release.

### Known limitations

- Builds are unsigned, so macOS and Windows both warn on first launch. See *Installing a release* in
  the README.
- Task-list checkboxes render but cannot be clicked: `gpui-base` draws them as an inert `div`.
- A `.AppImage` built for `x86_64` cannot be started on an Apple silicon machine to check it, so
  that artifact ships having been built and listed but never run.

[Unreleased]: https://github.com/aafnnp/md/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/aafnnp/md/releases/tag/v0.1.0

## Releasing

1. Move what is under `## [Unreleased]` into a new `## [x.y.z] - YYYY-MM-DD` section, and bump
   `version` in the workspace `Cargo.toml`.
2. Update the two compare links at the bottom: the new version points at its tag, and `Unreleased`
   compares from it.
3. Commit, then `git tag vX.Y.Z && git push origin master vX.Y.Z`.

The release job runs `scripts/changelog.sh X.Y.Z` and fails before anything is published if there is
no such section, so step 1 cannot be skipped by accident. CI runs the same script against the
version in `Cargo.toml` on every push, which catches the other order of the same mistake —
bumping the version and forgetting the changelog.
