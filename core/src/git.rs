use std::error::Error as StdError;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

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
        let status = parse_status(&git_bytes(
            &repository_root,
            ["status", "--porcelain=v1", "-z", "--branch"],
        )?)?;
        let branches = parse_branches(&git_bytes(
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
        let bytes = git_bytes(
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
    let repository = gix::discover(directory).map_err(|error| GitError::Discover {
        directory: directory.to_path_buf(),
        message: error.to_string(),
    })?;

    repository
        .workdir()
        .map(Path::to_path_buf)
        .ok_or_else(|| GitError::NotWorktree(repository.path().to_path_buf()))
}

fn staged_paths(repository_root: &Path) -> Result<Vec<String>> {
    let status = parse_status(&git_bytes(
        repository_root,
        ["status", "--porcelain=v1", "-z"],
    )?)?;

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
    let name = non_empty_string(&String::from_utf8_lossy(fields.next()?).to_string())?;
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
    match git_bytes(repository_root, ["log", "-1", "--pretty=%s"]) {
        Ok(output) => Ok(non_empty_string(&String::from_utf8_lossy(&output))),
        Err(GitError::CommandFailed { stderr, .. })
            if stderr.contains("does not have any commits") =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn has_head(repository_root: &Path) -> Result<bool> {
    match git(repository_root, ["rev-parse", "--verify", "HEAD"]) {
        Ok(_) => Ok(true),
        Err(GitError::CommandFailed { stderr, .. })
            if stderr.contains("Needed a single revision")
                || stderr.contains("unknown revision")
                || stderr.contains("ambiguous argument") =>
        {
            Ok(false)
        }
        Err(error) => Err(error),
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
    if path.is_dir() {
        for entry in fs::read_dir(path).map_err(|error| io_error(path, error))? {
            let entry = entry.map_err(|error| io_error(path, error))?;
            add_untracked_path_stats(&entry.path(), stats)?;
        }

        return Ok(());
    }

    if path.is_file() {
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
}
