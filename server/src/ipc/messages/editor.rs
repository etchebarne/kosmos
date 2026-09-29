use core::tabs::editor::EditorDocument;
use core::tabs::git::GitLineHunk;
use core::{
    EditorSessionSaveResult, EditorSessionSnapshot, EditorSessionSync, EditorSessionUpdate,
    EditorTextEdit,
};
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
pub(crate) struct RestoreEditorSessionParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) path: String,
    pub(crate) content: String,
    pub(crate) saved_content: String,
    pub(crate) revision: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChangeEditorSessionParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) base_revision: u64,
    pub(crate) revision: u64,
    pub(crate) edits: Vec<EditorTextEditParam>,
}

/// A text replacement addressed in UTF-16 code units of the document before the edit.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditorTextEditParam {
    pub(crate) offset: usize,
    pub(crate) length: usize,
    pub(crate) text: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncEditorDocumentParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) known_revision: u64,
    pub(crate) known_saved_generation: u64,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditorDocumentPayload {
    path: String,
    content: String,
    /// Omitted when it equals `content`, which keeps clean documents to one copy.
    saved_content: Option<String>,
    revision: u64,
    saved_generation: u64,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncEditorDocumentPayload {
    path: String,
    /// Present only when the document changed since the caller's known version.
    document: Option<EditorDocumentPayload>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditorSessionAckPayload {
    revision: u64,
    saved_generation: u64,
    accepted: bool,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SaveEditorDocumentPayload {
    saved_revision: u64,
    /// Present only when saving changed the text, for example through format on save.
    saved_content: Option<String>,
    current_revision: u64,
    saved_generation: u64,
    warnings: Vec<EditorSaveWarningPayload>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditorSaveWarningPayload {
    kind: EditorSaveWarningKindPayload,
    code: String,
    message: String,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum EditorSaveWarningKindPayload {
    Formatting,
    LanguageServerNotification,
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
            saved_content: None,
            revision: 0,
            saved_generation: 0,
        }
    }

    pub(crate) fn from_session(session: EditorSessionSnapshot) -> Self {
        let saved_content = session.is_dirty().then_some(session.saved_content);
        Self {
            path: session.path,
            content: session.content,
            saved_content,
            revision: session.revision,
            saved_generation: session.saved_generation,
        }
    }
}

impl SyncEditorDocumentPayload {
    pub(crate) fn from_core(sync: EditorSessionSync) -> Self {
        Self {
            path: sync.path,
            document: sync.document.map(EditorDocumentPayload::from_session),
        }
    }
}

impl EditorSessionAckPayload {
    pub(crate) fn from_update(update: EditorSessionUpdate) -> Self {
        let (ack, accepted) = match update {
            EditorSessionUpdate::Applied(ack) => (ack, true),
            EditorSessionUpdate::Stale(ack) => (ack, false),
        };
        Self {
            revision: ack.revision,
            saved_generation: ack.saved_generation,
            accepted,
        }
    }
}

impl EditorTextEditParam {
    pub(crate) fn into_core(self) -> EditorTextEdit {
        EditorTextEdit {
            offset: self.offset,
            length: self.length,
            text: self.text,
        }
    }
}

impl SaveEditorDocumentPayload {
    pub(crate) fn from_core(result: EditorSessionSaveResult) -> Self {
        Self {
            saved_revision: result.saved_revision(),
            saved_content: result
                .reformatted()
                .then(|| result.saved_content().to_owned()),
            current_revision: result.current_revision(),
            saved_generation: result.saved_generation(),
            warnings: result
                .warnings()
                .iter()
                .map(|warning| EditorSaveWarningPayload {
                    kind: match warning.kind() {
                        core::EditorSessionSaveWarningKind::Formatting => {
                            EditorSaveWarningKindPayload::Formatting
                        }
                        core::EditorSessionSaveWarningKind::LanguageServerNotification => {
                            EditorSaveWarningKindPayload::LanguageServerNotification
                        }
                    },
                    code: warning.code().to_owned(),
                    message: warning.message().to_owned(),
                })
                .collect(),
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
}
