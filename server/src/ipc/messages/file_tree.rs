use core::file_tree::{FileTree, FileTreeEntry, FileTreeEntryKind, FileTreeOptions};
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
