use std::error::Error as StdError;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use crate::tabs::editor::{EditorError, MAX_EDITOR_FILE_BYTES, save_document};
use crate::tree::{TabId, WorkspaceId};

pub type Result<T> = std::result::Result<T, GitError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitRepositorySnapshot {
    repository_root: PathBuf,
    branch: Option<String>,
    upstream: Option<String>,
    latest_commit: Option<String>,
    ahead: u32,
    behind: u32,
    insertions: u32,
    deletions: u32,
    branches: Vec<GitBranch>,
    changes: Vec<GitChange>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitBranch {
    name: String,
    current: bool,
    remote: bool,
    upstream: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitChange {
    path: String,
    original_path: Option<String>,
    staged: Option<GitChangeKind>,
    unstaged: Option<GitChangeKind>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FileTreeGitDecorations {
    entries: Vec<FileTreeGitDecoration>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileTreeGitDecoration {
    path: String,
    staged: Option<GitChangeKind>,
    unstaged: Option<GitChangeKind>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitStash {
    selector: String,
    commit: String,
    timestamp: i64,
    message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitRemote {
    name: String,
    fetch_urls: Vec<String>,
    push_urls: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitTag {
    name: String,
    target: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitDiff {
    focused_path: Option<String>,
    files: Vec<GitDiffFile>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitDiffFile {
    path: String,
    original_path: Option<String>,
    staged: Option<GitChangeKind>,
    unstaged: Option<GitChangeKind>,
    sections: Vec<GitDiffSection>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitDiffSection {
    kind: GitDiffSectionKind,
    original_content: Option<String>,
    modified_content: Option<String>,
    editable: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GitLineHunk {
    old_start: u32,
    old_lines: u32,
    new_start: u32,
    new_lines: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitDiffSectionKind {
    Staged,
    Unstaged,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitDiffViewState {
    workspace_id: WorkspaceId,
    tab_id: TabId,
    path: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitChangeKind {
    Added,
    Conflicted,
    Deleted,
    Ignored,
    Modified,
    Renamed,
    Untracked,
}

pub struct GitRepository;

impl GitRepository {
    pub fn init(directory: impl AsRef<Path>) -> Result<()> {
        git(directory.as_ref(), ["init"]).map(|_| ())
    }

    pub fn snapshot(directory: impl AsRef<Path>) -> Result<GitRepositorySnapshot> {
        let repository_root = repository_root(directory.as_ref())?;
        let status = parse_status(&git(
            &repository_root,
            ["status", "--porcelain=v1", "-z", "--branch"],
        )?)?;
        let branches = parse_branches(&git(
            &repository_root,
            [
                "for-each-ref",
                "--format=%(refname:short)%00%(HEAD)%00%(upstream:short)%00%(refname)",
                "refs/heads",
                "refs/remotes",
            ],
        )?);
        let diff_stats = diff_stats(&repository_root, &status.changes)?;
        let latest_commit = latest_commit(&repository_root)?;

        Ok(GitRepositorySnapshot {
            repository_root,
            branch: status.branch,
            upstream: status.upstream,
            latest_commit,
            ahead: status.ahead,
            behind: status.behind,
            insertions: diff_stats.insertions,
            deletions: diff_stats.deletions,
            branches,
            changes: status.changes,
        })
    }

    pub fn workspace_changes(directory: impl AsRef<Path>) -> Result<Vec<GitChange>> {
        let (repository_root, workspace_prefix) = workspace_repository(directory.as_ref())?;
        let status = parse_status(&git(
            &repository_root,
            [
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=all",
                "--ignored=matching",
            ],
        )?)?;

        Ok(status
            .changes
            .into_iter()
            .filter_map(|change| change.without_prefix(&workspace_prefix))
            .collect())
    }

    pub fn file_line_hunks(
        directory: impl AsRef<Path>,
        workspace_relative_path: &str,
    ) -> Result<Vec<GitLineHunk>> {
        let workspace_relative_path = normalize_path(workspace_relative_path)?;
        let (repository_root, workspace_prefix) = workspace_repository(directory.as_ref())?;
        let repository_path = prefixed_git_path(&workspace_prefix, &workspace_relative_path);
        let status = parse_status(&git(
            &repository_root,
            ["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        )?)?;
        let Some(change) = status
            .changes
            .iter()
            .find(|change| change.path() == repository_path)
        else {
            return Ok(Vec::new());
        };
        let current_line_count = working_tree_content(&repository_root, &repository_path)?
            .0
            .as_deref()
            .map(|content| line_count(content.as_bytes()))
            .unwrap_or(0);

        if change.staged() == Some(GitChangeKind::Added)
            || change.unstaged() == Some(GitChangeKind::Untracked)
            || !has_head(&repository_root)?
        {
            return Ok((current_line_count > 0)
                .then(|| GitLineHunk::new(0, 0, 1, current_line_count))
                .into_iter()
                .collect());
        }

        if change.staged() == Some(GitChangeKind::Conflicted)
            || change.unstaged() == Some(GitChangeKind::Conflicted)
        {
            return Ok((current_line_count > 0)
                .then(|| GitLineHunk::new(1, current_line_count, 1, current_line_count))
                .into_iter()
                .collect());
        }

        parse_line_hunks(&git_with_paths(
            &repository_root,
            ["diff", "--no-ext-diff", "--unified=0", "HEAD", "--"],
            &[repository_path],
        )?)
    }

    pub fn diff(directory: impl AsRef<Path>, focused_path: &str) -> Result<GitDiff> {
        let repository_root = repository_root(directory.as_ref())?;
        let focused_path = normalize_path(focused_path)?;
        let status = parse_status(&git(
            &repository_root,
            ["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        )?)?;

        Ok(GitDiff {
            focused_path: Some(focused_path),
            files: status
                .changes
                .iter()
                .map(|change| diff_file(&repository_root, change))
                .collect::<Result<Vec<_>>>()?,
        })
    }

    pub fn save_diff_file(
        directory: impl AsRef<Path>,
        path: &str,
        content: &str,
        stage: bool,
    ) -> Result<()> {
        let repository_root = repository_root(directory.as_ref())?;
        let path = normalize_path(path)?;
        let status = parse_status(&git(
            &repository_root,
            ["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        )?)?;
        let change = status
            .changes
            .iter()
            .find(|change| change.path() == path)
            .ok_or_else(|| GitError::InvalidPath(path.clone()))?;

        if !change.is_unstaged() || change.unstaged() == Some(GitChangeKind::Deleted) {
            return Err(GitError::InvalidPath(path));
        }

        save_document(&repository_root, &path, content)?;

        if stage {
            Self::stage_paths(&repository_root, &[path])?;
        }

        Ok(())
    }

    pub fn normalize_path(path: &str) -> Result<String> {
        normalize_path(path)
    }

    pub fn stage_paths(directory: impl AsRef<Path>, paths: &[String]) -> Result<()> {
        let repository_root = repository_root(directory.as_ref())?;
        let paths = normalize_paths(paths)?;
        git_with_paths(&repository_root, ["add", "--"], &paths).map(|_| ())
    }

    pub fn unstage_paths(directory: impl AsRef<Path>, paths: &[String]) -> Result<()> {
        let repository_root = repository_root(directory.as_ref())?;
        let paths = normalize_paths(paths)?;
        git_with_paths(&repository_root, ["reset", "HEAD", "--"], &paths).map(|_| ())
    }

    pub fn stage_all(directory: impl AsRef<Path>) -> Result<()> {
        let repository_root = repository_root(directory.as_ref())?;
        git(&repository_root, ["add", "--all"]).map(|_| ())
    }

    pub fn unstage_all(directory: impl AsRef<Path>) -> Result<()> {
        let repository_root = repository_root(directory.as_ref())?;
        git(&repository_root, ["reset", "HEAD", "--"]).map(|_| ())
    }

    pub fn commit(directory: impl AsRef<Path>, message: &str) -> Result<()> {
        let repository_root = repository_root(directory.as_ref())?;
        let message = normalize_commit_message(message)?;
        git(&repository_root, ["commit", "--message", message]).map(|_| ())
    }

    pub fn switch_branch(directory: impl AsRef<Path>, branch: &str) -> Result<()> {
        let repository_root = repository_root(directory.as_ref())?;
        let branch = normalize_branch_name(branch)?;
        git(&repository_root, ["switch", branch]).map(|_| ())
    }

    pub fn track_remote_branch(directory: impl AsRef<Path>, branch: &str) -> Result<()> {
        let repository_root = repository_root(directory.as_ref())?;
        let branch = normalize_branch_name(branch)?;

        git(&repository_root, ["switch", "--track", branch]).map(|_| ())
    }

    pub fn create_branch(directory: impl AsRef<Path>, name: &str, start_point: &str) -> Result<()> {
        let repository_root = repository_root(directory.as_ref())?;
        let name = normalize_branch_name(name)?;
        let start_point = normalize_branch_name(start_point)?;

        git(&repository_root, ["switch", "--create", name, start_point]).map(|_| ())
    }

    pub fn delete_branch(directory: impl AsRef<Path>, branch: &str) -> Result<()> {
        let repository_root = repository_root(directory.as_ref())?;
        let branch = normalize_branch_name(branch)?;

        git(&repository_root, ["branch", "--delete", branch]).map(|_| ())
    }

    pub fn fetch(directory: impl AsRef<Path>) -> Result<()> {
        let repository_root = repository_root(directory.as_ref())?;
        git(&repository_root, ["fetch", "--prune"]).map(|_| ())
    }

    pub fn pull(directory: impl AsRef<Path>, rebase: bool) -> Result<()> {
        let repository_root = repository_root(directory.as_ref())?;
        if rebase {
            git(&repository_root, ["pull", "--rebase"]).map(|_| ())
        } else {
            git(&repository_root, ["pull"]).map(|_| ())
        }
    }

    pub fn push(directory: impl AsRef<Path>, force: bool) -> Result<()> {
        let repository_root = repository_root(directory.as_ref())?;
        if force {
            git(&repository_root, ["push", "--force-with-lease"]).map(|_| ())
        } else {
            git(&repository_root, ["push"]).map(|_| ())
        }
    }

    pub fn stash(directory: impl AsRef<Path>) -> Result<()> {
        let repository_root = repository_root(directory.as_ref())?;
        git(&repository_root, ["stash", "push", "--include-untracked"]).map(|_| ())
    }

    pub fn stash_staged_changes(directory: impl AsRef<Path>) -> Result<()> {
        let repository_root = repository_root(directory.as_ref())?;
        let staged_paths = staged_paths(&repository_root)?;

        if staged_paths.is_empty() {
            return Ok(());
        }

        git(
            &repository_root,
            ["stash", "push", "--staged", "--message", "Staged changes"],
        )
        .map(|_| ())
    }

    pub fn stashes(directory: impl AsRef<Path>) -> Result<Vec<GitStash>> {
        let repository_root = repository_root(directory.as_ref())?;
        let bytes = git(
            &repository_root,
            ["stash", "list", "--format=%gd%x00%H%x00%ct%x00%gs%x1e"],
        )?;

        parse_stashes(&bytes)
    }

    pub fn apply_stash(directory: impl AsRef<Path>, selector: &str) -> Result<()> {
        let repository_root = repository_root(directory.as_ref())?;
        let selector = normalize_stash_selector(selector)?;

        git(&repository_root, ["stash", "apply", selector]).map(|_| ())
    }

    pub fn drop_stash(directory: impl AsRef<Path>, selector: &str) -> Result<()> {
        let repository_root = repository_root(directory.as_ref())?;
        let selector = normalize_stash_selector(selector)?;

        git(&repository_root, ["stash", "drop", selector]).map(|_| ())
    }

    pub fn remotes(directory: impl AsRef<Path>) -> Result<Vec<GitRemote>> {
        let repository_root = repository_root(directory.as_ref())?;
        let names = parse_lines(&git(&repository_root, ["remote"])?)?;

        names
            .into_iter()
            .map(|name| {
                let fetch_urls = remote_urls(&repository_root, &name, false)?;
                let push_urls = remote_urls(&repository_root, &name, true)?;

                Ok(GitRemote {
                    name,
                    fetch_urls,
                    push_urls,
                })
            })
            .collect()
    }

    pub fn add_remote(directory: impl AsRef<Path>, name: &str, url: &str) -> Result<()> {
        let repository_root = repository_root(directory.as_ref())?;
        let name = normalize_remote_name(name)?;
        let url = normalize_remote_url(url)?;

        git(&repository_root, ["remote", "add", "--", name, url]).map(|_| ())
    }

    pub fn remove_remote(directory: impl AsRef<Path>, name: &str) -> Result<()> {
        let repository_root = repository_root(directory.as_ref())?;
        let name = normalize_remote_name(name)?;

        git(&repository_root, ["remote", "remove", "--", name]).map(|_| ())
    }

    pub fn tags(directory: impl AsRef<Path>) -> Result<Vec<GitTag>> {
        let repository_root = repository_root(directory.as_ref())?;
        let bytes = git(
            &repository_root,
            [
                "tag",
                "--list",
                "--sort=-version:refname",
                "--format=%(refname:short)%00%(if)%(*objectname)%(then)%(*objectname:short)%(else)%(objectname:short)%(end)",
            ],
        )?;

        parse_tags(&bytes)
    }

    pub fn create_tag(directory: impl AsRef<Path>, name: &str) -> Result<()> {
        let repository_root = repository_root(directory.as_ref())?;
        let name = normalize_tag_name(name)?;

        git(&repository_root, ["tag", "--", name]).map(|_| ())
    }

    pub fn delete_tag(directory: impl AsRef<Path>, name: &str) -> Result<()> {
        let repository_root = repository_root(directory.as_ref())?;
        let name = normalize_tag_name(name)?;

        git(&repository_root, ["tag", "--delete", "--", name]).map(|_| ())
    }

    pub fn discard_all_changes(directory: impl AsRef<Path>) -> Result<()> {
        let repository_root = repository_root(directory.as_ref())?;

        if !has_head(&repository_root)? {
            return Ok(());
        }

        git(&repository_root, ["reset", "--hard", "HEAD"])?;
        git(&repository_root, ["clean", "-fd"]).map(|_| ())
    }

    pub fn discard_staged_changes(directory: impl AsRef<Path>) -> Result<()> {
        let repository_root = repository_root(directory.as_ref())?;

        if !has_head(&repository_root)? {
            return Ok(());
        }

        let staged_paths = staged_paths(&repository_root)?;

        if staged_paths.is_empty() {
            return Ok(());
        }

        git_with_paths(
            &repository_root,
            ["restore", "--staged", "--worktree", "--"],
            &staged_paths,
        )
        .map(|_| ())
    }
}

impl GitRepositorySnapshot {
    pub fn repository_root(&self) -> &Path {
        &self.repository_root
    }

    pub fn branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }

    pub fn upstream(&self) -> Option<&str> {
        self.upstream.as_deref()
    }

    pub fn latest_commit(&self) -> Option<&str> {
        self.latest_commit.as_deref()
    }

    pub fn ahead(&self) -> u32 {
        self.ahead
    }

    pub fn behind(&self) -> u32 {
        self.behind
    }

    pub fn insertions(&self) -> u32 {
        self.insertions
    }

    pub fn deletions(&self) -> u32 {
        self.deletions
    }

    pub fn branches(&self) -> &[GitBranch] {
        &self.branches
    }

    pub fn changes(&self) -> &[GitChange] {
        &self.changes
    }
}

impl GitBranch {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn current(&self) -> bool {
        self.current
    }

    pub fn remote(&self) -> bool {
        self.remote
    }

    pub fn upstream(&self) -> Option<&str> {
        self.upstream.as_deref()
    }
}

impl GitChange {
    fn without_prefix(mut self, prefix: &str) -> Option<Self> {
        self.path = strip_git_path_prefix(&self.path, prefix)?.to_owned();
        self.original_path = self
            .original_path
            .as_deref()
            .and_then(|path| strip_git_path_prefix(path, prefix))
            .map(ToOwned::to_owned);

        Some(self)
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn original_path(&self) -> Option<&str> {
        self.original_path.as_deref()
    }

    pub fn staged(&self) -> Option<GitChangeKind> {
        self.staged
    }

    pub fn unstaged(&self) -> Option<GitChangeKind> {
        self.unstaged
    }

    pub fn is_staged(&self) -> bool {
        self.staged.is_some()
    }

    pub fn is_unstaged(&self) -> bool {
        self.unstaged.is_some()
    }
}

impl FileTreeGitDecorations {
    pub(crate) fn from_changes(changes: Vec<GitChange>) -> Self {
        Self {
            entries: changes
                .into_iter()
                .map(|change| FileTreeGitDecoration {
                    path: change.path,
                    staged: change.staged,
                    unstaged: change.unstaged,
                })
                .collect(),
        }
    }

    pub fn entries(&self) -> &[FileTreeGitDecoration] {
        &self.entries
    }
}

impl FileTreeGitDecoration {
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn staged(&self) -> Option<GitChangeKind> {
        self.staged
    }

    pub fn unstaged(&self) -> Option<GitChangeKind> {
        self.unstaged
    }
}

impl GitStash {
    pub fn selector(&self) -> &str {
        &self.selector
    }

    pub fn commit(&self) -> &str {
        &self.commit
    }

    pub fn timestamp(&self) -> i64 {
        self.timestamp
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl GitRemote {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn fetch_urls(&self) -> &[String] {
        &self.fetch_urls
    }

    pub fn push_urls(&self) -> &[String] {
        &self.push_urls
    }
}

impl GitTag {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn target(&self) -> &str {
        &self.target
    }
}

impl GitDiff {
    pub fn focused_path(&self) -> Option<&str> {
        self.focused_path.as_deref()
    }

    pub fn files(&self) -> &[GitDiffFile] {
        &self.files
    }
}

impl GitDiffFile {
    fn new(
        path: impl Into<String>,
        original_path: Option<String>,
        staged: Option<GitChangeKind>,
        unstaged: Option<GitChangeKind>,
        sections: Vec<GitDiffSection>,
    ) -> Self {
        Self {
            path: path.into(),
            original_path,
            staged,
            unstaged,
            sections,
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn original_path(&self) -> Option<&str> {
        self.original_path.as_deref()
    }

    pub fn staged(&self) -> Option<GitChangeKind> {
        self.staged
    }

    pub fn unstaged(&self) -> Option<GitChangeKind> {
        self.unstaged
    }

    pub fn sections(&self) -> &[GitDiffSection] {
        &self.sections
    }
}

impl GitDiffSection {
    fn new(
        kind: GitDiffSectionKind,
        original_content: Option<String>,
        modified_content: Option<String>,
        editable: bool,
    ) -> Self {
        Self {
            kind,
            original_content,
            modified_content,
            editable,
        }
    }

    pub fn kind(&self) -> GitDiffSectionKind {
        self.kind
    }

    pub fn original_content(&self) -> Option<&str> {
        self.original_content.as_deref()
    }

    pub fn modified_content(&self) -> Option<&str> {
        self.modified_content.as_deref()
    }

    pub fn editable(&self) -> bool {
        self.editable
    }
}

impl GitLineHunk {
    fn new(old_start: u32, old_lines: u32, new_start: u32, new_lines: u32) -> Self {
        Self {
            old_start,
            old_lines,
            new_start,
            new_lines,
        }
    }

    pub fn old_start(&self) -> u32 {
        self.old_start
    }

    pub fn old_lines(&self) -> u32 {
        self.old_lines
    }

    pub fn new_start(&self) -> u32 {
        self.new_start
    }

    pub fn new_lines(&self) -> u32 {
        self.new_lines
    }
}

impl GitDiffViewState {
    pub fn new(workspace_id: WorkspaceId, tab_id: TabId, path: impl Into<String>) -> Self {
        Self {
            workspace_id,
            tab_id,
            path: path.into(),
        }
    }

    pub fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }

    pub fn tab_id(&self) -> TabId {
        self.tab_id
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn set_path(&mut self, path: impl Into<String>) {
        self.path = path.into();
    }
}

#[derive(Debug)]
pub enum GitError {
    BranchRequired,
    CommandFailed { command: String, stderr: String },
    CommitMessageRequired,
    Discover { directory: PathBuf, message: String },
    InvalidPath(String),
    InvalidStash(String),
    File(EditorError),
    Io(io::Error),
    NotWorktree(PathBuf),
    RemoteNameRequired,
    RemoteUrlRequired,
    TabNotFound,
    TagNameRequired,
    Utf8(String),
    WorkspaceNotFound,
}

impl fmt::Display for GitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BranchRequired => formatter.write_str("branch name is required"),
            Self::CommandFailed { command, stderr } => {
                write!(formatter, "git command failed ({command}): {stderr}")
            }
            Self::CommitMessageRequired => formatter.write_str("commit message is required"),
            Self::Discover { directory, message } => write!(
                formatter,
                "could not find a git repository from {}: {message}",
                directory.display()
            ),
            Self::InvalidPath(path) => write!(formatter, "invalid git path: {path}"),
            Self::InvalidStash(selector) => write!(formatter, "invalid git stash: {selector}"),
            Self::File(error) => error.fmt(formatter),
            Self::Io(error) => write!(formatter, "{error}"),
            Self::NotWorktree(path) => write!(
                formatter,
                "git repository has no worktree: {}",
                path.display()
            ),
            Self::RemoteNameRequired => formatter.write_str("remote name is required"),
            Self::RemoteUrlRequired => formatter.write_str("remote URL is required"),
            Self::TabNotFound => formatter.write_str("git tab does not exist"),
            Self::TagNameRequired => formatter.write_str("tag name is required"),
            Self::Utf8(message) => formatter.write_str(message),
            Self::WorkspaceNotFound => formatter.write_str("workspace does not exist"),
        }
    }
}

impl StdError for GitError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::File(error) => Some(error),
            Self::BranchRequired
            | Self::CommandFailed { .. }
            | Self::CommitMessageRequired
            | Self::Discover { .. }
            | Self::InvalidPath(_)
            | Self::InvalidStash(_)
            | Self::NotWorktree(_)
            | Self::RemoteNameRequired
            | Self::RemoteUrlRequired
            | Self::TabNotFound
            | Self::TagNameRequired
            | Self::Utf8(_)
            | Self::WorkspaceNotFound => None,
        }
    }
}

impl From<io::Error> for GitError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<EditorError> for GitError {
    fn from(error: EditorError) -> Self {
        Self::File(error)
    }
}

#[derive(Default)]
struct ParsedStatus {
    branch: Option<String>,
    upstream: Option<String>,
    ahead: u32,
    behind: u32,
    changes: Vec<GitChange>,
}

#[derive(Default)]
struct GitDiffStats {
    insertions: u32,
    deletions: u32,
}

pub(crate) struct RepositoryWatchPaths {
    pub(crate) metadata: Vec<PathBuf>,
    pub(crate) worktree: PathBuf,
}

pub(crate) fn repository_watch_paths(directory: &Path) -> Result<RepositoryWatchPaths> {
    let repository_root = repository_root(directory)?;
    let output = git(
        &repository_root,
        [
            "rev-parse",
            "--path-format=absolute",
            "--git-dir",
            "--git-common-dir",
        ],
    )?;
    let metadata_paths =
        String::from_utf8(output).map_err(|error| GitError::Utf8(error.to_string()))?;
    let mut metadata = metadata_paths
        .lines()
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .collect::<Vec<_>>();

    metadata.sort();
    metadata.dedup();

    Ok(RepositoryWatchPaths {
        metadata,
        worktree: repository_root,
    })
}

fn repository_root(directory: &Path) -> Result<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(["rev-parse", "--show-toplevel"])
        .output()?;

    if output.status.success() {
        let path =
            String::from_utf8(output.stdout).map_err(|error| GitError::Utf8(error.to_string()))?;
        let path = path.strip_suffix('\n').unwrap_or(&path);

        return if path.is_empty() {
            Err(GitError::Discover {
                directory: directory.to_path_buf(),
                message: "Git returned an empty repository root".to_owned(),
            })
        } else {
            Ok(PathBuf::from(path))
        };
    }

    let bare_output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(["rev-parse", "--is-bare-repository"])
        .output()?;

    if bare_output.status.success() && bare_output.stdout.starts_with(b"true") {
        return Err(GitError::NotWorktree(directory.to_path_buf()));
    }

    Err(GitError::Discover {
        directory: directory.to_path_buf(),
        message: command_stderr(&output.stderr),
    })
}

fn staged_paths(repository_root: &Path) -> Result<Vec<String>> {
    let status = parse_status(&git(repository_root, ["status", "--porcelain=v1", "-z"])?)?;

    Ok(status
        .changes
        .into_iter()
        .filter(|change| change.is_staged())
        .map(|change| change.path)
        .collect())
}

fn parse_status(bytes: &[u8]) -> Result<ParsedStatus> {
    let records = bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .collect::<Vec<_>>();
    let mut status = ParsedStatus::default();
    let mut index = 0;

    while index < records.len() {
        let record = record_to_string(records[index])?;

        if let Some(header) = record.strip_prefix("## ") {
            parse_branch_header(header, &mut status);
            index += 1;
            continue;
        }

        let Some(change) = parse_change_record(&record, &records, &mut index)? else {
            index += 1;
            continue;
        };

        status.changes.push(change);
        index += 1;
    }

    status
        .changes
        .sort_by(|left, right| left.path.cmp(&right.path));

    Ok(status)
}

fn parse_branch_header(header: &str, status: &mut ParsedStatus) {
    let (name_part, tracking_part) = match header.split_once("...") {
        Some((name_part, rest)) => {
            let (tracking_part, summary_part) = split_tracking_summary(rest);
            parse_ahead_behind(summary_part, status);
            (name_part, Some(tracking_part))
        }
        None => {
            let (name_part, summary_part) = split_tracking_summary(header);
            parse_ahead_behind(summary_part, status);
            (name_part, None)
        }
    };

    status.branch = parse_branch_name(name_part);
    status.upstream = tracking_part.and_then(parse_upstream_name);
}

fn split_tracking_summary(value: &str) -> (&str, Option<&str>) {
    match value.split_once(" [") {
        Some((name, summary)) => (name, Some(summary.trim_end_matches(']'))),
        None => (value, None),
    }
}

fn parse_branch_name(value: &str) -> Option<String> {
    let value = value.trim();

    if value.is_empty() || value.starts_with("HEAD") {
        None
    } else if let Some(branch) = value.strip_prefix("No commits yet on ") {
        non_empty_string(branch)
    } else {
        non_empty_string(value)
    }
}

fn parse_upstream_name(value: &str) -> Option<String> {
    non_empty_string(value.trim())
}

fn parse_ahead_behind(summary: Option<&str>, status: &mut ParsedStatus) {
    let Some(summary) = summary else {
        return;
    };

    for part in summary.split(',').map(str::trim) {
        if let Some(ahead) = part.strip_prefix("ahead ").and_then(parse_u32) {
            status.ahead = ahead;
        }

        if let Some(behind) = part.strip_prefix("behind ").and_then(parse_u32) {
            status.behind = behind;
        }
    }
}

fn parse_u32(value: &str) -> Option<u32> {
    value.parse().ok()
}

fn parse_change_record(
    record: &str,
    records: &[&[u8]],
    index: &mut usize,
) -> Result<Option<GitChange>> {
    if record.len() < 4 {
        return Ok(None);
    }

    let bytes = record.as_bytes();
    let staged_code = bytes[0] as char;
    let unstaged_code = bytes[1] as char;
    let path = record[3..].to_owned();
    let original_path = if needs_original_path(staged_code, unstaged_code) {
        let original_index = *index + 1;

        if original_index >= records.len() {
            None
        } else {
            *index = original_index;
            Some(record_to_string(records[original_index])?)
        }
    } else {
        None
    };

    let conflicted = is_conflicted_status(staged_code, unstaged_code);
    let staged = staged_kind(staged_code, unstaged_code, conflicted);
    let unstaged = unstaged_kind(staged_code, unstaged_code, conflicted);

    Ok(Some(GitChange {
        path,
        original_path,
        staged,
        unstaged,
    }))
}

fn staged_kind(staged_code: char, unstaged_code: char, conflicted: bool) -> Option<GitChangeKind> {
    if conflicted {
        return Some(GitChangeKind::Conflicted);
    }

    match (staged_code, unstaged_code) {
        ('?', '?') | ('!', '!') => None,
        (code, _) => tracked_kind(code),
    }
}

fn unstaged_kind(
    staged_code: char,
    unstaged_code: char,
    conflicted: bool,
) -> Option<GitChangeKind> {
    if conflicted {
        return Some(GitChangeKind::Conflicted);
    }

    match (staged_code, unstaged_code) {
        ('?', '?') => Some(GitChangeKind::Untracked),
        ('!', '!') => Some(GitChangeKind::Ignored),
        (_, code) => tracked_kind(code),
    }
}

fn tracked_kind(code: char) -> Option<GitChangeKind> {
    match code {
        'A' => Some(GitChangeKind::Added),
        'C' | 'R' => Some(GitChangeKind::Renamed),
        'D' => Some(GitChangeKind::Deleted),
        'M' | 'T' => Some(GitChangeKind::Modified),
        'U' => Some(GitChangeKind::Conflicted),
        ' ' => None,
        _ => None,
    }
}

fn needs_original_path(staged_code: char, unstaged_code: char) -> bool {
    matches!(staged_code, 'R' | 'C') || matches!(unstaged_code, 'R' | 'C')
}

fn is_conflicted_status(staged_code: char, unstaged_code: char) -> bool {
    matches!(staged_code, 'U' | 'A' | 'D') && matches!(unstaged_code, 'U' | 'A' | 'D')
}

fn parse_branches(bytes: &[u8]) -> Vec<GitBranch> {
    let mut branches = bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .filter_map(parse_branch_line)
        .collect::<Vec<_>>();

    branches.sort_by(|left, right| {
        right
            .current
            .cmp(&left.current)
            .then_with(|| left.name.cmp(&right.name))
    });

    branches
}

fn parse_branch_line(line: &[u8]) -> Option<GitBranch> {
    let mut fields = line.split(|byte| *byte == 0);
    let name = non_empty_string(String::from_utf8_lossy(fields.next()?).as_ref())?;
    let current = fields
        .next()
        .is_some_and(|field| String::from_utf8_lossy(field).trim() == "*");
    let upstream = fields
        .next()
        .and_then(|field| non_empty_string(&String::from_utf8_lossy(field)));
    let refname = fields.next().map(String::from_utf8_lossy)?;
    let remote = refname.starts_with("refs/remotes/");

    if remote && refname.ends_with("/HEAD") {
        return None;
    }

    Some(GitBranch {
        name,
        current,
        remote,
        upstream,
    })
}

fn parse_stashes(bytes: &[u8]) -> Result<Vec<GitStash>> {
    bytes
        .split(|byte| *byte == 0x1e)
        .map(trim_stash_record)
        .filter(|record| !record.is_empty())
        .map(parse_stash_record)
        .collect()
}

fn trim_stash_record(mut record: &[u8]) -> &[u8] {
    while matches!(record.first(), Some(b'\n' | b'\r')) {
        record = &record[1..];
    }

    while matches!(record.last(), Some(b'\n' | b'\r')) {
        record = &record[..record.len() - 1];
    }

    record
}

fn parse_stash_record(record: &[u8]) -> Result<GitStash> {
    let fields = record.split(|byte| *byte == 0).collect::<Vec<_>>();

    if fields.len() < 4 {
        return Err(GitError::InvalidStash(record_to_string(record)?));
    }

    let timestamp = record_to_string(fields[2])?
        .parse::<i64>()
        .map_err(|_| GitError::InvalidStash(record_to_string(record).unwrap_or_default()))?;

    Ok(GitStash {
        selector: record_to_string(fields[0])?,
        commit: record_to_string(fields[1])?,
        timestamp,
        message: record_to_string(fields[3])?,
    })
}

fn remote_urls(repository_root: &Path, name: &str, push: bool) -> Result<Vec<String>> {
    let output = if push {
        git(
            repository_root,
            ["remote", "get-url", "--push", "--all", "--", name],
        )?
    } else {
        git(repository_root, ["remote", "get-url", "--all", "--", name])?
    };

    parse_lines(&output)
}

fn parse_lines(bytes: &[u8]) -> Result<Vec<String>> {
    let output =
        String::from_utf8(bytes.to_vec()).map_err(|error| GitError::Utf8(error.to_string()))?;

    Ok(output
        .lines()
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn parse_tags(bytes: &[u8]) -> Result<Vec<GitTag>> {
    bytes
        .split(|byte| *byte == b'\n')
        .filter(|record| !record.is_empty())
        .map(|record| {
            let mut fields = record.splitn(2, |byte| *byte == 0);
            let name = fields.next().unwrap_or_default();
            let target = fields
                .next()
                .ok_or_else(|| GitError::Utf8("git returned an invalid tag record".to_owned()))?;

            Ok(GitTag {
                name: record_to_string(name)?,
                target: record_to_string(target)?,
            })
        })
        .collect()
}

fn diff_stats(repository_root: &Path, changes: &[GitChange]) -> Result<GitDiffStats> {
    let mut stats = GitDiffStats::default();

    add_numstat(
        &mut stats,
        &git_bytes(repository_root, ["diff", "--numstat"])?,
    );
    add_numstat(
        &mut stats,
        &git_bytes(repository_root, ["diff", "--cached", "--numstat"])?,
    );
    add_untracked_stats(repository_root, changes, &mut stats)?;

    Ok(stats)
}

fn latest_commit(repository_root: &Path) -> Result<Option<String>> {
    if !has_head(repository_root)? {
        return Ok(None);
    }

    git(repository_root, ["log", "-1", "--pretty=%s"])
        .map(|output| non_empty_string(&String::from_utf8_lossy(&output)))
}

fn has_head(repository_root: &Path) -> Result<bool> {
    let args = ["rev-parse", "--verify", "--quiet", "HEAD"];
    let output = Command::new("git")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .arg("-C")
        .arg(repository_root)
        .args(args)
        .output()?;

    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(GitError::CommandFailed {
            command: format!("git {}", args.join(" ")),
            stderr: command_stderr(&output.stderr),
        }),
    }
}

fn add_numstat(stats: &mut GitDiffStats, bytes: &[u8]) {
    for line in bytes.split(|byte| *byte == b'\n') {
        let fields = line.split(|byte| *byte == b'\t').collect::<Vec<_>>();

        if fields.len() < 3 {
            continue;
        }

        stats.insertions = stats
            .insertions
            .saturating_add(parse_numstat_count(fields[0]));
        stats.deletions = stats
            .deletions
            .saturating_add(parse_numstat_count(fields[1]));
    }
}

fn parse_numstat_count(value: &[u8]) -> u32 {
    std::str::from_utf8(value)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0)
}

fn add_untracked_stats(
    repository_root: &Path,
    changes: &[GitChange],
    stats: &mut GitDiffStats,
) -> Result<()> {
    for change in changes {
        if change.unstaged() != Some(GitChangeKind::Untracked) || change.is_staged() {
            continue;
        }

        add_untracked_path_stats(&repository_root.join(change.path()), stats)?;
    }

    Ok(())
}

fn add_untracked_path_stats(path: &Path, stats: &mut GitDiffStats) -> Result<()> {
    let file_type = fs::symlink_metadata(path)
        .map_err(|error| io_error(path, error))?
        .file_type();

    if file_type.is_symlink() {
        let target = fs::read_link(path).map_err(|error| io_error(path, error))?;
        stats.insertions = stats
            .insertions
            .saturating_add(line_count(target.to_string_lossy().as_bytes()));
        return Ok(());
    }

    if file_type.is_dir() {
        for entry in fs::read_dir(path).map_err(|error| io_error(path, error))? {
            let entry = entry.map_err(|error| io_error(path, error))?;
            add_untracked_path_stats(&entry.path(), stats)?;
        }

        return Ok(());
    }

    if file_type.is_file() {
        let bytes = fs::read(path).map_err(|error| io_error(path, error))?;
        stats.insertions = stats.insertions.saturating_add(line_count(&bytes));
    }

    Ok(())
}

fn line_count(bytes: &[u8]) -> u32 {
    if bytes.is_empty() {
        return 0;
    }

    let newline_count = bytes.iter().filter(|byte| **byte == b'\n').count();
    let has_trailing_partial_line = bytes.last().is_some_and(|byte| *byte != b'\n');
    let line_count = newline_count + usize::from(has_trailing_partial_line);

    u32::try_from(line_count).unwrap_or(u32::MAX)
}

fn git<I, S>(repository_root: &Path, args: I) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    git_with_args(repository_root, args, [])
}

fn git_bytes<I, S>(repository_root: &Path, args: I) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    git(repository_root, args)
}

fn diff_file(repository_root: &Path, change: &GitChange) -> Result<GitDiffFile> {
    let mut sections = Vec::new();

    if change.staged() == Some(GitChangeKind::Conflicted) {
        let ours = git_blob_content(repository_root, &format!(":2:{}", change.path()), true)?;
        let (working, editable) = working_tree_content(repository_root, change.path())?;

        sections.push(GitDiffSection::new(
            GitDiffSectionKind::Unstaged,
            ours,
            working,
            editable,
        ));
    } else {
        let original_content = original_content(repository_root, change)?;
        let staged_content = if change.is_staged() {
            let content = index_content(repository_root, change)?;

            sections.push(GitDiffSection::new(
                GitDiffSectionKind::Staged,
                original_content.clone(),
                content.clone(),
                false,
            ));
            content
        } else {
            original_content
        };

        if change.is_unstaged() {
            let (working, editable) = working_tree_content(repository_root, change.path())?;

            sections.push(GitDiffSection::new(
                GitDiffSectionKind::Unstaged,
                staged_content,
                working,
                editable,
            ));
        }
    }

    Ok(GitDiffFile::new(
        change.path(),
        change.original_path().map(ToOwned::to_owned),
        change.staged(),
        change.unstaged(),
        sections,
    ))
}

fn original_content(repository_root: &Path, change: &GitChange) -> Result<Option<String>> {
    if change.staged() == Some(GitChangeKind::Added)
        || change.unstaged() == Some(GitChangeKind::Untracked)
    {
        return Ok(Some(String::new()));
    }

    let path = change.original_path().unwrap_or_else(|| change.path());
    git_blob_content(repository_root, &format!("HEAD:{path}"), false)
}

fn index_content(repository_root: &Path, change: &GitChange) -> Result<Option<String>> {
    if change.staged() == Some(GitChangeKind::Deleted) {
        Ok(Some(String::new()))
    } else {
        git_blob_content(repository_root, &format!(":{}", change.path()), false)
    }
}

fn git_blob_content(
    repository_root: &Path,
    object: &str,
    missing_as_empty: bool,
) -> Result<Option<String>> {
    let output = Command::new("git")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .arg("-C")
        .arg(repository_root)
        .args(["show", "--no-textconv", object])
        .output()?;

    if output.status.success() {
        return Ok(diff_text(output.stdout));
    }

    if missing_as_empty {
        return Ok(Some(String::new()));
    }

    Err(GitError::CommandFailed {
        command: format!("git show --no-textconv {object}"),
        stderr: command_stderr(&output.stderr),
    })
}

fn working_tree_content(repository_root: &Path, path: &str) -> Result<(Option<String>, bool)> {
    let full_path = repository_root.join(path);
    let metadata = match fs::symlink_metadata(&full_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok((Some(String::new()), false));
        }
        Err(error) => return Err(io_error(&full_path, error)),
    };

    if metadata.file_type().is_symlink() {
        let target = fs::read_link(&full_path).map_err(|error| io_error(&full_path, error))?;
        return Ok((Some(target.to_string_lossy().into_owned()), false));
    }

    if !metadata.is_file() || metadata.len() > MAX_EDITOR_FILE_BYTES as u64 {
        return Ok((None, false));
    }

    let bytes = fs::read(&full_path).map_err(|error| io_error(&full_path, error))?;
    let content = diff_text(bytes);
    let editable = content.is_some();

    Ok((content, editable))
}

fn diff_text(bytes: Vec<u8>) -> Option<String> {
    if bytes.len() > MAX_EDITOR_FILE_BYTES {
        None
    } else {
        String::from_utf8(bytes).ok()
    }
}

fn git_with_paths<I, S>(repository_root: &Path, args: I, paths: &[String]) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    git_with_args(repository_root, args, paths.iter().map(OsString::from))
}

fn git_with_args<I, S, P>(repository_root: &Path, args: I, path_args: P) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
    P: IntoIterator<Item = OsString>,
{
    let mut args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_os_string())
        .collect::<Vec<_>>();
    args.extend(path_args);
    let output = Command::new("git")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .arg("-C")
        .arg(repository_root)
        .args(&args)
        .output()?;

    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(GitError::CommandFailed {
            command: format_command(&args),
            stderr: command_stderr(&output.stderr),
        })
    }
}

fn format_command(args: &[OsString]) -> String {
    let args = args
        .iter()
        .map(|arg| arg.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");

    format!("git {args}")
}

fn command_stderr(stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr).trim().to_owned();

    if stderr.is_empty() {
        "unknown error".to_owned()
    } else {
        stderr
    }
}

fn normalize_commit_message(message: &str) -> Result<&str> {
    let message = message.trim();

    if message.is_empty() {
        Err(GitError::CommitMessageRequired)
    } else {
        Ok(message)
    }
}

fn normalize_branch_name(branch: &str) -> Result<&str> {
    let branch = branch.trim();

    if branch.is_empty() || branch.starts_with('-') || branch.contains('\0') {
        Err(GitError::BranchRequired)
    } else {
        Ok(branch)
    }
}

fn normalize_stash_selector(selector: &str) -> Result<&str> {
    let selector = selector.trim();

    if selector.len() <= "stash@{}".len()
        || !selector.starts_with("stash@{")
        || !selector.ends_with('}')
        || !selector[7..selector.len() - 1]
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        return Err(GitError::InvalidStash(selector.to_owned()));
    }

    Ok(selector)
}

fn normalize_remote_name(name: &str) -> Result<&str> {
    let name = name.trim();

    if name.is_empty() || name.starts_with('-') || name.contains('\0') {
        Err(GitError::RemoteNameRequired)
    } else {
        Ok(name)
    }
}

fn normalize_remote_url(url: &str) -> Result<&str> {
    let url = url.trim();

    if url.is_empty() || url.contains('\0') {
        Err(GitError::RemoteUrlRequired)
    } else {
        Ok(url)
    }
}

fn normalize_tag_name(name: &str) -> Result<&str> {
    let name = name.trim();

    if name.is_empty() || name.starts_with('-') || name.contains('\0') {
        Err(GitError::TagNameRequired)
    } else {
        Ok(name)
    }
}

fn normalize_paths(paths: &[String]) -> Result<Vec<String>> {
    if paths.is_empty() {
        return Err(GitError::InvalidPath("empty selection".to_owned()));
    }

    let mut normalized_paths = paths
        .iter()
        .map(|path| normalize_path(path))
        .collect::<Result<Vec<_>>>()?;

    normalized_paths.sort();
    normalized_paths.dedup();

    Ok(normalized_paths)
}

fn workspace_repository(directory: &Path) -> Result<(PathBuf, String)> {
    let directory = fs::canonicalize(directory)?;
    let repository_root = fs::canonicalize(repository_root(&directory)?)?;
    let workspace_prefix =
        directory
            .strip_prefix(&repository_root)
            .map_err(|_| GitError::Discover {
                directory: directory.clone(),
                message: "workspace is outside the repository worktree".to_owned(),
            })?;

    Ok((repository_root, git_path(workspace_prefix)?))
}

fn prefixed_git_path(prefix: &str, path: &str) -> String {
    if prefix.is_empty() {
        path.to_owned()
    } else {
        format!("{prefix}/{path}")
    }
}

fn git_path(path: &Path) -> Result<String> {
    path.components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| GitError::Utf8("git path is not valid UTF-8".to_owned()))
        })
        .collect::<Result<Vec<_>>>()
        .map(|components| components.join("/"))
}

fn strip_git_path_prefix<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    if prefix.is_empty() {
        return Some(path);
    }

    path.strip_prefix(prefix)?
        .strip_prefix('/')
        .filter(|path| !path.is_empty())
}

fn parse_line_hunks(bytes: &[u8]) -> Result<Vec<GitLineHunk>> {
    bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| line.starts_with(b"@@ "))
        .map(parse_line_hunk)
        .collect()
}

fn parse_line_hunk(line: &[u8]) -> Result<GitLineHunk> {
    let line = std::str::from_utf8(line).map_err(|error| GitError::Utf8(error.to_string()))?;
    let ranges = line
        .strip_prefix("@@ -")
        .and_then(|line| line.split_once(" +"))
        .and_then(|(old, rest)| rest.split_once(" @@").map(|(new, _)| (old, new)))
        .ok_or_else(|| GitError::Utf8("git returned an invalid diff hunk".to_owned()))?;
    let (old_start, old_lines) = parse_line_range(ranges.0)?;
    let (new_start, new_lines) = parse_line_range(ranges.1)?;

    Ok(GitLineHunk::new(old_start, old_lines, new_start, new_lines))
}

fn parse_line_range(range: &str) -> Result<(u32, u32)> {
    let (start, lines) = range.split_once(',').unwrap_or((range, "1"));
    let invalid_hunk = || GitError::Utf8("git returned an invalid diff hunk".to_owned());

    Ok((
        start.parse().map_err(|_| invalid_hunk())?,
        lines.parse().map_err(|_| invalid_hunk())?,
    ))
}

fn normalize_path(path: &str) -> Result<String> {
    let path = path.trim();

    if path.is_empty() || path.starts_with('/') || path.contains('\0') {
        return Err(GitError::InvalidPath(path.to_owned()));
    }

    let candidate = Path::new(path.trim_end_matches('/'));

    if candidate.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(GitError::InvalidPath(path.to_owned()));
    }

    Ok(path.to_owned())
}

fn record_to_string(record: &[u8]) -> Result<String> {
    String::from_utf8(record.to_vec()).map_err(|error| GitError::Utf8(error.to_string()))
}

fn io_error(_path: &Path, error: io::Error) -> GitError {
    GitError::Io(error)
}

fn non_empty_string(value: &str) -> Option<String> {
    let value = value.trim();

    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_status_records() {
        let status = parse_status(
            b"## main...origin/main [ahead 1, behind 2]\0 M src/main.rs\0A  src/lib.rs\0?? README.md\0R  new.rs\0old.rs\0",
        )
        .expect("status should parse");

        assert_eq!(status.branch.as_deref(), Some("main"));
        assert_eq!(status.upstream.as_deref(), Some("origin/main"));
        assert_eq!(status.ahead, 1);
        assert_eq!(status.behind, 2);
        assert_eq!(status.changes.len(), 4);
        assert_eq!(status.changes[0].path(), "README.md");
        assert_eq!(status.changes[0].unstaged(), Some(GitChangeKind::Untracked));
        assert_eq!(status.changes[1].path(), "new.rs");
        assert_eq!(status.changes[1].original_path(), Some("old.rs"));
        assert_eq!(status.changes[1].staged(), Some(GitChangeKind::Renamed));
        assert_eq!(status.changes[2].path(), "src/lib.rs");
        assert_eq!(status.changes[2].staged(), Some(GitChangeKind::Added));
        assert_eq!(status.changes[3].path(), "src/main.rs");
        assert_eq!(status.changes[3].unstaged(), Some(GitChangeKind::Modified));
    }

    #[test]
    fn scopes_workspace_changes_to_a_nested_workspace() {
        let root = test_directory("nested-workspace-status");
        let workspace = root.join("packages/app");
        fs::create_dir_all(&workspace).expect("workspace should be created");
        GitRepository::init(&root).expect("repository should initialize");
        fs::write(root.join("outside.txt"), "outside\n").expect("outside file should be written");
        fs::write(workspace.join("inside.txt"), "inside\n").expect("inside file should be written");

        let changes =
            GitRepository::workspace_changes(&workspace).expect("workspace changes should load");

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path(), "inside.txt");
        assert_eq!(changes[0].unstaged(), Some(GitChangeKind::Untracked));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn workspace_changes_include_ignored_files_and_matching_directories() {
        let root = test_directory("ignored-workspace-status");
        GitRepository::init(&root).expect("repository should initialize");
        fs::write(root.join(".gitignore"), "ignored.txt\ncache/\n")
            .expect("ignore rules should be written");
        fs::write(root.join("ignored.txt"), "ignored\n").expect("ignored file should be written");
        fs::create_dir(root.join("cache")).expect("ignored directory should be created");
        fs::write(root.join("cache/output.bin"), "output\n")
            .expect("ignored directory file should be written");

        let changes =
            GitRepository::workspace_changes(&root).expect("workspace changes should load");
        let ignored_paths = changes
            .iter()
            .filter(|change| change.unstaged() == Some(GitChangeKind::Ignored))
            .map(GitChange::path)
            .collect::<Vec<_>>();

        assert_eq!(ignored_paths, ["cache/", "ignored.txt"]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn parses_zero_context_diff_hunks() {
        let hunks = parse_line_hunks(
            b"diff --git a/file b/file\n@@ -2 +2,2 @@ context\n@@ -7,3 +8,0 @@ context\n",
        )
        .expect("diff hunks should parse");

        assert_eq!(
            hunks,
            vec![GitLineHunk::new(2, 1, 2, 2), GitLineHunk::new(7, 3, 8, 0)]
        );
    }

    #[test]
    fn loads_all_lines_for_an_untracked_file() {
        let root = test_directory("untracked-line-hunks");
        let workspace = root.join("app");
        fs::create_dir(&workspace).expect("workspace should be created");
        GitRepository::init(&root).expect("repository should initialize");
        fs::write(workspace.join("main.rs"), "first\nsecond\n").expect("file should be written");

        let hunks =
            GitRepository::file_line_hunks(&workspace, "main.rs").expect("line hunks should load");

        assert_eq!(hunks, vec![GitLineHunk::new(0, 0, 1, 2)]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn loads_a_deletion_hunk_when_a_tracked_file_becomes_empty() {
        let root = test_directory("empty-modified-line-hunks");
        let path = root.join("main.rs");
        GitRepository::init(&root).expect("repository should initialize");
        fs::write(&path, "first\nsecond\n").expect("file should be written");
        GitRepository::stage_paths(&root, &["main.rs".to_owned()]).expect("file should be staged");
        commit(&root, "Initial");
        fs::write(&path, "").expect("file should be emptied");

        let hunks =
            GitRepository::file_line_hunks(&root, "main.rs").expect("line hunks should load");

        assert_eq!(hunks, vec![GitLineHunk::new(1, 2, 0, 0)]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn parses_branch_rows() {
        let branches = parse_branches(
            b"main\0*\0origin/main\0refs/heads/main\nfeature\0 \0\0refs/heads/feature\norigin/feature\0 \0\0refs/remotes/origin/feature\norigin\0 \0\0refs/remotes/origin/HEAD\n",
        );

        assert_eq!(branches.len(), 3);
        assert_eq!(branches[0].name(), "main");
        assert!(branches[0].current());
        assert!(!branches[0].remote());
        assert_eq!(branches[0].upstream(), Some("origin/main"));
        assert_eq!(branches[1].name(), "feature");
        assert!(!branches[1].current());
        assert!(!branches[1].remote());
        assert_eq!(branches[2].name(), "origin/feature");
        assert!(branches[2].remote());
    }

    #[test]
    fn parses_stash_records() {
        let stashes = parse_stashes(
            b"stash@{0}\x00abc1234\x001783456789\x00On main: Staged changes\x1e\nstash@{1}\x00def5678\x001783456000\x00WIP on main\x1e\n",
        )
        .expect("stashes should parse");

        assert_eq!(stashes.len(), 2);
        assert_eq!(stashes[0].selector(), "stash@{0}");
        assert_eq!(stashes[0].commit(), "abc1234");
        assert_eq!(stashes[0].timestamp(), 1_783_456_789);
        assert_eq!(stashes[0].message(), "On main: Staged changes");
        assert_eq!(stashes[1].selector(), "stash@{1}");
    }

    #[test]
    fn parses_tag_records() {
        let tags = parse_tags(b"v2.0.0\0def5678\nv1.0.0\0abc1234\n").expect("tags should parse");

        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].name(), "v2.0.0");
        assert_eq!(tags[0].target(), "def5678");
        assert_eq!(tags[1].name(), "v1.0.0");
    }

    #[test]
    fn rejects_unsafe_remote_and_tag_names() {
        assert!(normalize_remote_name("origin").is_ok());
        assert!(normalize_remote_name("--upload-pack=bad").is_err());
        assert!(normalize_tag_name("v1.0.0").is_ok());
        assert!(normalize_tag_name("--contains").is_err());
    }

    #[test]
    fn manages_repository_remotes_and_tags() {
        let root = test_directory("repository-remotes-tags");

        GitRepository::init(&root).expect("repository should initialize");
        GitRepository::add_remote(&root, "origin", "https://example.com/repository.git")
            .expect("remote should be added");

        let remotes = GitRepository::remotes(&root).expect("remotes should load");
        assert_eq!(remotes.len(), 1);
        assert_eq!(remotes[0].name(), "origin");
        assert_eq!(
            remotes[0].fetch_urls(),
            &["https://example.com/repository.git"]
        );
        assert_eq!(remotes[0].push_urls(), remotes[0].fetch_urls());

        GitRepository::remove_remote(&root, "origin").expect("remote should be removed");
        assert!(
            GitRepository::remotes(&root)
                .expect("remotes should reload")
                .is_empty()
        );

        git(
            &root,
            [
                "-c",
                "user.name=Kosmos Test",
                "-c",
                "user.email=kosmos@example.com",
                "commit",
                "--allow-empty",
                "--message",
                "Initial commit",
            ],
        )
        .expect("commit should be created");
        GitRepository::create_tag(&root, "v1.0.0").expect("tag should be created");

        let tags = GitRepository::tags(&root).expect("tags should load");
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].name(), "v1.0.0");
        assert!(!tags[0].target().is_empty());

        GitRepository::delete_tag(&root, "v1.0.0").expect("tag should be deleted");
        assert!(
            GitRepository::tags(&root)
                .expect("tags should reload")
                .is_empty()
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_unsafe_paths() {
        assert!(normalize_path("src/main.rs").is_ok());
        assert!(normalize_path("../main.rs").is_err());
        assert!(normalize_path("/tmp/main.rs").is_err());
    }

    #[test]
    fn builds_repository_diff_with_focused_path() {
        let root = test_directory("repository-diff");
        let readme_path = root.join("README.md");
        let license_path = root.join("LICENSE");

        GitRepository::init(&root).expect("repository should initialize");
        fs::write(&readme_path, "hello\n").expect("file should be written");
        fs::write(&license_path, "license\n").expect("file should be written");
        GitRepository::stage_paths(&root, &["README.md".to_owned()])
            .expect("file should be staged");

        let diff = GitRepository::diff(&root, "LICENSE").expect("diff should load");
        let readme = diff
            .files()
            .iter()
            .find(|file| file.path() == "README.md")
            .expect("staged file should be in diff");
        let license = diff
            .files()
            .iter()
            .find(|file| file.path() == "LICENSE")
            .expect("unstaged file should be in diff");

        assert_eq!(diff.focused_path(), Some("LICENSE"));
        assert_eq!(diff.files().len(), 2);
        assert_eq!(readme.staged(), Some(GitChangeKind::Added));
        assert_eq!(readme.sections()[0].kind(), GitDiffSectionKind::Staged);
        assert_eq!(readme.sections()[0].original_content(), Some(""));
        assert_eq!(readme.sections()[0].modified_content(), Some("hello\n"));
        assert!(!readme.sections()[0].editable());
        assert_eq!(license.unstaged(), Some(GitChangeKind::Untracked));
        assert_eq!(license.sections()[0].kind(), GitDiffSectionKind::Unstaged);
        assert_eq!(license.sections()[0].original_content(), Some(""));
        assert_eq!(license.sections()[0].modified_content(), Some("license\n"));
        assert!(license.sections()[0].editable());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn loads_and_resolves_conflicted_files() {
        let root = test_directory("repository-conflict");
        let file_path = root.join("conflict.txt");

        GitRepository::init(&root).expect("repository should initialize");
        fs::write(&file_path, "original\n").expect("file should be written");
        GitRepository::stage_paths(&root, &["conflict.txt".to_owned()])
            .expect("file should be staged");
        commit(&root, "Initial");
        let main_branch = String::from_utf8(git(&root, ["branch", "--show-current"]).unwrap())
            .unwrap()
            .trim()
            .to_owned();

        git(&root, ["checkout", "-b", "incoming"]).expect("branch should be created");
        fs::write(&file_path, "incoming\n").expect("file should be changed");
        GitRepository::stage_paths(&root, &["conflict.txt".to_owned()])
            .expect("file should be staged");
        commit(&root, "Incoming");

        git(&root, ["checkout", &main_branch]).expect("main branch should be restored");
        fs::write(&file_path, "current\n").expect("file should be changed");
        GitRepository::stage_paths(&root, &["conflict.txt".to_owned()])
            .expect("file should be staged");
        commit(&root, "Current");
        assert!(git(&root, ["merge", "incoming"]).is_err());

        let diff = GitRepository::diff(&root, "conflict.txt").expect("diff should load");
        let file = &diff.files()[0];
        let section = &file.sections()[0];
        assert_eq!(file.staged(), Some(GitChangeKind::Conflicted));
        assert_eq!(section.original_content(), Some("current\n"));
        assert!(
            section
                .modified_content()
                .is_some_and(|content| content.contains("<<<<<<<"))
        );
        assert!(section.editable());

        GitRepository::save_diff_file(&root, "conflict.txt", "resolved\n", true)
            .expect("conflict should be saved and staged");
        let snapshot = GitRepository::snapshot(&root).expect("status should load");
        assert_eq!(snapshot.changes().len(), 1);
        assert_eq!(
            snapshot.changes()[0].staged(),
            Some(GitChangeKind::Modified)
        );
        assert!(!snapshot.changes()[0].is_unstaged());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repository_discovery_preserves_trailing_path_whitespace() {
        let parent = test_directory("repository-whitespace");
        let root = parent.join("repository \n");
        fs::create_dir(&root).expect("repository directory should be created");
        GitRepository::init(&root).expect("repository should initialize");

        assert_eq!(repository_root(&root).unwrap(), root);

        let _ = fs::remove_dir_all(parent);
    }

    #[cfg(unix)]
    #[test]
    fn working_tree_content_does_not_follow_symlinks() {
        use std::os::unix::fs::symlink;

        let root = test_directory("untracked-symlink");
        let outside = root.with_extension("outside");
        fs::create_dir(&outside).expect("outside directory should be created");
        fs::write(outside.join("private.txt"), "private\n")
            .expect("outside file should be written");
        symlink(&outside, root.join("linked")).expect("symlink should be created");
        symlink("first\nsecond", root.join("multiline"))
            .expect("multiline symlink should be created");
        let mut stats = GitDiffStats::default();

        add_untracked_path_stats(&root.join("linked"), &mut stats)
            .expect("symlink stats should be safe");
        let (linked_content, linked_editable) =
            working_tree_content(&root, "linked").expect("symlink should load");
        let (multiline_content, multiline_editable) =
            working_tree_content(&root, "multiline").expect("symlink should load");

        assert_eq!(stats.insertions, 1);
        assert_eq!(linked_content.as_deref(), outside.to_str());
        assert!(!linked_content.unwrap_or_default().contains("private\n"));
        assert!(!linked_editable);
        assert_eq!(multiline_content.as_deref(), Some("first\nsecond"));
        assert!(!multiline_editable);

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    fn commit(root: &Path, message: &str) {
        git(
            root,
            [
                "-c",
                "user.name=Kosmos Test",
                "-c",
                "user.email=kosmos@example.com",
                "commit",
                "--message",
                message,
            ],
        )
        .expect("commit should be created");
    }

    fn test_directory(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();

        let root = std::env::temp_dir().join(format!(
            "kosmos-core-git-{}-{name}-{nanos}",
            std::process::id()
        ));

        fs::create_dir_all(&root).expect("test root should be created");

        root
    }
}
