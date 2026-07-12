use core::tree::{PaneId, SplitPaneId, TabId, WorkspaceId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub(crate) struct WorkspaceIdParam(u64);

impl From<WorkspaceIdParam> for WorkspaceId {
    fn from(value: WorkspaceIdParam) -> Self {
        Self::new(value.0)
    }
}

impl From<WorkspaceId> for WorkspaceIdParam {
    fn from(value: WorkspaceId) -> Self {
        Self(value.value())
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub(crate) struct PaneIdParam(u64);

impl From<PaneIdParam> for PaneId {
    fn from(value: PaneIdParam) -> Self {
        Self::new(value.0)
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub(crate) struct SplitPaneIdParam(u64);

impl From<SplitPaneIdParam> for SplitPaneId {
    fn from(value: SplitPaneIdParam) -> Self {
        Self::new(value.0)
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub(crate) struct TabIdParam(u64);

impl From<TabIdParam> for TabId {
    fn from(value: TabIdParam) -> Self {
        Self::new(value.0)
    }
}
