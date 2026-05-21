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
    Conflicted,
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

        let files = file_changes(path.as_ref()).unwrap_or_default();
        let changes = files.len();
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
    let status = git_output(
        &work_dir,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    let mut stats_by_path = git_output(&work_dir, &["diff", "--numstat", "HEAD"])
        .map(|numstat| parse_numstat(&numstat))
        .unwrap_or_default();
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
        let staged = kind != FileChangeKind::Conflicted
            && entry.index_status != ' '
            && entry.index_status != '?';
        let mut stats = stats_by_path.remove(&entry.path).unwrap_or_default();
        if kind == FileChangeKind::Created && stats.insertions == 0 && stats.deletions == 0 {
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
        ('D', 'D')
        | ('A', 'U')
        | ('U', 'D')
        | ('U', 'A')
        | ('D', 'U')
        | ('A', 'A')
        | ('U', 'U') => FileChangeKind::Conflicted,
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

pub fn init(path: impl AsRef<Path>) -> Result<(), Error> {
    let output = std::process::Command::new("git")
        .args(["init"])
        .current_dir(path.as_ref())
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

pub fn unstage_all(path: impl AsRef<Path>) -> Result<(), Error> {
    unstage_matching_changes(path.as_ref(), None)
}

pub fn stage_file(path: impl AsRef<Path>, file: &str) -> Result<(), Error> {
    run_git(path.as_ref(), &["add", "--all", "--", file])
}

pub fn stage_files(path: impl AsRef<Path>, files: &[String]) -> Result<(), Error> {
    let path = path.as_ref();
    for file in files {
        run_git(path, &["add", "--all", "--", file])?;
    }
    Ok(())
}

pub fn unstage_file(path: impl AsRef<Path>, file: &str) -> Result<(), Error> {
    unstage_matching_changes(path.as_ref(), Some(file))
}

fn unstage_matching_changes(path: &Path, selected_path: Option<&str>) -> Result<(), Error> {
    for change_path in non_conflicted_change_paths(path, selected_path, Some(true))? {
        run_git(path, &["reset", "--", &change_path])?;
    }
    Ok(())
}

fn non_conflicted_change_paths(
    path: &Path,
    selected_path: Option<&str>,
    staged: Option<bool>,
) -> Result<Vec<String>, Error> {
    Ok(file_changes(path)?
        .into_iter()
        .filter(|change| change.kind != FileChangeKind::Conflicted)
        .filter(|change| selected_path.is_none_or(|path| path_matches_change(path, &change.path)))
        .filter(|change| staged.is_none_or(|staged| change.staged == staged))
        .map(|change| change.path)
        .collect())
}

fn path_matches_change(selected_path: &str, change_path: &str) -> bool {
    let selected_path = selected_path.trim_end_matches('/');
    change_path == selected_path
        || change_path
            .strip_prefix(selected_path)
            .is_some_and(|suffix| suffix.starts_with('/'))
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
    fn parses_conflicted_status_entries() {
        let mut stats = HashMap::from([(
            "file.txt".to_string(),
            DiffStats {
                insertions: 4,
                deletions: 0,
            },
        )]);
        let changes = parse_status_changes(
            "UU file.txt\0AA both-added.txt\0DD both-deleted.txt\0",
            Path::new("/tmp"),
            &mut stats,
        );

        assert_eq!(changes.len(), 3);
        assert_eq!(changes[0].path, "file.txt");
        assert_eq!(changes[0].kind, FileChangeKind::Conflicted);
        assert!(!changes[0].staged);
        assert_eq!(changes[0].insertions, 4);

        assert_eq!(changes[1].kind, FileChangeKind::Conflicted);
        assert!(!changes[1].staged);
        assert_eq!(changes[2].kind, FileChangeKind::Conflicted);
        assert!(!changes[2].staged);
    }

    #[test]
    fn discovers_files_in_unborn_repository() {
        let root = temp_test_dir("unborn");
        std::fs::create_dir_all(&root).expect("create test repository directory");
        run_test_git(&root, &["init"]);
        std::fs::write(root.join("file.txt"), "first\nsecond\n").expect("write test file");
        std::fs::create_dir(root.join("src")).expect("create nested source directory");
        std::fs::write(root.join("src/main.zig"), "pub fn main() void {}\n")
            .expect("write nested test file");

        let summary = RepositorySummary::discover(&root).expect("discover unborn repository");

        assert_eq!(summary.changes, 2);
        assert_eq!(summary.files.len(), 2);
        assert!(summary.files.iter().all(|file| !file.path.ends_with('/')));
        let file = summary
            .files
            .iter()
            .find(|file| file.path == "file.txt")
            .expect("include root file");
        assert_eq!(file.kind, FileChangeKind::Created);
        assert_eq!(file.insertions, 2);
        let nested_file = summary
            .files
            .iter()
            .find(|file| file.path == "src/main.zig")
            .expect("include nested file instead of collapsed directory");
        assert_eq!(nested_file.kind, FileChangeKind::Created);
        assert_eq!(nested_file.insertions, 1);

        run_test_git(&root, &["add", "src/main.zig"]);
        let staged_summary =
            RepositorySummary::discover(&root).expect("rediscover unborn repository");
        let staged_nested_file = staged_summary
            .files
            .iter()
            .find(|file| file.path == "src/main.zig")
            .expect("include staged nested file");
        assert!(staged_nested_file.staged);
        assert_eq!(staged_nested_file.insertions, 1);

        std::fs::remove_dir_all(root).expect("remove test repository directory");
    }

    #[test]
    fn unstages_files_in_unborn_repository() {
        let root = temp_test_dir("unstage-unborn");
        std::fs::create_dir_all(root.join("src")).expect("create test repository directories");
        run_test_git(&root, &["init"]);
        std::fs::write(root.join("file.txt"), "root\n").expect("write root test file");
        std::fs::write(root.join("src/main.zig"), "pub fn main() void {}\n")
            .expect("write nested test file");
        run_test_git(&root, &["add", "."]);

        unstage_file(&root, "src").expect("unstage nested directory before first commit");
        let summary = RepositorySummary::discover(&root).expect("discover partially unstaged repo");
        assert!(
            summary
                .files
                .iter()
                .find(|file| file.path == "file.txt")
                .expect("include root file")
                .staged
        );
        assert!(
            !summary
                .files
                .iter()
                .find(|file| file.path == "src/main.zig")
                .expect("include nested file")
                .staged
        );

        unstage_all(&root).expect("unstage all before first commit");
        let summary = RepositorySummary::discover(&root).expect("discover unstaged repo");
        assert!(summary.files.iter().all(|file| !file.staged));

        std::fs::remove_dir_all(root).expect("remove test repository directory");
    }

    #[test]
    fn unstage_all_does_not_clear_conflicts() {
        let root = create_conflicted_test_repository("bulk-stage-conflict");

        unstage_all(&root).expect("skip conflicted file when unstaging all");

        let summary = RepositorySummary::discover(&root).expect("discover conflicted repository");
        assert_eq!(summary.files.len(), 1);
        assert_eq!(summary.files[0].kind, FileChangeKind::Conflicted);

        std::fs::remove_dir_all(root).expect("remove test repository directory");
    }

    #[test]
    fn stage_all_marks_conflict_resolved() {
        let root = create_conflicted_test_repository("stage-all-conflict");

        stage_all(&root).expect("stage all, including conflicted files");

        let summary = RepositorySummary::discover(&root).expect("discover resolved repository");
        assert_eq!(summary.files.len(), 1);
        assert_eq!(summary.files[0].kind, FileChangeKind::Modified);
        assert!(summary.files[0].staged);

        std::fs::remove_dir_all(root).expect("remove test repository directory");
    }

    #[test]
    fn stage_file_marks_conflict_resolved() {
        let root = create_conflicted_test_repository("stage-conflict-file");

        stage_file(&root, "file.txt").expect("stage conflicted file as resolved");

        let summary = RepositorySummary::discover(&root).expect("discover resolved repository");
        assert_eq!(summary.files.len(), 1);
        assert_eq!(summary.files[0].kind, FileChangeKind::Modified);
        assert!(summary.files[0].staged);

        std::fs::remove_dir_all(root).expect("remove test repository directory");
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

    fn temp_test_dir(name: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("kosmos-git-{name}-{}-{unique}", std::process::id()))
    }

    fn create_conflicted_test_repository(name: &str) -> PathBuf {
        let root = temp_test_dir(name);
        std::fs::create_dir_all(&root).expect("create test repository directory");
        run_test_git(&root, &["init"]);
        run_test_git(&root, &["config", "user.email", "a@example.com"]);
        run_test_git(&root, &["config", "user.name", "A"]);
        std::fs::write(root.join("file.txt"), "base\n").expect("write base file");
        run_test_git(&root, &["add", "file.txt"]);
        run_test_git(&root, &["commit", "-m", "base"]);
        run_test_git(&root, &["switch", "-c", "left"]);
        std::fs::write(root.join("file.txt"), "left\n").expect("write left file");
        run_test_git(&root, &["commit", "-am", "left"]);
        run_test_git(&root, &["switch", "-"]);
        run_test_git(&root, &["switch", "-c", "right"]);
        std::fs::write(root.join("file.txt"), "right\n").expect("write right file");
        run_test_git(&root, &["commit", "-am", "right"]);
        run_test_git_expect_failure(&root, &["merge", "left"]);
        root
    }

    fn run_test_git(root: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("run git command");
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn run_test_git_expect_failure(root: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("run git command");
        assert!(
            !output.status.success(),
            "git {} succeeded unexpectedly",
            args.join(" ")
        );
    }
}
