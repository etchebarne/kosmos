use core::tree::{Pane, PaneId, PaneNode, SplitAxis};
use serde::{Deserialize, Serialize};

use super::tab::TabSnapshot;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SplitPaneParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) pane_id: Option<PaneIdParam>,
    pub(crate) axis: SplitAxisPayload,
    pub(crate) new_pane_first: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActivatePaneParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) pane_id: PaneIdParam,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub(crate) struct WorkspaceIdParam(u64);

impl From<WorkspaceIdParam> for core::tree::WorkspaceId {
    fn from(value: WorkspaceIdParam) -> Self {
        Self::new(value.0)
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub(crate) struct PaneIdParam(u64);

impl From<PaneIdParam> for PaneId {
    fn from(value: PaneIdParam) -> Self {
        Self::new(value.0)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SplitAxisPayload {
    Horizontal,
    Vertical,
}

impl From<SplitAxisPayload> for SplitAxis {
    fn from(value: SplitAxisPayload) -> Self {
        match value {
            SplitAxisPayload::Horizontal => Self::Horizontal,
            SplitAxisPayload::Vertical => Self::Vertical,
        }
    }
}

impl From<SplitAxis> for SplitAxisPayload {
    fn from(value: SplitAxis) -> Self {
        match value {
            SplitAxis::Horizontal => Self::Horizontal,
            SplitAxis::Vertical => Self::Vertical,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub(crate) enum PaneNodeSnapshot {
    Leaf {
        pane: PaneSnapshot,
    },
    Split {
        axis: SplitAxisPayload,
        ratio: f32,
        first: Box<PaneNodeSnapshot>,
        second: Box<PaneNodeSnapshot>,
    },
}

impl PaneNodeSnapshot {
    pub(crate) fn from_node(node: &PaneNode) -> Self {
        match node {
            PaneNode::Leaf(pane) => Self::Leaf {
                pane: PaneSnapshot::from_pane(pane),
            },
            PaneNode::Split(split) => Self::Split {
                axis: split.axis().into(),
                ratio: split.ratio(),
                first: Box::new(Self::from_node(split.first())),
                second: Box::new(Self::from_node(split.second())),
            },
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PaneSnapshot {
    id: u64,
    active_tab_id: u64,
    tabs: Vec<TabSnapshot>,
}

impl PaneSnapshot {
    fn from_pane(pane: &Pane) -> Self {
        Self {
            id: pane.id().value(),
            active_tab_id: pane.active_tab_id().value(),
            tabs: pane.tabs().iter().map(TabSnapshot::from_tab).collect(),
        }
    }
}
