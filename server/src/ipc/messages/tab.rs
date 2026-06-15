use core::tree::{Tab, TabId, TabKind};
use serde::{Deserialize, Serialize};

use super::pane::{PaneIdParam, SplitAxisPayload, WorkspaceIdParam};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenTabParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) pane_id: Option<PaneIdParam>,
    pub(crate) title: Option<String>,
    pub(crate) kind: Option<TabKindPayload>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActivateTabParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) pane_id: PaneIdParam,
    pub(crate) tab_id: TabIdParam,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetTabKindParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) pane_id: PaneIdParam,
    pub(crate) tab_id: TabIdParam,
    pub(crate) kind: TabKindPayload,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloseTabParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) pane_id: PaneIdParam,
    pub(crate) tab_id: TabIdParam,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReorderTabParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) pane_id: PaneIdParam,
    pub(crate) tab_id: TabIdParam,
    pub(crate) target_index: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SplitTabParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) pane_id: PaneIdParam,
    pub(crate) target_pane_id: Option<PaneIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) axis: SplitAxisPayload,
    pub(crate) new_pane_first: Option<bool>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub(crate) struct TabIdParam(u64);

impl From<TabIdParam> for TabId {
    fn from(value: TabIdParam) -> Self {
        Self::new(value.0)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TabKindPayload {
    #[default]
    Blank,
    FileTree,
    Editor,
    Git,
    Search,
    Terminal,
    Settings,
}

impl From<TabKindPayload> for TabKind {
    fn from(value: TabKindPayload) -> Self {
        match value {
            TabKindPayload::Blank => Self::Blank,
            TabKindPayload::FileTree => Self::FileTree,
            TabKindPayload::Editor => Self::Editor,
            TabKindPayload::Git => Self::Git,
            TabKindPayload::Search => Self::Search,
            TabKindPayload::Terminal => Self::Terminal,
            TabKindPayload::Settings => Self::Settings,
        }
    }
}

impl From<&TabKind> for TabKindPayload {
    fn from(value: &TabKind) -> Self {
        match value {
            TabKind::Blank => Self::Blank,
            TabKind::FileTree => Self::FileTree,
            TabKind::Editor => Self::Editor,
            TabKind::Git => Self::Git,
            TabKind::Search => Self::Search,
            TabKind::Terminal => Self::Terminal,
            TabKind::Settings => Self::Settings,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TabSnapshot {
    id: u64,
    title: String,
    kind: TabKindPayload,
}

impl TabSnapshot {
    pub(crate) fn from_tab(tab: &Tab) -> Self {
        Self {
            id: tab.id().value(),
            title: tab.title().to_owned(),
            kind: tab.kind().into(),
        }
    }
}
