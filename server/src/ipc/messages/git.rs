use core::git::{GitBranch, GitChange, GitChangeKind, GitRepositorySnapshot, GitStash};
use serde::{Deserialize, Serialize};

use super::pane::WorkspaceIdParam;
use super::tab::TabIdParam;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitTabParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitPathsParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommitGitChangesParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SwitchGitBranchParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) branch: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PullGitChangesParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) rebase: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PushGitChangesParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) force: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitStashParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) selector: String,
}

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitStashPayload {
    selector: String,
    commit: String,
    timestamp: i64,
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GitBranchPayload {
    name: String,
    current: bool,
    upstream: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GitChangePayload {
    path: String,
    original_path: Option<String>,
    staged: Option<GitChangeKindPayload>,
    unstaged: Option<GitChangeKindPayload>,
    is_staged: bool,
    is_unstaged: bool,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
enum GitChangeKindPayload {
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
