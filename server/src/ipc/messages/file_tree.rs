use std::path::Path;

use core::tabs::file_tree::{FileTree, FileTreeDirectory, FileTreeEntryKind, FileTreeError};
use core::tabs::git::FileTreeGitDecorations;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::git::GitChangeKindPayload;
use super::ids::{TabIdParam, WorkspaceIdParam};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GetFileTreeParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: Option<TabIdParam>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GetFileTreeChildrenParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GetFileTreeGitStatusParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetFileTreeExpandedPathsParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) expanded_paths: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateFileTreeEntryParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) parent_path: Option<String>,
    pub(crate) name: String,
    pub(crate) kind: FileTreeEntryKindParam,
}

#[derive(Debug, Deserialize, JsonSchema)]
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RenameFileTreeEntryParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) source_path: String,
    pub(crate) destination_path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TransferFileTreeEntriesParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) source_paths: Vec<String>,
    pub(crate) target_directory_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteFileTreeEntriesParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) paths: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolveFileTreePathParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) path: Option<String>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileTreeSnapshot {
    root: String,
    root_path: String,
    paths: Vec<String>,
    expanded_paths: Vec<String>,
    deferred_paths: Vec<String>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileTreeChildrenSnapshot {
    paths: Vec<String>,
    deferred_paths: Vec<String>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileTreeGitStatusSnapshot {
    entries: Vec<FileTreeGitStatusEntry>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileTreeGitStatusEntry {
    path: String,
    staged: Option<GitChangeKindPayload>,
    unstaged: Option<GitChangeKindPayload>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileTreeResolvedPath {
    path: String,
}

pub(crate) struct FileTreePathMapper {
    root_path: String,
}

impl FileTreeSnapshot {
    pub(crate) fn from_file_tree(file_tree: &FileTree) -> Self {
        let mapper = FileTreePathMapper::new(file_tree.root());

        Self {
            root: file_tree.root().to_string_lossy().into_owned(),
            root_path: mapper.root_path().to_owned(),
            paths: std::iter::once(mapper.root_path().to_owned())
                .chain(file_tree.paths().iter().map(|path| mapper.tree_path(path)))
                .collect(),
            expanded_paths: std::iter::once(mapper.root_path().to_owned())
                .chain(
                    file_tree
                        .expanded_paths()
                        .iter()
                        .map(|path| mapper.tree_path(path)),
                )
                .collect(),
            deferred_paths: file_tree
                .deferred_paths()
                .iter()
                .map(|path| mapper.tree_path(path))
                .collect(),
        }
    }
}

impl FileTreeChildrenSnapshot {
    pub(crate) fn from_directory(
        directory: &FileTreeDirectory,
        mapper: &FileTreePathMapper,
    ) -> Self {
        Self {
            paths: directory
                .paths()
                .iter()
                .map(|path| mapper.tree_path(path))
                .collect(),
            deferred_paths: directory
                .deferred_paths()
                .iter()
                .map(|path| mapper.tree_path(path))
                .collect(),
        }
    }
}

impl FileTreeGitStatusSnapshot {
    pub(crate) fn from_decorations(
        decorations: &FileTreeGitDecorations,
        mapper: &FileTreePathMapper,
    ) -> Self {
        Self {
            entries: decorations
                .entries()
                .iter()
                .map(|decoration| FileTreeGitStatusEntry {
                    path: mapper.tree_path(decoration.path()),
                    staged: decoration.staged().map(Into::into),
                    unstaged: decoration.unstaged().map(Into::into),
                })
                .collect(),
        }
    }
}

impl FileTreePathMapper {
    pub(crate) fn new(root: &Path) -> Self {
        let name = root
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("workspace");

        Self {
            root_path: format!("{name}/"),
        }
    }

    pub(crate) fn root_path(&self) -> &str {
        &self.root_path
    }

    pub(crate) fn tree_path(&self, relative_path: &str) -> String {
        format!("{}{relative_path}", self.root_path)
    }

    pub(crate) fn relative_path(&self, path: &str) -> Result<Option<String>, FileTreeError> {
        if path == self.root_path || path == self.root_path.trim_end_matches('/') {
            return Ok(None);
        }

        path.strip_prefix(&self.root_path)
            .filter(|path| !path.is_empty())
            .map(|path| Some(path.to_owned()))
            .ok_or_else(|| FileTreeError::InvalidPath(path.to_owned()))
    }

    pub(crate) fn relative_entry_path(&self, path: &str) -> Result<String, FileTreeError> {
        self.relative_path(path)?
            .ok_or_else(|| FileTreeError::InvalidPath(path.to_owned()))
    }
}

impl FileTreeResolvedPath {
    pub(crate) fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_between_workspace_relative_and_rooted_tree_paths() {
        let mapper = FileTreePathMapper::new(Path::new("/home/user/kosmos"));

        assert_eq!(mapper.root_path(), "kosmos/");
        assert_eq!(mapper.tree_path("src/main.rs"), "kosmos/src/main.rs");
        assert_eq!(mapper.relative_path("kosmos/").unwrap(), None);
        assert_eq!(
            mapper.relative_path("kosmos/src/main.rs").unwrap(),
            Some("src/main.rs".to_owned())
        );
        assert!(mapper.relative_path("other/src/main.rs").is_err());
        assert!(mapper.relative_entry_path("kosmos/").is_err());
    }

    #[test]
    fn serializes_both_git_statuses_with_rooted_paths() {
        let root = test_directory("git-status");
        core::tabs::git::GitRepository::init(&root).expect("repository should initialize");
        std::fs::write(root.join("document.txt"), "staged\n").expect("file should be written");
        core::tabs::git::GitRepository::stage_paths(&root, &["document.txt".to_owned()])
            .expect("file should be staged");
        std::fs::write(root.join("document.txt"), "unstaged\n").expect("file should be changed");
        let mut state = core::State::new();
        let workspace_id = state.open_workspace(&root);
        assert!(state.set_tab_kind(
            Some(workspace_id),
            core::tree::PaneId::new(1),
            core::tree::TabId::new(1),
            core::tree::TabKind::FileTree,
        ));
        let decorations = state
            .file_tree_git_decorations(Some(workspace_id), core::tree::TabId::new(1))
            .expect("decorations should load");
        let snapshot = FileTreeGitStatusSnapshot::from_decorations(
            &decorations,
            &FileTreePathMapper::new(&root),
        );
        let root_name = root.file_name().unwrap().to_string_lossy();

        assert_eq!(
            serde_json::to_value(snapshot).unwrap(),
            serde_json::json!({
                "entries": [{
                    "path": format!("{root_name}/document.txt"),
                    "staged": "added",
                    "unstaged": "modified",
                }],
            })
        );

        let _ = std::fs::remove_dir_all(root);
    }

    fn test_directory(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kosmos-server-file-tree-{name}-{}-{nanos}",
            std::process::id()
        ));

        std::fs::create_dir_all(&root).expect("test root should be created");

        root
    }
}
