//! The sidebar: a tree of the Markdown files under a folder the user opens.
//!
//! The tree is a *view* of the filesystem, not a mirror of it. Nothing here
//! mutates a file, and the listing is re-read from disk on every rebuild — so a
//! file created or deleted by another program shows up as soon as the tree is
//! next rebuilt, and never appears to exist when it does not.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::button::Button;
use gpui_kit::component::list::ListItem;
use gpui_kit::component::tree::{self, TreeEvent, TreeItem, TreeState};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::*;

/// What the sidebar asks of whoever owns it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SidebarEvent {
    /// A file row was activated; open it in a tab.
    Open(PathBuf),
}

/// The extensions the tree lists.
///
/// This is a Markdown editor's tree, not a file browser. Listing every file
/// under the folder would mostly offer rows that fail the moment they are
/// clicked, so the listing is narrowed to what the editor can actually open.
const MARKDOWN_EXTENSIONS: [&str; 3] = ["md", "markdown", "mdx"];

/// Left padding of a top-level row, and how much each level of nesting adds.
const ROW_PADDING: f32 = 10.;
const ROW_INDENT: f32 = 14.;

pub struct Sidebar {
    /// The folder the tree is rooted at, once the user has opened one.
    root: Option<PathBuf>,
    tree: Entity<TreeState>,
    /// The directories the user has expanded.
    ///
    /// The tree is rebuilt from disk whenever it changes, and a rebuilt
    /// `TreeItem` starts collapsed, so this has to be remembered here and
    /// re-applied on every rebuild.
    expanded: HashSet<PathBuf>,
    /// Ids of the rows that are directories, as of the last [`Sidebar::rebuild`].
    ///
    /// Rendering has to know a row's kind, and `TreeItem::is_folder` cannot
    /// answer it — that is `!children.is_empty()`, so an empty directory reads
    /// as a file, and a collapsed one reads as a file until it has a child.
    /// Asking the filesystem per row would mean a syscall per visible row per
    /// frame, so the answer is recorded at build time instead. It is shared
    /// through an `Rc` because the render callback is `'static` and re-runs on
    /// every frame.
    directories: Rc<HashSet<String>>,
    _tree_events: Subscription,
}

impl Sidebar {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // Wrapped in a closure rather than passed as `TreeState::new`: `cx.new`
        // wants `&mut Context<Self>` here, and a plain fn item taking `&mut App`
        // does not coerce in argument position.
        let tree = cx.new(|cx| TreeState::new(cx));
        // The kit's tree toggles a folder in its own item tree and then tells
        // us about it. That toggle is not enough on its own: the child rows it
        // reveals are whatever was built last time, so the event is treated as
        // a request to re-read the directory and rebuild from disk.
        let _tree_events = cx.subscribe(&tree, |this, _, event: &TreeEvent, cx| {
            let (id, expanded) = match event {
                TreeEvent::Expanded(id) => (id, true),
                TreeEvent::Collapsed(id) => (id, false),
            };
            let path = PathBuf::from(id.as_ref());
            if expanded {
                this.expanded.insert(path);
            } else {
                this.expanded.remove(&path);
            }
            this.rebuild(cx);
        });

        Self {
            root: None,
            tree,
            expanded: HashSet::new(),
            directories: Rc::new(HashSet::new()),
            _tree_events,
        }
    }

    /// Announce that a file row was activated.
    ///
    /// The row's click handler calls this; the tests drive it directly, since
    /// the alternative is simulating a click at a pixel position the test would
    /// have to guess.
    pub(crate) fn open(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        cx.emit(SidebarEvent::Open(path));
    }

    /// Ask the user for a folder and root the tree at it.
    fn pick_folder(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open folder".into()),
        });
        cx.spawn(async move |this, cx| {
            // A cancelled or failed prompt is not worth reporting: the user
            // either changed their mind, which needs no explanation, or the
            // platform dialog is unavailable, which no dialog of ours can fix.
            let Ok(Ok(Some(mut paths))) = picked.await else {
                return;
            };
            let Some(root) = paths.pop() else {
                return;
            };
            this.update(cx, |sidebar, cx| sidebar.set_root(root, cx))
                .ok();
        })
        .detach();
    }

    /// Root the tree at `root` and show its top level.
    fn set_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.expanded.clear();
        self.expanded.insert(root.clone());
        self.root = Some(root);
        self.rebuild(cx);
    }

    /// Re-read the tree from disk, keeping the expansion state.
    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let mut directories = HashSet::new();
        let items = match &self.root {
            Some(root) => vec![self.build_item(root, &mut directories)],
            None => Vec::new(),
        };
        self.tree.update(cx, |tree, cx| tree.set_items(items, cx));
        self.directories = Rc::new(directories);
        cx.notify();
    }

    /// Build one row, recursing into it when it is an expanded directory.
    fn build_item(&self, path: &Path, directories: &mut HashSet<String>) -> TreeItem {
        let id = path_id(path);
        let item = TreeItem::new(id.clone(), display_name(path));
        if !path.is_dir() {
            return item;
        }
        directories.insert(id.clone());

        let children = self.children_of(path);
        if children.is_empty() {
            // Nothing under it. Deliberately left without children, which makes
            // `TreeItem::is_folder` false and so leaves the row with no
            // disclosure triangle — the row is a leaf, because expanding it
            // would show nothing.
            return item;
        }
        if self.expanded.contains(path) {
            item.children(
                children
                    .iter()
                    .map(|child| self.build_item(child, directories)),
            )
            .expanded(true)
        } else {
            // `TreeItem::is_folder` is `!children.is_empty()`, and the tree's
            // expand-click is a no-op for anything else, so a collapsed
            // directory needs a child to stay expandable. One stand-in is
            // enough: a collapsed row's children are never rendered, and the
            // real listing replaces it the moment the directory is expanded.
            item.child(TreeItem::new(placeholder_id(&id), ""))
        }
    }

    /// The rows directly under `dir`: subdirectories first, then Markdown
    /// files, each group sorted by name.
    fn children_of(&self, dir: &Path) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            // An unreadable directory — a permission it does not have, a
            // directory that vanished mid-walk — is shown as empty rather than
            // reported. There is nothing the user could do about it from here.
            return Vec::new();
        };

        let mut rows = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            // Dotfiles are configuration and version control, not documents.
            if name.starts_with('.') {
                continue;
            }
            // `DirEntry::file_type` does not follow symlinks, so a symlinked
            // directory reports `is_symlink` rather than `is_dir` and is skipped
            // here. That also means a symlink loop cannot send the walk
            // diverging through a directory it has already visited.
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                rows.push((false, path));
            } else if kind.is_file() && is_markdown(&path) {
                rows.push((true, path));
            }
        }

        // Directories first — the shape of the project matters more than its
        // files — then by name, case-insensitively, so the order does not
        // depend on the filesystem's.
        rows.sort_by_cached_key(|(is_file, path)| (*is_file, name_key(path)));
        rows.into_iter().map(|(_, path)| path).collect()
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let name = self
            .root
            .as_ref()
            .map(|root| display_name(root))
            .unwrap_or_else(|| "No folder open".to_string());

        h_flex()
            .flex_shrink_0()
            .items_center()
            .justify_between()
            .gap_2()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(div().truncate().text_sm().child(name))
            .child(
                Button::new("open-folder")
                    .icon(IconName::FolderOpen)
                    .label("Open")
                    .compact()
                    .on_click(cx.listener(|this, _, window, cx| this.pick_folder(window, cx))),
            )
    }

    fn render_tree(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.root.is_none() {
            return div()
                .flex_1()
                .p_4()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Open a folder to see its Markdown files.")
                .into_any_element();
        }

        let sidebar = cx.weak_entity();
        let directories = self.directories.clone();
        let muted = cx.theme().muted_foreground;

        div()
            .flex_1()
            .min_h_0()
            .child(tree::tree(
                &self.tree,
                move |_ix, entry, _selected, _window, _cx| {
                    let id = entry.item().id.clone();
                    let name = entry.item().label.clone();
                    let is_dir = directories.contains(id.as_ref());
                    let icon = match (is_dir, entry.is_expanded()) {
                        (true, true) => IconName::FolderOpen,
                        (true, false) => IconName::Folder,
                        (false, _) => IconName::FileText,
                    };

                    let mut row = ListItem::new(id.clone())
                        .accessibility_label(name.clone())
                        .pl(px(ROW_PADDING + entry.depth() as f32 * ROW_INDENT))
                        .child(
                            h_flex()
                                .gap_1()
                                .items_center()
                                .child(Icon::new(icon).xsmall().text_color(muted))
                                .child(name),
                        );

                    // A directory row is left alone: the tree's own click
                    // handler already toggles it. Only a file row needs a
                    // handler, and only a file row may be opened.
                    if !is_dir {
                        let sidebar = sidebar.clone();
                        row = row.on_click(move |_, _, cx| {
                            let path = PathBuf::from(id.to_string());
                            sidebar
                                .update(cx, |sidebar, cx| sidebar.open(path, cx))
                                .ok();
                        });
                    }
                    row
                },
            ))
            .into_any_element()
    }
}

impl EventEmitter<SidebarEvent> for Sidebar {}

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .border_r_1()
            .border_color(cx.theme().border)
            .child(self.render_header(cx))
            .child(self.render_tree(cx))
    }
}

/// The stable id of the row for `path`. The tree hands it back verbatim in
/// [`TreeEvent`], so it has to survive a round trip through `SharedString`.
fn path_id(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// An id for the stand-in child of a collapsed directory.
///
/// A NUL cannot appear in a path, so this can never collide with a real row —
/// which matters, because a collision would make an expand event name a file.
fn placeholder_id(id: &str) -> String {
    format!("{id}\u{0}")
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn name_key(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            let extension = extension.to_lowercase();
            MARKDOWN_EXTENSIONS.contains(&extension.as_str())
        })
}

#[cfg(test)]
mod tests {
    // Imported narrowly: `use super::*` would drag in the `gpui_kit::*` glob,
    // whose `test` attribute macro shadows the built-in `#[test]`.
    use super::Sidebar;
    use gpui_kit::component::tree::{TreeEvent, TreeItem};
    use gpui_kit::{AppContext as _, Entity, TestAppContext};
    use std::path::{Path, PathBuf};

    /// A scratch directory private to one test, so the tests can run in
    /// parallel without treading on each other's files.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("md-app-sidebar-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    fn sidebar_for(cx: &mut TestAppContext, root: &Path) -> Entity<Sidebar> {
        let sidebar = cx.update(|cx| cx.new(Sidebar::new));
        sidebar.update(cx, |sidebar, cx| sidebar.set_root(root.to_path_buf(), cx));
        sidebar
    }

    /// The root row's children, by label.
    fn child_labels(cx: &TestAppContext, sidebar: &Entity<Sidebar>) -> Vec<String> {
        children_of(cx, sidebar, 0)
            .iter()
            .map(|item| item.label.to_string())
            .collect()
    }

    /// The children of the root row's `index`th child.
    fn grandchildren(
        cx: &TestAppContext,
        sidebar: &Entity<Sidebar>,
        index: usize,
    ) -> Vec<TreeItem> {
        children_of(cx, sidebar, 0)[index].children.clone()
    }

    fn children_of(cx: &TestAppContext, sidebar: &Entity<Sidebar>, ix: usize) -> Vec<TreeItem> {
        sidebar.read_with(cx, |sidebar, cx| {
            sidebar
                .tree
                .read(cx)
                .entry(ix)
                .expect("a row at that index")
                .item()
                .children
                .clone()
        })
    }

    /// Expanding a directory is reported by the tree; the sidebar must turn
    /// that into the directory's real contents.
    fn expand(cx: &mut TestAppContext, sidebar: &Entity<Sidebar>, path: &Path) {
        let id = path.to_string_lossy().to_string();
        sidebar.update(cx, |sidebar, cx| {
            sidebar
                .tree
                .update(cx, |_, cx| cx.emit(TreeEvent::Expanded(id.into())));
        });
        // The emit is deferred to the end of the update cycle.
        cx.run_until_parked();
    }

    /// A folder's tree shows its Markdown files, top level already open.
    #[gpui_kit::test]
    fn the_tree_lists_markdown_files_with_directories_first(cx: &mut TestAppContext) {
        let root = scratch("listing");
        write(&root.join("b.md"), "# b");
        write(&root.join("A.md"), "# A");
        write(&root.join("notes.txt"), "not markdown");
        write(&root.join(".hidden.md"), "# hidden");
        write(&root.join("drafts").join("c.md"), "# c");

        let sidebar = sidebar_for(cx, &root);

        // Directories first, then files, each group sorted by name without
        // regard to case; `.hidden.md` and `notes.txt` are not listed at all.
        assert_eq!(child_labels(cx, &sidebar), ["drafts", "A.md", "b.md"]);
        // A collapsed directory keeps one stand-in child so that the tree still
        // treats it as a folder and will expand it.
        assert_eq!(grandchildren(cx, &sidebar, 0).len(), 1);
        assert!(grandchildren(cx, &sidebar, 0)[0].label.is_empty());

        std::fs::remove_dir_all(&root).ok();
    }

    /// Expanding a directory replaces the stand-in with what is on disk.
    #[gpui_kit::test]
    fn expanding_a_directory_reads_its_contents(cx: &mut TestAppContext) {
        let root = scratch("expand");
        write(&root.join("drafts").join("c.md"), "# c");
        write(&root.join("drafts").join("skip.txt"), "no");

        let sidebar = sidebar_for(cx, &root);
        assert_eq!(grandchildren(cx, &sidebar, 0).len(), 1, "still a stand-in");

        expand(cx, &sidebar, &root.join("drafts"));

        let drafts = &children_of(cx, &sidebar, 0)[0];
        assert!(drafts.is_folder());
        assert!(drafts.is_expanded());
        let labels = drafts
            .children
            .iter()
            .map(|item| item.label.to_string())
            .collect::<Vec<_>>();
        assert_eq!(labels, ["c.md"]);

        std::fs::remove_dir_all(&root).ok();
    }

    /// A directory with nothing to open is a leaf: no disclosure triangle, and
    /// expanding it can do nothing.
    #[gpui_kit::test]
    fn a_directory_without_markdown_is_not_expandable(cx: &mut TestAppContext) {
        let root = scratch("empty");
        write(&root.join("assets").join("logo.svg"), "<svg/>");
        write(&root.join("real.md"), "# real");

        let sidebar = sidebar_for(cx, &root);
        assert_eq!(child_labels(cx, &sidebar), ["assets", "real.md"]);

        let assets = &children_of(cx, &sidebar, 0)[0];
        assert!(assets.children.is_empty());
        assert!(
            !assets.is_folder(),
            "with no children the tree offers no way to expand it"
        );

        std::fs::remove_dir_all(&root).ok();
    }
}
