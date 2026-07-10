mod state;
mod workspace_changes;

pub mod persistence;
pub use state::{PersistentStateCandidate, State};
pub use workspace_changes::WorkspaceChangeWatcher;
pub mod tabs;
pub mod tree;
