use core::tree::{Workspace, WorkspaceList};
use serde::{Deserialize, Serialize};

use super::ids::WorkspaceIdParam;
use super::pane::PaneNodeSnapshot;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenWorkspaceParams {
    pub(crate) path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActivateWorkspaceParams {
    pub(crate) workspace_id: WorkspaceIdParam,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloseWorkspaceParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceListSnapshot {
    active_workspace_id: Option<u64>,
    workspaces: Vec<WorkspaceSnapshot>,
}

impl WorkspaceListSnapshot {
    pub(crate) fn from_list(workspaces: &WorkspaceList) -> Self {
        Self {
            active_workspace_id: workspaces.active_workspace_id().map(|id| id.value()),
            workspaces: workspaces
                .workspaces()
                .iter()
                .map(WorkspaceSnapshot::from_workspace)
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceSnapshot {
    id: u64,
    name: String,
    directory: String,
    active_pane_id: u64,
    root: PaneNodeSnapshot,
}

impl WorkspaceSnapshot {
    fn from_workspace(workspace: &Workspace) -> Self {
        Self {
            id: workspace.id().value(),
            name: workspace.name().to_owned(),
            directory: workspace.directory().to_string_lossy().into_owned(),
            active_pane_id: workspace.active_pane_id().value(),
            root: PaneNodeSnapshot::from_node(workspace.root()),
        }
    }
}
