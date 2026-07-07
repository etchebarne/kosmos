mod panes;
mod tabs;
mod workspaces;

pub use panes::{Pane, PaneId, PaneNode, SplitAxis, SplitPane, SplitPaneId};
pub use tabs::{Tab, TabId, TabKind, TabLifecycle};
pub use workspaces::{Workspace, WorkspaceId, WorkspaceList};
