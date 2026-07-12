use core::EditorSessionSnapshot;
use core::tree::{Tab, TabKind, TabLifecycle};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ids::{PaneIdParam, TabIdParam, WorkspaceIdParam};
use super::pane::SplitAxisPayload;
use super::workspace::WorkspaceListSnapshot;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenTabParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) pane_id: Option<PaneIdParam>,
    pub(crate) title: Option<String>,
    pub(crate) kind: Option<TabKindPayload>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActivateTabParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) pane_id: PaneIdParam,
    pub(crate) tab_id: TabIdParam,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetTabKindParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) pane_id: PaneIdParam,
    pub(crate) tab_id: TabIdParam,
    pub(crate) kind: TabKindPayload,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloseTabParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) pane_id: PaneIdParam,
    pub(crate) tab_id: TabIdParam,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolveCloseParams {
    pub(crate) close_id: u64,
    pub(crate) decision: CloseDecisionPayload,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub(crate) enum CloseDecisionPayload {
    Cancel,
    Resolve {
        documents: Vec<CloseDocumentDecisionPayload>,
    },
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloseDocumentDecisionPayload {
    pub(crate) workspace_id: WorkspaceIdParam,
    pub(crate) tab_id: TabIdParam,
    pub(crate) revision: u64,
    pub(crate) decision: CloseDocumentDecisionKindPayload,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CloseDocumentDecisionKindPayload {
    Save,
    Discard,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub(crate) enum CloseResultPayload {
    Completed {
        snapshot: WorkspaceListSnapshot,
    },
    RequiresDocumentDecision {
        #[serde(rename = "closeId")]
        close_id: u64,
        documents: Vec<UnsavedDocumentPayload>,
    },
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UnsavedDocumentPayload {
    workspace_id: u64,
    tab_id: u64,
    path: String,
    revision: u64,
}

impl CloseResultPayload {
    pub(crate) fn from_core(
        result: core::CloseIntentResult,
        snapshot: WorkspaceListSnapshot,
    ) -> Self {
        match result {
            core::CloseIntentResult::Completed => Self::Completed { snapshot },
            core::CloseIntentResult::RequiresDocumentDecision {
                close_id,
                documents,
            } => Self::RequiresDocumentDecision {
                close_id,
                documents: documents
                    .iter()
                    .map(UnsavedDocumentPayload::from_session)
                    .collect(),
            },
        }
    }
}

impl UnsavedDocumentPayload {
    fn from_session(session: &EditorSessionSnapshot) -> Self {
        Self {
            workspace_id: session.id.workspace_id.value(),
            tab_id: session.id.tab_id.value(),
            path: session.path.clone(),
            revision: session.revision,
        }
    }
}

impl From<CloseDocumentDecisionKindPayload> for core::CloseDocumentDecision {
    fn from(value: CloseDocumentDecisionKindPayload) -> Self {
        match value {
            CloseDocumentDecisionKindPayload::Save => Self::Save,
            CloseDocumentDecisionKindPayload::Discard => Self::Discard,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MoveTabParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) pane_id: PaneIdParam,
    pub(crate) target_pane_id: PaneIdParam,
    pub(crate) tab_id: TabIdParam,
    pub(crate) target_index: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SplitTabParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) pane_id: PaneIdParam,
    pub(crate) target_pane_id: Option<PaneIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) axis: SplitAxisPayload,
    pub(crate) new_pane_first: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TabKindPayload {
    #[default]
    Blank,
    Diff,
    FileTree,
    Editor,
    Git,
    Search,
    Terminal,
}

impl From<TabKindPayload> for TabKind {
    fn from(value: TabKindPayload) -> Self {
        match value {
            TabKindPayload::Blank => Self::Blank,
            TabKindPayload::Diff => Self::Diff,
            TabKindPayload::FileTree => Self::FileTree,
            TabKindPayload::Editor => Self::Editor,
            TabKindPayload::Git => Self::Git,
            TabKindPayload::Search => Self::Search,
            TabKindPayload::Terminal => Self::Terminal,
        }
    }
}

impl From<&TabKind> for TabKindPayload {
    fn from(value: &TabKind) -> Self {
        match value {
            TabKind::Blank => Self::Blank,
            TabKind::Diff => Self::Diff,
            TabKind::FileTree => Self::FileTree,
            TabKind::Editor => Self::Editor,
            TabKind::Git => Self::Git,
            TabKind::Search => Self::Search,
            TabKind::Terminal => Self::Terminal,
        }
    }
}

#[derive(Clone, Copy, Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TabLifecyclePayload {
    Ephemeral,
    KeepAlive,
}

impl From<TabLifecycle> for TabLifecyclePayload {
    fn from(value: TabLifecycle) -> Self {
        match value {
            TabLifecycle::Ephemeral => Self::Ephemeral,
            TabLifecycle::KeepAlive => Self::KeepAlive,
        }
    }
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TabSnapshot {
    id: u64,
    title: String,
    kind: TabKindPayload,
    lifecycle: TabLifecyclePayload,
}

impl TabSnapshot {
    pub(crate) fn from_tab(tab: &Tab) -> Self {
        Self {
            id: tab.id().value(),
            title: tab.title().to_owned(),
            kind: tab.kind().into(),
            lifecycle: tab.kind().lifecycle().into(),
        }
    }
}
