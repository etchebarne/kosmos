use std::path::PathBuf;

use file_tree::{ActiveFileTree, FileTree};
use gpui::{App, AppContext, Bounds, Entity, Global, Pixels};
use gpui_component::{
    input::{InputEvent, InputState},
    tree::TreeState,
};

use super::actions;

pub(crate) struct PendingFileTreeDrop {
    pub tree: Entity<FileTree>,
    pub paths: Vec<PathBuf>,
    pub destination: PathBuf,
    pub bounds: Bounds<Pixels>,
}

/// Holds stateful file tree UI components so they survive re-renders triggered
/// by file system updates.
pub struct FileTreeUi {
    input: Entity<InputState>,
    tree: Entity<TreeState>,
    pending_drop: Option<PendingFileTreeDrop>,
}

impl FileTreeUi {
    pub fn install(window: &mut gpui::Window, cx: &mut App) {
        let input = cx.new(|cx| InputState::new(window, cx));
        let tree = cx.new(|cx| TreeState::new(cx));
        cx.subscribe(&input, |_, event: &InputEvent, cx| {
            if !matches!(event, InputEvent::Blur) {
                return;
            }
            let Some(file_tree) = cx.file_tree().cloned() else {
                return;
            };
            actions::commit_pending_input(&file_tree, cx);
        })
        .detach();
        cx.set_global(FileTreeUi {
            input,
            tree,
            pending_drop: None,
        });
    }

    pub fn input(&self) -> Entity<InputState> {
        self.input.clone()
    }

    pub fn tree(&self) -> Entity<TreeState> {
        self.tree.clone()
    }

    pub(crate) fn set_pending_drop(&mut self, pending_drop: PendingFileTreeDrop) {
        self.pending_drop = Some(pending_drop);
    }

    pub(crate) fn clear_pending_drop(&mut self) {
        self.pending_drop = None;
    }

    pub(crate) fn take_pending_drop(&mut self) -> Option<PendingFileTreeDrop> {
        self.pending_drop.take()
    }
}

impl Global for FileTreeUi {}

pub trait ActiveFileTreeUi {
    fn file_tree_ui(&self) -> Option<&FileTreeUi>;
}

impl ActiveFileTreeUi for App {
    fn file_tree_ui(&self) -> Option<&FileTreeUi> {
        self.try_global::<FileTreeUi>()
    }
}
