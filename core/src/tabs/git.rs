use std::error::Error as StdError;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitStash {
    selector: String,
    commit: String,
    timestamp: i64,
    message: String,
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
    patch: String,
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

    pub fn diff(directory: impl AsRef<Path>, focused_path: &str) -> Result<GitDiff> {
        let repository_root = repository_root(directory.as_ref())?;
        let focused_path = normalize_path(focused_path)?;
        let status = parse_status(&git(&repository_root, ["status", "--porcelain=v1", "-z"])?)?;

        Ok(GitDiff {
            focused_path: Some(focused_path),
            files: status
                .changes
                .iter()
                .map(|change| diff_file(&repository_root, change))
                .collect::<Result<Vec<_>>>()?,
        })
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
    fn new(kind: GitDiffSectionKind, patch: String) -> Self {
        Self { kind, patch }
    }

    pub fn kind(&self) -> GitDiffSectionKind {
        self.kind
    }

    pub fn patch(&self) -> &str {
        &self.patch
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
    Io(io::Error),
    NotWorktree(PathBuf),
    TabNotFound,
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
            Self::Io(error) => write!(formatter, "{error}"),
            Self::NotWorktree(path) => write!(
                formatter,
                "git repository has no worktree: {}",
                path.display()
            ),
            Self::TabNotFound => formatter.write_str("git tab does not exist"),
            Self::Utf8(message) => formatter.write_str(message),
            Self::WorkspaceNotFound => formatter.write_str("workspace does not exist"),
        }
    }
}

impl StdError for GitError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::BranchRequired
            | Self::CommandFailed { .. }
            | Self::CommitMessageRequired
            | Self::Discover { .. }
            | Self::InvalidPath(_)
            | Self::InvalidStash(_)
            | Self::NotWorktree(_)
            | Self::TabNotFound
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

    if change.is_staged() {
        sections.push(GitDiffSection::new(
            GitDiffSectionKind::Staged,
            git_patch(repository_root, &["diff", "--cached", "--"], change.path())?,
        ));
    }

    if change.is_unstaged() {
        let patch = if change.unstaged() == Some(GitChangeKind::Untracked) {
            untracked_patch(repository_root, change.path())?
        } else {
            git_patch(repository_root, &["diff", "--"], change.path())?
        };

        sections.push(GitDiffSection::new(GitDiffSectionKind::Unstaged, patch));
    }

    Ok(GitDiffFile::new(
        change.path(),
        change.original_path().map(ToOwned::to_owned),
        change.staged(),
        change.unstaged(),
        sections,
    ))
}

fn git_patch(repository_root: &Path, args: &[&str], path: &str) -> Result<String> {
    let paths = [path.to_owned()];

    String::from_utf8(git_with_paths(
        repository_root,
        args.iter().copied(),
        &paths,
    )?)
    .map_err(|error| GitError::Utf8(error.to_string()))
}

fn untracked_patch(repository_root: &Path, path: &str) -> Result<String> {
    let full_path = repository_root.join(path.trim_end_matches('/'));
    let file_type = fs::symlink_metadata(&full_path)
        .map_err(|error| io_error(&full_path, error))?
        .file_type();

    if file_type.is_symlink() {
        return untracked_symlink_patch(&full_path, path);
    }

    if file_type.is_dir() {
        let mut files = Vec::new();
        collect_files(&full_path, &mut files)?;
        files.sort();

        let mut patch = String::new();
        for file in files {
            let relative_path = file
                .strip_prefix(repository_root)
                .map(|path| path.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| file.to_string_lossy().into_owned());
            patch.push_str(&untracked_file_patch(repository_root, &relative_path)?);
        }

        return Ok(patch);
    }

    untracked_file_patch(repository_root, path)
}

fn untracked_symlink_patch(full_path: &Path, relative_path: &str) -> Result<String> {
    let target = fs::read_link(full_path).map_err(|error| io_error(full_path, error))?;
    let target = target.to_string_lossy();
    let insertion_count = line_count(target.as_bytes());
    let old_path = format!("a/{relative_path}");
    let new_path = format!("b/{relative_path}");
    let mut added_lines = String::new();

    for line in target.split_inclusive('\n') {
        added_lines.push('+');
        added_lines.push_str(line);
    }

    if !target.ends_with('\n') {
        added_lines.push_str("\n\\ No newline at end of file\n");
    }

    Ok(format!(
        "diff --git {old_path:?} {new_path:?}\nnew file mode 120000\n--- /dev/null\n+++ {new_path:?}\n@@ -0,0 +1,{insertion_count} @@\n{added_lines}"
    ))
}

fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(directory).map_err(|error| io_error(directory, error))? {
        let entry = entry.map_err(|error| io_error(directory, error))?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| io_error(&path, error))?;

        if file_type.is_dir() {
            collect_files(&path, files)?;
        } else if file_type.is_file() {
            files.push(path);
        }
    }

    Ok(())
}

fn untracked_file_patch(repository_root: &Path, path: &str) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository_root)
        .args(["diff", "--no-index", "--", "/dev/null"])
        .arg(path)
        .output()?;

    if output.status.success() || output.status.code() == Some(1) {
        String::from_utf8(output.stdout).map_err(|error| GitError::Utf8(error.to_string()))
    } else {
        Err(GitError::CommandFailed {
            command: format!("git diff --no-index -- /dev/null {path}"),
            stderr: command_stderr(&output.stderr),
        })
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
        assert!(readme.sections()[0].patch().contains("+hello"));
        assert_eq!(license.unstaged(), Some(GitChangeKind::Untracked));
        assert_eq!(license.sections()[0].kind(), GitDiffSectionKind::Unstaged);
        assert!(license.sections()[0].patch().contains("+license"));

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
    fn untracked_file_collection_does_not_follow_symlinks() {
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
        let mut files = Vec::new();

        add_untracked_path_stats(&root.join("linked"), &mut stats)
            .expect("symlink stats should be safe");
        collect_files(&root, &mut files).expect("file collection should succeed");
        let patch = untracked_patch(&root, "linked").expect("symlink patch should be generated");
        let multiline_patch =
            untracked_patch(&root, "multiline").expect("multiline patch should be generated");

        assert_eq!(stats.insertions, 1);
        assert!(files.is_empty());
        assert!(patch.contains(outside.to_string_lossy().as_ref()));
        assert!(!patch.contains("private\n"));
        assert!(multiline_patch.contains("@@ -0,0 +1,2 @@"));
        assert!(multiline_patch.contains("+first\n+second"));

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
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
