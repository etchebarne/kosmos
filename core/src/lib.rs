mod application;
mod editor_sessions;
mod state;
mod workspace_changes;

pub mod events;
pub mod formatters;
pub mod language_servers;
pub mod persistence;
pub mod settings;
pub use application::{
    Application, ApplicationError, CloseDecision, CloseDocumentDecision,
    CloseDocumentDecisionRequest, CloseIntent, CloseIntentResult, CloseTarget,
    PreparedExternalOperation, PreparedPersistentOperation,
};
pub use editor_sessions::{
    EditorSessionError, EditorSessionId, EditorSessionRegistry, EditorSessionSnapshot,
    EditorSessionUpdate,
};
pub use persistence::StateStore as DurableStore;
pub use state::State;
pub use workspace_changes::WorkspaceChangeWatcher;
pub mod tabs;
pub mod tree;
pub mod window;
