use core::EditorSessionSnapshot;
use core::tabs::editor::EditorDocument;
use core::tabs::git::GitLineHunk;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ids::{TabIdParam, WorkspaceIdParam};
use super::workspace::WorkspaceListSnapshot;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenEditorTabParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenEditorLocationParams {
    pub(crate) workspace_id: WorkspaceIdParam,
    pub(crate) path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditorDocumentParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SaveEditorDocumentParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) revision: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenEditorSessionParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) path: String,
    pub(crate) content: String,
    pub(crate) revision: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChangeEditorSessionParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) content: String,
    pub(crate) revision: u64,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditorDocumentPayload {
    path: String,
    content: String,
    saved_content: String,
    revision: u64,
    accepted: bool,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenEditorLocationPayload {
    snapshot: WorkspaceListSnapshot,
    target: EditorLocationTargetPayload,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
struct EditorLocationTargetPayload {
    workspace_id: u64,
    tab_id: u64,
    path: String,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditorGitLineHunksPayload {
    hunks: Vec<EditorGitLineHunkPayload>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditorGitLineHunkPayload {
    old_start: u32,
    old_lines: u32,
    new_start: u32,
    new_lines: u32,
}

impl EditorDocumentPayload {
    pub(crate) fn from_document(document: &EditorDocument) -> Self {
        Self {
            path: document.path().to_owned(),
            content: document.content().to_owned(),
            saved_content: document.content().to_owned(),
            revision: 0,
            accepted: true,
        }
    }

    pub(crate) fn from_session(session: EditorSessionSnapshot, accepted: bool) -> Self {
        Self {
            path: session.path,
            content: session.content,
            saved_content: session.saved_content,
            revision: session.revision,
            accepted,
        }
    }
}

impl OpenEditorLocationPayload {
    pub(crate) fn from_core(location: core::OpenEditorLocation) -> Self {
        Self {
            snapshot: WorkspaceListSnapshot::from_list(location.workspaces()),
            target: EditorLocationTargetPayload {
                workspace_id: location.workspace_id().value(),
                tab_id: location.tab_id().value(),
                path: location.path().to_owned(),
            },
        }
    }
}

impl EditorGitLineHunksPayload {
    pub(crate) fn from_hunks(hunks: &[GitLineHunk]) -> Self {
        Self {
            hunks: hunks
                .iter()
                .map(|hunk| EditorGitLineHunkPayload {
                    old_start: hunk.old_start(),
                    old_lines: hunk.old_lines(),
                    new_start: hunk.new_start(),
                    new_lines: hunk.new_lines(),
                })
                .collect(),
        }
    }

    pub(crate) fn empty() -> Self {
        Self { hunks: Vec::new() }
    }
}
