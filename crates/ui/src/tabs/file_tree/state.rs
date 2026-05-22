use gpui::{App, AppContext, Entity, Global};
use gpui_component::{input::InputState, tree::TreeState};

/// Holds stateful file tree UI components so they survive re-renders triggered
/// by file system updates.
pub struct FileTreeUi {
    input: Entity<InputState>,
    tree: Entity<TreeState>,
}

impl FileTreeUi {
    pub fn install(window: &mut gpui::Window, cx: &mut App) {
        let input = cx.new(|cx| InputState::new(window, cx));
        let tree = cx.new(|cx| TreeState::new(cx));
        cx.set_global(FileTreeUi {
            input,
            tree,
        });
    }

    pub fn input(&self) -> Entity<InputState> {
        self.input.clone()
    }

    pub fn tree(&self) -> Entity<TreeState> {
        self.tree.clone()
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
