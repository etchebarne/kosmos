use core::file_tree::{FileTree, FileTreeEntryKind};
use serde::{Deserialize, Serialize};

use super::pane::WorkspaceIdParam;
use super::tab::TabIdParam;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GetFileTreeParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: Option<TabIdParam>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetFileTreeExpandedPathsParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) expanded_paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateFileTreeEntryParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) parent_path: Option<String>,
    pub(crate) name: String,
    pub(crate) kind: FileTreeEntryKindParam,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum FileTreeEntryKindParam {
    Directory,
    File,
}

impl From<FileTreeEntryKindParam> for FileTreeEntryKind {
    fn from(kind: FileTreeEntryKindParam) -> Self {
        match kind {
            FileTreeEntryKindParam::Directory => Self::Directory,
            FileTreeEntryKindParam::File => Self::File,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RenameFileTreeEntryParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) source_path: String,
    pub(crate) destination_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TransferFileTreeEntriesParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) source_paths: Vec<String>,
    pub(crate) target_directory_path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteFileTreeEntryParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteFileTreeEntriesParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolveFileTreePathParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileTreeSnapshot {
    root: String,
    paths: Vec<String>,
    expanded_paths: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileTreeResolvedPath {
    path: String,
}

impl FileTreeSnapshot {
    pub(crate) fn from_file_tree(file_tree: &FileTree) -> Self {
        Self {
            root: file_tree.root().to_string_lossy().into_owned(),
            paths: file_tree.paths().to_vec(),
            expanded_paths: file_tree.expanded_paths().to_vec(),
        }
    }
}

impl FileTreeResolvedPath {
    pub(crate) fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
}
