use core::file_tree::{
    FileTree, FileTreeCreateKind, FileTreeEntry, FileTreeEntryKind, FileTreeOptions,
};
use serde::{Deserialize, Serialize};

use super::pane::WorkspaceIdParam;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileTreeParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    max_depth: Option<usize>,
    max_entries_per_directory: Option<usize>,
}

impl FileTreeParams {
    pub(crate) fn options(&self) -> FileTreeOptions {
        let defaults = FileTreeOptions::default();

        FileTreeOptions::new(
            self.max_depth.unwrap_or(defaults.max_depth()),
            self.max_entries_per_directory
                .unwrap_or(defaults.max_entries_per_directory()),
        )
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateFileTreeEntryParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) parent_path: String,
    pub(crate) name: String,
    pub(crate) kind: FileTreeCreateKindPayload,
    max_depth: Option<usize>,
    max_entries_per_directory: Option<usize>,
}

impl CreateFileTreeEntryParams {
    pub(crate) fn options(&self) -> FileTreeOptions {
        options(self.max_depth, self.max_entries_per_directory)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RenameFileTreeEntryParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) path: String,
    pub(crate) name: String,
    max_depth: Option<usize>,
    max_entries_per_directory: Option<usize>,
}

impl RenameFileTreeEntryParams {
    pub(crate) fn options(&self) -> FileTreeOptions {
        options(self.max_depth, self.max_entries_per_directory)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteFileTreeEntryParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) path: String,
    max_depth: Option<usize>,
    max_entries_per_directory: Option<usize>,
}

impl DeleteFileTreeEntryParams {
    pub(crate) fn options(&self) -> FileTreeOptions {
        options(self.max_depth, self.max_entries_per_directory)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MoveFileTreeEntryParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) path: String,
    pub(crate) target_directory_path: String,
    max_depth: Option<usize>,
    max_entries_per_directory: Option<usize>,
}

impl MoveFileTreeEntryParams {
    pub(crate) fn options(&self) -> FileTreeOptions {
        options(self.max_depth, self.max_entries_per_directory)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CopyFileTreeEntryParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) path: String,
    pub(crate) target_directory_path: String,
    max_depth: Option<usize>,
    max_entries_per_directory: Option<usize>,
}

impl CopyFileTreeEntryParams {
    pub(crate) fn options(&self) -> FileTreeOptions {
        options(self.max_depth, self.max_entries_per_directory)
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum FileTreeCreateKindPayload {
    File,
    Directory,
}

impl From<FileTreeCreateKindPayload> for FileTreeCreateKind {
    fn from(value: FileTreeCreateKindPayload) -> Self {
        match value {
            FileTreeCreateKindPayload::File => Self::File,
            FileTreeCreateKindPayload::Directory => Self::Directory,
        }
    }
}

fn options(max_depth: Option<usize>, max_entries_per_directory: Option<usize>) -> FileTreeOptions {
    let defaults = FileTreeOptions::default();

    FileTreeOptions::new(
        max_depth.unwrap_or(defaults.max_depth()),
        max_entries_per_directory.unwrap_or(defaults.max_entries_per_directory()),
    )
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileTreeSnapshot {
    root: FileTreeEntrySnapshot,
}

impl FileTreeSnapshot {
    pub(crate) fn from_tree(tree: &FileTree) -> Self {
        Self {
            root: FileTreeEntrySnapshot::from_entry(tree.root()),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileTreeEntrySnapshot {
    name: String,
    path: String,
    kind: FileTreeEntryKindPayload,
    children: Vec<FileTreeEntrySnapshot>,
    children_truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    read_error: Option<String>,
}

impl FileTreeEntrySnapshot {
    fn from_entry(entry: &FileTreeEntry) -> Self {
        Self {
            name: entry.name().to_owned(),
            path: entry.path().to_string_lossy().into_owned(),
            kind: entry.kind().into(),
            children: entry.children().iter().map(Self::from_entry).collect(),
            children_truncated: entry.children_truncated(),
            read_error: entry.read_error().map(str::to_owned),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
enum FileTreeEntryKindPayload {
    Directory,
    File,
    Symlink,
    Other,
}

impl From<FileTreeEntryKind> for FileTreeEntryKindPayload {
    fn from(value: FileTreeEntryKind) -> Self {
        match value {
            FileTreeEntryKind::Directory => Self::Directory,
            FileTreeEntryKind::File => Self::File,
            FileTreeEntryKind::Symlink => Self::Symlink,
            FileTreeEntryKind::Other => Self::Other,
        }
    }
}
