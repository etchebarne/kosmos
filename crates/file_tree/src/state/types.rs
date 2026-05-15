use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use gpui::{App, Context, Entity, EventEmitter, Global, Pixels, Point, SharedString, Task};

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
    selected: HashSet<PathBuf>,
    selection_anchor: Option<PathBuf>,
    clipboard: Option<(ClipboardOp, Vec<PathBuf>)>,
    error: Option<SharedString>,
    rename: Option<RenameTarget>,
    new_entry: Option<NewEntryDraft>,
    context_menu: Option<ContextMenuState>,

    // Watcher resources kept alive for the lifetime of the active root.
    watcher: Option<notify::RecommendedWatcher>,
    watcher_rx: Option<Receiver<FsEvents>>,
    watcher_task: Option<Task<()>>,
}

#[derive(Clone, Debug)]
pub enum FileTreeEvent {
    FsChanged { paths: Vec<PathBuf> },
}

