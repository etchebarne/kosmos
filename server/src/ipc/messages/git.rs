use core::tabs::git::{
    GitBranch, GitChange, GitChangeKind, GitDiff, GitDiffFile, GitDiffSection, GitDiffSectionKind,
    GitRemote, GitRepositorySnapshot, GitStash, GitTag,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ids::{TabIdParam, WorkspaceIdParam};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitTabParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitPathsParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) paths: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenGitDiffTabParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SaveGitDiffFileParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) path: String,
    pub(crate) content: String,
    pub(crate) stage: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommitGitChangesParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) message: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SwitchGitBranchParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) branch: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateGitBranchParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) name: String,
    pub(crate) start_point: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PullGitChangesParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) rebase: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PushGitChangesParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) force: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitStashParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) selector: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitRemoteParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AddGitRemoteParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) name: String,
    pub(crate) url: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitTagParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) name: String,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitRepositorySnapshotPayload {
    repository_root: String,
    branch: Option<String>,
    upstream: Option<String>,
    latest_commit: Option<String>,
    ahead: u32,
    behind: u32,
    insertions: u32,
    deletions: u32,
    branches: Vec<GitBranchPayload>,
    changes: Vec<GitChangePayload>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitStashPayload {
    selector: String,
    commit: String,
    timestamp: i64,
    message: String,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitRemotePayload {
    name: String,
    fetch_urls: Vec<String>,
    push_urls: Vec<String>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitTagPayload {
    name: String,
    target: String,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitDiffPayload {
    focused_path: Option<String>,
    files: Vec<GitDiffFilePayload>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitDiffFilePayload {
    path: String,
    original_path: Option<String>,
    staged: Option<GitChangeKindPayload>,
    unstaged: Option<GitChangeKindPayload>,
    sections: Vec<GitDiffSectionPayload>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitDiffSectionPayload {
    kind: GitDiffSectionKindPayload,
    original_content: Option<String>,
    modified_content: Option<String>,
    editable: bool,
}

#[derive(Clone, Copy, Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum GitDiffSectionKindPayload {
    Staged,
    Unstaged,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitBranchPayload {
    name: String,
    current: bool,
    remote: bool,
    upstream: Option<String>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitChangePayload {
    path: String,
    original_path: Option<String>,
    staged: Option<GitChangeKindPayload>,
    unstaged: Option<GitChangeKindPayload>,
    is_staged: bool,
    is_unstaged: bool,
}

#[derive(Clone, Copy, Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum GitChangeKindPayload {
    Added,
    Conflicted,
    Deleted,
    Ignored,
    Modified,
    Renamed,
    Untracked,
}

impl GitRepositorySnapshotPayload {
    pub(crate) fn from_snapshot(snapshot: &GitRepositorySnapshot) -> Self {
        Self {
            repository_root: snapshot.repository_root().to_string_lossy().into_owned(),
            branch: snapshot.branch().map(ToOwned::to_owned),
            upstream: snapshot.upstream().map(ToOwned::to_owned),
            latest_commit: snapshot.latest_commit().map(ToOwned::to_owned),
            ahead: snapshot.ahead(),
            behind: snapshot.behind(),
            insertions: snapshot.insertions(),
            deletions: snapshot.deletions(),
            branches: snapshot
                .branches()
                .iter()
                .map(GitBranchPayload::from_branch)
                .collect(),
            changes: snapshot
                .changes()
                .iter()
                .map(GitChangePayload::from_change)
                .collect(),
        }
    }
}

impl GitBranchPayload {
    fn from_branch(branch: &GitBranch) -> Self {
        Self {
            name: branch.name().to_owned(),
            current: branch.current(),
            remote: branch.remote(),
            upstream: branch.upstream().map(ToOwned::to_owned),
        }
    }
}

impl GitChangePayload {
    fn from_change(change: &GitChange) -> Self {
        Self {
            path: change.path().to_owned(),
            original_path: change.original_path().map(ToOwned::to_owned),
            staged: change.staged().map(Into::into),
            unstaged: change.unstaged().map(Into::into),
            is_staged: change.is_staged(),
            is_unstaged: change.is_unstaged(),
        }
    }
}

impl GitStashPayload {
    pub(crate) fn from_stash(stash: &GitStash) -> Self {
        Self {
            selector: stash.selector().to_owned(),
            commit: stash.commit().to_owned(),
            timestamp: stash.timestamp(),
            message: stash.message().to_owned(),
        }
    }
}

impl GitRemotePayload {
    pub(crate) fn from_remote(remote: &GitRemote) -> Self {
        Self {
            name: remote.name().to_owned(),
            fetch_urls: remote.fetch_urls().to_vec(),
            push_urls: remote.push_urls().to_vec(),
        }
    }
}

impl GitTagPayload {
    pub(crate) fn from_tag(tag: &GitTag) -> Self {
        Self {
            name: tag.name().to_owned(),
            target: tag.target().to_owned(),
        }
    }
}

impl GitDiffPayload {
    pub(crate) fn from_diff(diff: &GitDiff) -> Self {
        Self {
            focused_path: diff.focused_path().map(ToOwned::to_owned),
            files: diff
                .files()
                .iter()
                .map(GitDiffFilePayload::from_file)
                .collect(),
        }
    }
}

impl GitDiffFilePayload {
    fn from_file(file: &GitDiffFile) -> Self {
        Self {
            path: file.path().to_owned(),
            original_path: file.original_path().map(ToOwned::to_owned),
            staged: file.staged().map(Into::into),
            unstaged: file.unstaged().map(Into::into),
            sections: file
                .sections()
                .iter()
                .map(GitDiffSectionPayload::from_section)
                .collect(),
        }
    }
}

impl GitDiffSectionPayload {
    fn from_section(section: &GitDiffSection) -> Self {
        Self {
            kind: section.kind().into(),
            original_content: section.original_content().map(ToOwned::to_owned),
            modified_content: section.modified_content().map(ToOwned::to_owned),
            editable: section.editable(),
        }
    }
}

impl From<GitChangeKind> for GitChangeKindPayload {
    fn from(kind: GitChangeKind) -> Self {
        match kind {
            GitChangeKind::Added => Self::Added,
            GitChangeKind::Conflicted => Self::Conflicted,
            GitChangeKind::Deleted => Self::Deleted,
            GitChangeKind::Ignored => Self::Ignored,
            GitChangeKind::Modified => Self::Modified,
            GitChangeKind::Renamed => Self::Renamed,
            GitChangeKind::Untracked => Self::Untracked,
        }
    }
}

impl From<GitDiffSectionKind> for GitDiffSectionKindPayload {
    fn from(kind: GitDiffSectionKind) -> Self {
        match kind {
            GitDiffSectionKind::Staged => Self::Staged,
            GitDiffSectionKind::Unstaged => Self::Unstaged,
        }
    }
}
