use gpui::{App, AppContext, Entity, Global, ScrollHandle};

use crate::components::TextInput;

/// Holds the single rename / new-entry text input plus the persistent scroll
/// handle for the file tree. We keep these out of the render so they survive
/// re-renders triggered by file system updates.
pub struct FileTreeUi {
    input: Entity<TextInput>,
    scroll: ScrollHandle,
}

impl FileTreeUi {
    pub fn install(cx: &mut App) {
        let input = cx.new(|cx| TextInput::new("", "", cx));
        cx.set_global(FileTreeUi {
            input,
            scroll: ScrollHandle::new(),
        });
    }

    pub fn input(&self) -> Entity<TextInput> {
        self.input.clone()
    }

    pub fn scroll(&self) -> ScrollHandle {
        self.scroll.clone()
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
