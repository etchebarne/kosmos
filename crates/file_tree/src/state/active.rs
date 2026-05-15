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

impl EventEmitter<FileTreeEvent> for FileTree {}

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
