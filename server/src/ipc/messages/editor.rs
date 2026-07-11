use core::tabs::editor::EditorDocument;
use core::tabs::git::GitLineHunk;
use serde::{Deserialize, Serialize};

use super::ids::{TabIdParam, WorkspaceIdParam};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenEditorTabParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditorDocumentParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SaveEditorDocumentParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) content: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditorDocumentPayload {
    path: String,
    content: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditorGitLineHunksPayload {
    hunks: Vec<EditorGitLineHunkPayload>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EditorGitLineHunkPayload {
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
