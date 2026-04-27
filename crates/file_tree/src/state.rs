use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use gpui::{App, Context, Entity, Global, Pixels, Point, SharedString, Task};

use crate::ops::{self, ClipboardOp};
use crate::watcher::{self, FsEvents};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    File,
    Directory,
}

#[derive(Clone)]
pub struct Node {
    pub path: PathBuf,
    pub name: SharedString,
    pub kind: NodeKind,
}

#[derive(Clone)]
pub struct RenameTarget {
    pub path: PathBuf,
    pub original_name: SharedString,
}

#[derive(Clone)]
pub struct NewEntryDraft {
    pub parent: PathBuf,
    pub kind: NodeKind,
}

#[derive(Clone, Debug)]
pub struct ContextMenuState {
    pub target: Option<PathBuf>,
    pub position: Point<Pixels>,
}

pub struct FileTree {
    root: Option<PathBuf>,
    children: HashMap<PathBuf, Vec<Node>>,
    expanded: HashSet<PathBuf>,
    selected: Option<PathBuf>,
    clipboard: Option<(ClipboardOp, PathBuf)>,
    error: Option<SharedString>,
    rename: Option<RenameTarget>,
    new_entry: Option<NewEntryDraft>,
    context_menu: Option<ContextMenuState>,

    // Watcher resources kept alive for the lifetime of the active root.
    watcher: Option<notify::RecommendedWatcher>,
    watcher_rx: Option<Receiver<FsEvents>>,
    watcher_task: Option<Task<()>>,
}

impl FileTree {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let _ = cx;
        Self {
            root: None,
            children: HashMap::new(),
            expanded: HashSet::new(),
            selected: None,
            clipboard: None,
            error: None,
            rename: None,
            new_entry: None,
            context_menu: None,
            watcher: None,
            watcher_rx: None,
            watcher_task: None,
        }
    }

    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    pub fn selected(&self) -> Option<&Path> {
        self.selected.as_deref()
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    pub fn clear_error(&mut self) {
        self.error = None;
    }

    pub fn clipboard(&self) -> Option<(ClipboardOp, &Path)> {
        self.clipboard.as_ref().map(|(op, p)| (*op, p.as_path()))
    }

    pub fn rename_target(&self) -> Option<&RenameTarget> {
        self.rename.as_ref()
    }

    pub fn new_entry_draft(&self) -> Option<&NewEntryDraft> {
        self.new_entry.as_ref()
    }

    pub fn context_menu(&self) -> Option<&ContextMenuState> {
        self.context_menu.as_ref()
    }

    pub fn is_expanded(&self, path: &Path) -> bool {
        self.expanded.contains(path)
    }

    pub fn children_of(&self, path: &Path) -> Option<&[Node]> {
        self.children.get(path).map(|v| v.as_slice())
    }

    /// Set or replace the root path. Drops any existing watcher and reloads
    /// the top-level directory listing, then installs a recursive watcher.
    pub fn set_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if Some(&root) == self.root.as_ref() {
            return;
        }
        self.children.clear();
        self.expanded.clear();
        self.selected = None;
        self.error = None;
        self.rename = None;
        self.new_entry = None;
        self.context_menu = None;
        self.watcher = None;
        self.watcher_rx = None;
        self.watcher_task = None;

        self.root = Some(root.clone());
        self.expanded.insert(root.clone());
        self.reload_dir(&root);

        // Install a watcher and a polling task to drain its events.
        let (events_tx, events_rx) = channel::<FsEvents>();
        match watcher::start(&root, events_tx) {
            Ok(w) => {
                self.watcher = Some(w);
                self.watcher_rx = Some(events_rx);
                self.spawn_poll_task(cx);
            }
            Err(err) => {
                self.error = Some(format!("Watcher failed: {err}").into());
            }
        }

        cx.notify();
    }

    fn spawn_poll_task(&mut self, cx: &mut Context<Self>) {
        let task = cx.spawn(async move |this, cx| {
            loop {
                let timer = cx
                    .background_executor()
                    .timer(Duration::from_millis(150));
                timer.await;

                let mut paths_to_refresh: HashSet<PathBuf> = HashSet::new();
                let drained = this
                    .update(cx, |tree, _| {
                        let Some(rx) = tree.watcher_rx.as_ref() else {
                            return false;
                        };
                        while let Ok(events) = rx.try_recv() {
                            for path in events.affected_paths {
                                if let Some(parent) = path.parent() {
                                    paths_to_refresh.insert(parent.to_path_buf());
                                }
                                if path.is_dir() {
                                    paths_to_refresh.insert(path);
                                }
                            }
                        }
                        true
                    })
                    .ok();

                if drained.is_none() {
                    break;
                }

                if !paths_to_refresh.is_empty() {
                    let _ = this.update(cx, |tree, cx| {
                        let mut changed = false;
                        for path in paths_to_refresh {
                            if tree.children.contains_key(&path) {
                                tree.reload_dir(&path);
                                changed = true;
                            }
                        }
                        if changed {
                            cx.notify();
                        }
                    });
                }
            }
        });
        self.watcher_task = Some(task);
    }

    /// Re-read a directory's contents from disk into our cache.
    pub fn reload_dir(&mut self, path: &Path) {
        match ops::read_dir(path) {
            Ok(nodes) => {
                self.children.insert(path.to_path_buf(), nodes);
            }
            Err(err) => {
                self.error = Some(format!("Failed to read {}: {err}", path.display()).into());
                self.children.insert(path.to_path_buf(), Vec::new());
            }
        }
    }

    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        self.expanded.clear();
        if let Some(root) = self.root.clone() {
            self.expanded.insert(root);
        }
        cx.notify();
    }

    pub fn toggle_expand(&mut self, path: &Path, cx: &mut Context<Self>) {
        if self.expanded.contains(path) {
            self.expanded.remove(path);
        } else {
            self.expanded.insert(path.to_path_buf());
            if !self.children.contains_key(path) {
                self.reload_dir(path);
            }
        }
        cx.notify();
    }

    pub fn expand(&mut self, path: &Path, cx: &mut Context<Self>) {
        if !self.expanded.contains(path) {
            self.expanded.insert(path.to_path_buf());
            if !self.children.contains_key(path) {
                self.reload_dir(path);
            }
            cx.notify();
        }
    }

    pub fn select(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.selected = Some(path);
        cx.notify();
    }

    pub fn open_context_menu(
        &mut self,
        target: Option<PathBuf>,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = Some(ContextMenuState { target, position });
        cx.notify();
    }

    pub fn close_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.context_menu.take().is_some() {
            cx.notify();
        }
    }

    pub fn cut(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.clipboard = Some((ClipboardOp::Cut, path));
        cx.notify();
    }

    pub fn copy(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.clipboard = Some((ClipboardOp::Copy, path));
        cx.notify();
    }

    pub fn paste_into(&mut self, dest_dir: PathBuf, cx: &mut Context<Self>) {
        let Some((op, src)) = self.clipboard.clone() else {
            return;
        };
        let target_dir = if dest_dir.is_dir() {
            dest_dir
        } else if let Some(parent) = dest_dir.parent() {
            parent.to_path_buf()
        } else {
            return;
        };
        match ops::paste(&src, &target_dir, op) {
            Ok(_) => {
                if op == ClipboardOp::Cut {
                    self.clipboard = None;
                }
                self.reload_dir(&target_dir);
                if let Some(src_parent) = src.parent()
                    && self.children.contains_key(src_parent)
                {
                    self.reload_dir(src_parent);
                }
                cx.notify();
            }
            Err(err) => self.set_error(format!("Paste failed: {err}"), cx),
        }
    }

    pub fn move_into(&mut self, src: PathBuf, dest_dir: PathBuf, cx: &mut Context<Self>) {
        let target_dir = if dest_dir.is_dir() {
            dest_dir
        } else if let Some(parent) = dest_dir.parent() {
            parent.to_path_buf()
        } else {
            return;
        };
        if src == target_dir
            || target_dir
                .ancestors()
                .any(|a| a == src.as_path())
        {
            return;
        }
        let src_parent = src.parent().map(Path::to_path_buf);
        match ops::move_into(&src, &target_dir) {
            Ok(_) => {
                self.reload_dir(&target_dir);
                if let Some(parent) = src_parent
                    && self.children.contains_key(&parent)
                {
                    self.reload_dir(&parent);
                }
                cx.notify();
            }
            Err(err) => self.set_error(format!("Move failed: {err}"), cx),
        }
    }

    pub fn trash(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        match ops::trash(&path) {
            Ok(_) => {
                if let Some(parent) = path.parent()
                    && self.children.contains_key(parent)
                {
                    self.reload_dir(parent);
                }
                if self.selected.as_deref() == Some(path.as_path()) {
                    self.selected = None;
                }
                cx.notify();
            }
            Err(err) => self.set_error(format!("Trash failed: {err}"), cx),
        }
    }

    pub fn delete(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        match ops::delete(&path) {
            Ok(_) => {
                if let Some(parent) = path.parent()
                    && self.children.contains_key(parent)
                {
                    self.reload_dir(parent);
                }
                if self.selected.as_deref() == Some(path.as_path()) {
                    self.selected = None;
                }
                cx.notify();
            }
            Err(err) => self.set_error(format!("Delete failed: {err}"), cx),
        }
    }

    pub fn start_rename(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let original = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        self.rename = Some(RenameTarget {
            path,
            original_name: original.into(),
        });
        cx.notify();
    }

    pub fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        if self.rename.take().is_some() {
            cx.notify();
        }
    }

    pub fn apply_rename(&mut self, new_name: String, cx: &mut Context<Self>) {
        let Some(target) = self.rename.take() else {
            return;
        };
        let trimmed = new_name.trim();
        if trimmed.is_empty() || trimmed == target.original_name.as_ref() {
            cx.notify();
            return;
        }
        let Some(parent) = target.path.parent() else {
            return;
        };
        let new_path = parent.join(trimmed);
        match ops::rename(&target.path, &new_path) {
            Ok(_) => {
                if self.children.contains_key(parent) {
                    self.reload_dir(parent);
                }
                if self.selected.as_deref() == Some(target.path.as_path()) {
                    self.selected = Some(new_path);
                }
                cx.notify();
            }
            Err(err) => self.set_error(format!("Rename failed: {err}"), cx),
        }
    }

    pub fn start_new_entry(
        &mut self,
        anchor: Option<&Path>,
        kind: NodeKind,
        cx: &mut Context<Self>,
    ) {
        let parent = match anchor {
            Some(path) if path.is_dir() => path.to_path_buf(),
            Some(path) => path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.root.clone().unwrap_or_default()),
            None => self.root.clone().unwrap_or_default(),
        };
        self.expand(&parent, cx);
        self.new_entry = Some(NewEntryDraft { parent, kind });
        cx.notify();
    }

    pub fn cancel_new_entry(&mut self, cx: &mut Context<Self>) {
        if self.new_entry.take().is_some() {
            cx.notify();
        }
    }

    pub fn apply_new_entry(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(draft) = self.new_entry.take() else {
            return;
        };
        let trimmed = name.trim();
        if trimmed.is_empty() {
            cx.notify();
            return;
        }
        let path = draft.parent.join(trimmed);
        let result = match draft.kind {
            NodeKind::File => ops::create_file(&path),
            NodeKind::Directory => ops::create_dir(&path),
        };
        match result {
            Ok(_) => {
                self.reload_dir(&draft.parent);
                self.selected = Some(path);
                cx.notify();
            }
            Err(err) => self.set_error(format!("Create failed: {err}"), cx),
        }
    }

    fn set_error(&mut self, message: String, cx: &mut Context<Self>) {
        self.error = Some(message.into());
        cx.notify();
    }
}

/// Global wrapper around the active workspace's `FileTree` entity.
#[derive(Default)]
pub struct FileTreeState {
    active: Option<Entity<FileTree>>,
}

impl FileTreeState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_active(&mut self, entity: Option<Entity<FileTree>>) {
        self.active = entity;
    }

    pub fn active(&self) -> Option<&Entity<FileTree>> {
        self.active.as_ref()
    }
}

impl Global for FileTreeState {}

pub trait ActiveFileTree {
    fn file_tree(&self) -> Option<&Entity<FileTree>>;
}

impl ActiveFileTree for App {
    fn file_tree(&self) -> Option<&Entity<FileTree>> {
        self.try_global::<FileTreeState>()
            .and_then(|s| s.active.as_ref())
    }
}

/// Marker trait that signals an app-level type can mutate the file tree state
/// in response to UI events. Currently empty — actions are dispatched directly
/// against the `FileTree` entity, so the trait exists only for any future
/// app-level callbacks we want to plug in (e.g. opening a file in the editor).
pub trait FileTreeUiActions: Sized + 'static {}
