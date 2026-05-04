use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositorySummary {
    pub work_dir: PathBuf,
    pub git_dir: PathBuf,
    pub branch: Option<String>,
    pub changes: usize,
    pub insertions: usize,
    pub deletions: usize,
    pub files: Vec<FileChange>,
    pub latest_commit: Option<CommitInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInfo {
    pub short_sha: String,
    pub subject: String,
    pub author: String,
    pub relative_time: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    pub kind: FileChangeKind,
    pub staged: bool,
    pub insertions: usize,
    pub deletions: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChangeKind {
    Created,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone)]
pub struct Remote {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct Tag {
    pub name: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Stash {
    pub id: String,
    pub message: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Branch {
    pub name: String,
    pub current: bool,
}

impl RepositorySummary {
    pub fn discover(path: impl AsRef<Path>) -> Result<Self, Error> {
        let repo = gix::discover(path.as_ref()).map_err(|source| Error::Discover {
            path: path.as_ref().to_path_buf(),
            source: source.to_string(),
        })?;

        let branch = repo
            .head_name()
            .map_err(|source| Error::ReadHead(source.to_string()))?
            .map(|name| name.shorten().to_string());

        let changes = repo
            .status(gix::progress::Discard)
            .map_err(|source| Error::Status(source.to_string()))?
            .into_iter(Vec::new())
            .map_err(|source| Error::Status(source.to_string()))?
            .try_fold(0, |count, item| {
                item.map(|_| count + 1)
                    .map_err(|source| Error::Status(source.to_string()))
            })?;

        let files = file_changes(path.as_ref()).unwrap_or_default();
        let stats = files.iter().fold(DiffStats::default(), |mut stats, file| {
            stats.insertions += file.insertions;
            stats.deletions += file.deletions;
            stats
        });
        let latest_commit = read_latest_commit(path.as_ref()).ok().flatten();

        Ok(Self {
            work_dir: repo
                .workdir()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| repo.path().to_path_buf()),
            git_dir: repo.path().to_path_buf(),
            branch,
            changes,
            insertions: stats.insertions,
            deletions: stats.deletions,
            files,
            latest_commit,
        })
    }

    pub fn is_clean(&self) -> bool {
        self.changes == 0
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct DiffStats {
    insertions: usize,
    deletions: usize,
}

fn file_changes(path: &Path) -> Result<Vec<FileChange>, Error> {
    let repo = gix::discover(path).map_err(|source| Error::Discover {
        path: path.to_path_buf(),
        source: source.to_string(),
    })?;
    let work_dir = repo.workdir().unwrap_or(path);
    let mut stats_by_path = std::collections::HashMap::new();
    for line in git_output(work_dir, &["diff", "--numstat", "HEAD"])?.lines() {
        let mut parts = line.split('\t');
        let insertions = parts.next().unwrap_or_default();
        let deletions = parts.next().unwrap_or_default();
        if let Some(path) = parts.last().filter(|path| !path.is_empty()) {
            stats_by_path.insert(
                normalize_numstat_path(path),
                DiffStats {
                    insertions: insertions.parse::<usize>().unwrap_or(0),
                    deletions: deletions.parse::<usize>().unwrap_or(0),
                },
            );
        }
    }

    let status = git_output(work_dir, &["status", "--porcelain=v1", "-z"])?;
    let mut changes = Vec::new();
    let mut entries = status.split('\0').filter(|entry| !entry.is_empty());
    while let Some(entry) = entries.next() {
        if entry.len() < 4 {
            continue;
        }
        let bytes = entry.as_bytes();
        let index_status = bytes[0] as char;
        let worktree_status = bytes[1] as char;
        let path = entry[3..].to_string();
        if matches!(index_status, 'R' | 'C') {
            let _ = entries.next();
        }

        let kind = change_kind(index_status, worktree_status);
        let staged = index_status != ' ' && index_status != '?';
        let mut stats = stats_by_path.remove(&path).unwrap_or_default();
        if index_status == '?' && stats.insertions == 0 && stats.deletions == 0 {
            stats.insertions = count_text_lines(work_dir.join(&path));
        }
        changes.push(FileChange {
            path,
            kind,
            staged,
            insertions: stats.insertions,
            deletions: stats.deletions,
        });
    }
    Ok(changes)
}

fn change_kind(index_status: char, worktree_status: char) -> FileChangeKind {
    match (index_status, worktree_status) {
        ('R', _) | ('C', _) => FileChangeKind::Renamed,
        ('A', _) | ('?', '?') => FileChangeKind::Created,
        ('D', _) | (_, 'D') => FileChangeKind::Deleted,
        _ => FileChangeKind::Modified,
    }
}

fn normalize_numstat_path(path: &str) -> String {
    path.rsplit_once(" => ")
        .map(|(_, after)| after.trim_matches(['{', '}']).to_string())
        .unwrap_or_else(|| path.to_string())
}

fn read_latest_commit(path: &Path) -> Result<Option<CommitInfo>, Error> {
    let output = git_output(
        path,
        &["log", "-1", "--pretty=format:%h%x1f%s%x1f%an%x1f%ar"],
    )?;
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let mut parts = trimmed.splitn(4, '\x1f');
    let short_sha = parts.next().unwrap_or("").to_string();
    let subject = parts.next().unwrap_or("").to_string();
    let author = parts.next().unwrap_or("").to_string();
    let relative_time = parts.next().unwrap_or("").to_string();
    if short_sha.is_empty() {
        return Ok(None);
    }
    Ok(Some(CommitInfo {
        short_sha,
        subject,
        author,
        relative_time,
    }))
}

fn count_text_lines(path: impl AsRef<Path>) -> usize {
    std::fs::read_to_string(path)
        .map(|contents| contents.lines().count())
        .unwrap_or(0)
}

pub fn stage_all(path: impl AsRef<Path>) -> Result<(), Error> {
    run_git(path.as_ref(), &["add", "--all"])
}

pub fn unstage_all(path: impl AsRef<Path>) -> Result<(), Error> {
    run_git(path.as_ref(), &["restore", "--staged", "."])
}

pub fn stage_file(path: impl AsRef<Path>, file: &str) -> Result<(), Error> {
    run_git(path.as_ref(), &["add", "--", file])
}

pub fn unstage_file(path: impl AsRef<Path>, file: &str) -> Result<(), Error> {
    run_git(path.as_ref(), &["restore", "--staged", "--", file])
}

pub fn stash_staged(path: impl AsRef<Path>) -> Result<(), Error> {
    run_git(path.as_ref(), &["stash", "push", "--staged"])
}

pub fn commit_staged(path: impl AsRef<Path>, message: &str) -> Result<(), Error> {
    run_git(path.as_ref(), &["commit", "-m", message])
}

pub fn list_branches(path: impl AsRef<Path>) -> Result<Vec<Branch>, Error> {
    let output = git_output(
        path.as_ref(),
        &["branch", "--format=%(HEAD)%09%(refname:short)"],
    )?;
    Ok(output
        .lines()
        .filter_map(|line| {
            let (head, name) = line.split_once('\t')?;
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            Some(Branch {
                name: name.to_string(),
                current: head.trim() == "*",
            })
        })
        .collect())
}

pub fn switch_branch(path: impl AsRef<Path>, branch: &str) -> Result<(), Error> {
    run_git(path.as_ref(), &["switch", branch])
}

pub fn push(path: impl AsRef<Path>) -> Result<(), Error> {
    run_git(path.as_ref(), &["push"])
}

pub fn pull(path: impl AsRef<Path>) -> Result<(), Error> {
    run_git(path.as_ref(), &["pull"])
}

pub fn fetch(path: impl AsRef<Path>) -> Result<(), Error> {
    run_git(path.as_ref(), &["fetch", "--all", "--prune"])
}

pub fn discard_all_changes(path: impl AsRef<Path>) -> Result<(), Error> {
    run_git(path.as_ref(), &["reset", "--hard", "HEAD"])?;
    run_git(path.as_ref(), &["clean", "-fd"])
}

pub fn discard_files(path: impl AsRef<Path>, files: &[String]) -> Result<(), Error> {
    for file in files {
        run_git(
            path.as_ref(),
            &["restore", "--staged", "--worktree", "--", file],
        )?;
    }
    Ok(())
}

pub fn list_remotes(path: impl AsRef<Path>) -> Result<Vec<Remote>, Error> {
    let output = git_output(path.as_ref(), &["remote", "-v"])?;
    let mut remotes = Vec::new();
    for line in output.lines().filter(|line| line.ends_with("(fetch)")) {
        let mut parts = line.split_whitespace();
        let Some(name) = parts.next() else { continue };
        let Some(url) = parts.next() else { continue };
        remotes.push(Remote {
            name: name.to_string(),
            url: url.to_string(),
        });
    }
    Ok(remotes)
}

pub fn add_remote(path: impl AsRef<Path>, name: &str, url: &str) -> Result<(), Error> {
    run_git(path.as_ref(), &["remote", "add", name, url])
}

pub fn delete_remote(path: impl AsRef<Path>, name: &str) -> Result<(), Error> {
    run_git(path.as_ref(), &["remote", "remove", name])
}

pub fn list_tags(path: impl AsRef<Path>) -> Result<Vec<Tag>, Error> {
    let output = git_output(path.as_ref(), &["tag", "-n99"])?;
    Ok(output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, char::is_whitespace);
            let name = parts.next()?.trim();
            if name.is_empty() {
                return None;
            }
            Some(Tag {
                name: name.to_string(),
                message: parts.next().unwrap_or_default().trim().to_string(),
            })
        })
        .collect())
}

pub fn add_tag(
    path: impl AsRef<Path>,
    name: &str,
    message: Option<&str>,
    commit_sha: Option<&str>,
) -> Result<(), Error> {
    let mut args = vec!["tag"];
    if let Some(message) = message.filter(|message| !message.trim().is_empty()) {
        args.extend(["-a", name, "-m", message]);
    } else {
        args.push(name);
    }
    if let Some(commit_sha) = commit_sha.filter(|sha| !sha.trim().is_empty()) {
        args.push(commit_sha);
    }
    run_git(path.as_ref(), &args)
}

pub fn delete_tag(path: impl AsRef<Path>, name: &str) -> Result<(), Error> {
    run_git(path.as_ref(), &["tag", "-d", name])
}

pub fn list_stashes(path: impl AsRef<Path>) -> Result<Vec<Stash>, Error> {
    let path = path.as_ref();
    let output = git_output(path, &["stash", "list"])?;
    output
        .lines()
        .filter_map(|line| {
            let (id, message) = line.split_once(':')?;
            Some((id.to_string(), message.trim().to_string()))
        })
        .map(|(id, message)| {
            let files = git_output(path, &["stash", "show", "--name-only", &id])?
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(ToString::to_string)
                .collect();
            Ok(Stash { id, message, files })
        })
        .collect()
}

pub fn apply_stash(path: impl AsRef<Path>, id: &str) -> Result<(), Error> {
    run_git(path.as_ref(), &["stash", "apply", id])
}

pub fn delete_stash(path: impl AsRef<Path>, id: &str) -> Result<(), Error> {
    run_git(path.as_ref(), &["stash", "drop", id])
}

fn git_output(path: &Path, args: &[&str]) -> Result<String, Error> {
    let repo = gix::discover(path).map_err(|source| Error::Discover {
        path: path.to_path_buf(),
        source: source.to_string(),
    })?;
    let work_dir = repo.workdir().unwrap_or(path);
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(work_dir)
        .output()
        .map_err(|source| Error::Status(source.to_string()))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(Error::Status(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

fn run_git(path: &Path, args: &[&str]) -> Result<(), Error> {
    let repo = gix::discover(path).map_err(|source| Error::Discover {
        path: path.to_path_buf(),
        source: source.to_string(),
    })?;
    let work_dir = repo.workdir().unwrap_or(path);
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(work_dir)
        .output()
        .map_err(|source| Error::Status(source.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(Error::Status(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

#[derive(Debug, Clone)]
pub enum Error {
    Discover { path: PathBuf, source: String },
    ReadHead(String),
    Status(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Discover { path, source } => {
                write!(
                    f,
                    "failed to discover git repository at {}: {source}",
                    path.display()
                )
            }
            Self::ReadHead(source) => write!(f, "failed to read git HEAD: {source}"),
            Self::Status(source) => write!(f, "failed to read git status: {source}"),
        }
    }
}

impl std::error::Error for Error {}
