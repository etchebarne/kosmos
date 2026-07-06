use core::file_tree::FileTree;
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileTreeSnapshot {
    root: String,
    paths: Vec<String>,
    expanded_paths: Vec<String>,
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
