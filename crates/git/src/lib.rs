use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Output;

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
    pub remote: bool,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct DiffStats {
    insertions: usize,
    deletions: usize,
}

fn file_changes(path: &Path) -> Result<Vec<FileChange>, Error> {
    let work_dir = git_work_dir(path)?;
    let numstat = git_output(&work_dir, &["diff", "--numstat", "HEAD"])?;
    let mut stats_by_path = parse_numstat(&numstat);
    let status = git_output(&work_dir, &["status", "--porcelain=v1", "-z"])?;
    Ok(parse_status_changes(&status, &work_dir, &mut stats_by_path))
}

fn parse_numstat(output: &str) -> HashMap<String, DiffStats> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let insertions = parts.next().unwrap_or_default();
            let deletions = parts.next().unwrap_or_default();
            let changed_path = parts.next_back().filter(|path| !path.is_empty())?;
            Some((
                normalize_numstat_path(changed_path),
                DiffStats {
                    insertions: insertions.parse::<usize>().unwrap_or(0),
                    deletions: deletions.parse::<usize>().unwrap_or(0),
                },
            ))
        })
        .collect()
}

struct StatusEntry {
    index_status: char,
    worktree_status: char,
    path: String,
}

fn parse_status_changes(
    status: &str,
    work_dir: &Path,
    stats_by_path: &mut HashMap<String, DiffStats>,
) -> Vec<FileChange> {
    let mut changes = Vec::new();
    let mut entries = status.split('\0').filter(|entry| !entry.is_empty());
    while let Some(raw_entry) = entries.next() {
        let Some(entry) = parse_status_entry(raw_entry) else {
            continue;
        };
        if matches!(entry.index_status, 'R' | 'C') {
            let _ = entries.next();
        }

        let kind = change_kind(entry.index_status, entry.worktree_status);
        let staged = entry.index_status != ' ' && entry.index_status != '?';
        let mut stats = stats_by_path.remove(&entry.path).unwrap_or_default();
        if entry.index_status == '?' && stats.insertions == 0 && stats.deletions == 0 {
            stats.insertions = count_text_lines(work_dir.join(&entry.path));
        }
        changes.push(FileChange {
            path: entry.path,
            kind,
            staged,
            insertions: stats.insertions,
            deletions: stats.deletions,
        });
    }
    changes
}

fn parse_status_entry(entry: &str) -> Option<StatusEntry> {
    if entry.len() < 4 {
        return None;
    }
    let bytes = entry.as_bytes();
    Some(StatusEntry {
        index_status: bytes[0] as char,
        worktree_status: bytes[1] as char,
        path: entry[3..].to_string(),
    })
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
    let Some(short_sha) = parts.next().filter(|short_sha| !short_sha.is_empty()) else {
        return Ok(None);
    };
    let subject = parts.next().unwrap_or("").to_string();
    let author = parts.next().unwrap_or("").to_string();
    let relative_time = parts.next().unwrap_or("").to_string();
    Ok(Some(CommitInfo {
        short_sha: short_sha.to_string(),
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
    let local_output = git_output(
        path.as_ref(),
        &["branch", "--format=%(HEAD)%09%(refname:short)"],
    )?;
    let remote_output = git_output(
        path.as_ref(),
        &[
            "branch",
            "--remotes",
            "--format=%(refname:short)%09%(symref)",
        ],
    )?;
    let mut branches = local_output
        .lines()
        .filter_map(parse_local_branch)
        .collect::<Vec<_>>();

    branches.extend(remote_output.lines().filter_map(parse_remote_branch));

    Ok(branches)
}

fn parse_local_branch(line: &str) -> Option<Branch> {
    let (head, name) = line.split_once('\t')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    Some(Branch {
        name: name.to_string(),
        current: head.trim() == "*",
        remote: false,
    })
}

fn parse_remote_branch(line: &str) -> Option<Branch> {
    let (name, symref) = line.split_once('\t').unwrap_or((line, ""));
    let name = name.trim();
    if name.is_empty() || !symref.trim().is_empty() {
        return None;
    }
    Some(Branch {
        name: name.to_string(),
        current: false,
        remote: true,
    })
}

pub fn switch_branch(path: impl AsRef<Path>, branch: &str) -> Result<(), Error> {
    run_git(path.as_ref(), &["switch", branch])
}

pub fn switch_remote_branch(path: impl AsRef<Path>, branch: &str) -> Result<(), Error> {
    run_git(path.as_ref(), &["switch", "--track", branch])
}

pub fn create_branch(path: impl AsRef<Path>, branch: &str) -> Result<(), Error> {
    run_git(path.as_ref(), &["branch", branch])
}

pub fn delete_branch(path: impl AsRef<Path>, branch: &str) -> Result<(), Error> {
    run_git(path.as_ref(), &["branch", "-d", branch])
}

pub fn push(path: impl AsRef<Path>) -> Result<(), Error> {
    run_git(path.as_ref(), &["push"])
}

pub fn force_push(path: impl AsRef<Path>) -> Result<(), Error> {
    run_git(path.as_ref(), &["push", "--force-with-lease"])
}

pub fn pull(path: impl AsRef<Path>) -> Result<(), Error> {
    run_git(path.as_ref(), &["pull"])
}

pub fn pull_rebase(path: impl AsRef<Path>) -> Result<(), Error> {
    run_git(path.as_ref(), &["pull", "--rebase"])
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
    Ok(output.lines().filter_map(parse_remote).collect())
}

fn parse_remote(line: &str) -> Option<Remote> {
    if !line.ends_with("(fetch)") {
        return None;
    }
    let mut parts = line.split_whitespace();
    let name = parts.next()?;
    let url = parts.next()?;
    Some(Remote {
        name: name.to_string(),
        url: url.to_string(),
    })
}

pub fn add_remote(path: impl AsRef<Path>, name: &str, url: &str) -> Result<(), Error> {
    run_git(path.as_ref(), &["remote", "add", name, url])
}

pub fn delete_remote(path: impl AsRef<Path>, name: &str) -> Result<(), Error> {
    run_git(path.as_ref(), &["remote", "remove", name])
}

pub fn list_tags(path: impl AsRef<Path>) -> Result<Vec<Tag>, Error> {
    let output = git_output(path.as_ref(), &["tag", "-n99"])?;
    Ok(output.lines().filter_map(parse_tag).collect())
}

fn parse_tag(line: &str) -> Option<Tag> {
    let mut parts = line.splitn(2, char::is_whitespace);
    let name = parts.next()?.trim();
    if name.is_empty() {
        return None;
    }
    Some(Tag {
        name: name.to_string(),
        message: parts.next().unwrap_or_default().trim().to_string(),
    })
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
        .filter_map(parse_stash_line)
        .map(|(stash_id, message)| {
            let files = git_output(path, &["stash", "show", "--name-only", &stash_id])?
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(ToString::to_string)
                .collect();
            Ok(Stash {
                id: stash_id,
                message,
                files,
            })
        })
        .collect()
}

fn parse_stash_line(line: &str) -> Option<(String, String)> {
    let (stash_id, message) = line.split_once(':')?;
    Some((stash_id.to_string(), message.trim().to_string()))
}

pub fn apply_stash(path: impl AsRef<Path>, id: &str) -> Result<(), Error> {
    run_git(path.as_ref(), &["stash", "apply", id])
}

pub fn delete_stash(path: impl AsRef<Path>, id: &str) -> Result<(), Error> {
    run_git(path.as_ref(), &["stash", "drop", id])
}

fn git_output(path: &Path, args: &[&str]) -> Result<String, Error> {
    let output = run_git_command(path, args)?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn run_git(path: &Path, args: &[&str]) -> Result<(), Error> {
    run_git_command(path, args).map(|_| ())
}

fn run_git_command(path: &Path, args: &[&str]) -> Result<Output, Error> {
    let work_dir = git_work_dir(path)?;
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(work_dir)
        .output()
        .map_err(|source| Error::Status(source.to_string()))?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(Error::Status(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

fn git_work_dir(path: &Path) -> Result<PathBuf, Error> {
    let repo = gix::discover(path).map_err(|source| Error::Discover {
        path: path.to_path_buf(),
        source: source.to_string(),
    })?;
    Ok(repo
        .workdir()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| path.to_path_buf()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_numstat_by_normalized_path() {
        let stats = parse_numstat("3\t1\tsrc/lib.rs\n-\t-\tassets/logo.png\n2\t0\told => new.rs\n");

        assert_eq!(
            stats.get("src/lib.rs"),
            Some(&DiffStats {
                insertions: 3,
                deletions: 1,
            })
        );
        assert_eq!(
            stats.get("assets/logo.png"),
            Some(&DiffStats {
                insertions: 0,
                deletions: 0,
            })
        );
        assert!(stats.contains_key("new.rs"));
    }

    #[test]
    fn parses_status_entries_with_stats_and_renames() {
        let mut stats = HashMap::from([(
            "src/lib.rs".to_string(),
            DiffStats {
                insertions: 4,
                deletions: 2,
            },
        )]);
        let changes = parse_status_changes(
            " M src/lib.rs\0?? scratch.txt\0R  renamed.rs\0old.rs\0bad\0",
            Path::new("/tmp"),
            &mut stats,
        );

        assert_eq!(changes.len(), 3);
        assert_eq!(changes[0].path, "src/lib.rs");
        assert_eq!(changes[0].kind, FileChangeKind::Modified);
        assert!(!changes[0].staged);
        assert_eq!(changes[0].insertions, 4);
        assert_eq!(changes[0].deletions, 2);

        assert_eq!(changes[1].kind, FileChangeKind::Created);
        assert!(!changes[1].staged);

        assert_eq!(changes[2].path, "renamed.rs");
        assert_eq!(changes[2].kind, FileChangeKind::Renamed);
        assert!(changes[2].staged);
    }

    #[test]
    fn parses_git_listing_rows() {
        let local = parse_local_branch("*\tmain").unwrap();
        assert_eq!(local.name, "main");
        assert!(local.current);
        assert!(!local.remote);

        let remote = parse_remote_branch("origin/main\t").unwrap();
        assert_eq!(remote.name, "origin/main");
        assert!(remote.remote);
        assert!(parse_remote_branch("origin/HEAD\trefs/remotes/origin/main").is_none());

        let parsed_remote = parse_remote("origin\tgit@example.com:repo.git (fetch)").unwrap();
        assert_eq!(parsed_remote.name, "origin");
        assert_eq!(parsed_remote.url, "git@example.com:repo.git");

        let tag = parse_tag("v1.0.0    First release").unwrap();
        assert_eq!(tag.name, "v1.0.0");
        assert_eq!(tag.message, "First release");

        let stash = parse_stash_line("stash@{0}: WIP on main").unwrap();
        assert_eq!(stash.0, "stash@{0}");
        assert_eq!(stash.1, "WIP on main");
    }
}
