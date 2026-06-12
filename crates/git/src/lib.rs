use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Output;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositorySummary {
    pub work_dir: PathBuf,
    pub git_dir: PathBuf,
    pub branch: Option<String>,
    pub branch_sync: BranchSyncStatus,
    pub changes: usize,
    pub insertions: usize,
    pub deletions: usize,
    pub files: Vec<FileChange>,
    pub latest_commit: Option<CommitInfo>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BranchSyncStatus {
    pub ahead: usize,
    pub behind: usize,
}

impl BranchSyncStatus {
    pub fn is_synced(self) -> bool {
        self.ahead == 0 && self.behind == 0
    }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryDiff {
    pub work_dir: PathBuf,
    pub files: Vec<FileDiff>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub old_path: Option<String>,
    pub path: String,
    pub kind: FileChangeKind,
    pub binary: bool,
    pub hunks: Vec<DiffHunk>,
    pub conflicts: Vec<ConflictBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    pub old_start: usize,
    pub old_lines: usize,
    pub new_start: usize,
    pub new_lines: usize,
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictBlock {
    pub start_line: usize,
    pub separator_line: usize,
    pub end_line: usize,
    pub current_label: String,
    pub incoming_label: String,
    pub current: Vec<ConflictLine>,
    pub incoming: Vec<ConflictLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictLine {
    pub line: usize,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictResolution {
    Current,
    Incoming,
    Both,
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
    pub files: Vec<StashFile>,
}

#[derive(Debug, Clone)]
pub struct StashFile {
    pub path: String,
    pub insertions: usize,
    pub deletions: usize,
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
        let branch_sync = read_branch_sync_status(path.as_ref()).unwrap_or_default();
        let latest_commit = read_latest_commit(path.as_ref()).ok().flatten();

        Ok(Self {
            work_dir: repo
                .workdir()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| repo.path().to_path_buf()),
            git_dir: repo.path().to_path_buf(),
            branch,
            branch_sync,
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

impl RepositoryDiff {
    pub fn discover(path: impl AsRef<Path>) -> Result<Self, Error> {
        Self::discover_pathspec(path.as_ref(), None)
    }

    pub fn discover_paths(path: impl AsRef<Path>, paths: &[String]) -> Result<Self, Error> {
        if paths.is_empty() {
            let work_dir = git_work_dir(path.as_ref())?;
            return Ok(Self {
                work_dir,
                files: Vec::new(),
            });
        }

        Self::discover_pathspec(path.as_ref(), Some(paths))
    }

    fn discover_pathspec(path: &Path, paths: Option<&[String]>) -> Result<Self, Error> {
        let work_dir = git_work_dir(path.as_ref())?;
        let has_head = git_output(&work_dir, &["rev-parse", "--verify", "HEAD"]).is_ok();
        let output = if has_head {
            let mut args = vec![
                "diff",
                "--no-color",
                "--no-ext-diff",
                "--find-renames",
                "--unified=3",
                "HEAD",
                "--",
            ];
            if let Some(paths) = paths {
                args.extend(paths.iter().map(String::as_str));
            }
            git_output(&work_dir, &args)?
        } else {
            String::new()
        };
        let mut files = parse_unified_diff(&output);
        let changes = filtered_file_changes(&work_dir, paths);
        apply_status_to_file_diffs(&mut files, &work_dir, &changes);
        synthesize_created_file_diffs(&mut files, &work_dir, &changes);
        synthesize_conflicted_file_diffs(&mut files, &work_dir, &changes);

        Ok(Self { work_dir, files })
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

fn filtered_file_changes(path: &Path, paths: Option<&[String]>) -> Vec<FileChange> {
    let changes = file_changes(path).unwrap_or_default();
    let Some(paths) = paths else {
        return changes;
    };

    let paths = paths.iter().map(String::as_str).collect::<HashSet<_>>();
    changes
        .into_iter()
        .filter(|change| paths.contains(change.path.as_str()))
        .collect()
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

struct ParsedHunk {
    hunk: DiffHunk,
    old_line: usize,
    new_line: usize,
}

fn parse_unified_diff(output: &str) -> Vec<FileDiff> {
    let mut files = Vec::new();
    let mut current_file: Option<FileDiff> = None;
    let mut current_hunk: Option<ParsedHunk> = None;

    for line in output.lines() {
        if line.starts_with("diff --git ") {
            finish_diff_file(&mut files, &mut current_file, &mut current_hunk);
            current_file = Some(parse_diff_git_header(line));
            continue;
        }

        let Some(file) = current_file.as_mut() else {
            continue;
        };

        if let Some((old_start, old_lines, new_start, new_lines, header)) = parse_hunk_header(line)
        {
            finish_hunk(file, &mut current_hunk);
            current_hunk = Some(ParsedHunk {
                hunk: DiffHunk {
                    old_start,
                    old_lines,
                    new_start,
                    new_lines,
                    header,
                    lines: Vec::new(),
                },
                old_line: old_start,
                new_line: new_start,
            });
            continue;
        }

        if let Some(hunk) = current_hunk.as_mut() {
            if parse_diff_line(line, hunk) {
                continue;
            }
        }

        parse_file_header_line(file, line);
    }

    finish_diff_file(&mut files, &mut current_file, &mut current_hunk);
    files
}

fn finish_diff_file(
    files: &mut Vec<FileDiff>,
    current_file: &mut Option<FileDiff>,
    current_hunk: &mut Option<ParsedHunk>,
) {
    if let Some(file) = current_file.as_mut() {
        finish_hunk(file, current_hunk);
    }
    if let Some(file) = current_file.take() {
        if file.binary || !file.hunks.is_empty() {
            files.push(file);
        }
    }
}

fn finish_hunk(file: &mut FileDiff, current_hunk: &mut Option<ParsedHunk>) {
    if let Some(parsed) = current_hunk.take()
        && !parsed.hunk.lines.is_empty()
    {
        file.hunks.push(parsed.hunk);
    }
}

fn parse_diff_git_header(line: &str) -> FileDiff {
    let rest = line.trim_start_matches("diff --git ");
    let (old_path, path) = rest
        .split_once(" b/")
        .map(|(old, new)| {
            (
                normalize_diff_path(old),
                normalize_diff_path(&format!("b/{new}")),
            )
        })
        .unwrap_or_else(|| (String::new(), normalize_diff_path(rest)));

    FileDiff {
        old_path: (old_path != path).then_some(old_path),
        path,
        kind: FileChangeKind::Modified,
        binary: false,
        hunks: Vec::new(),
        conflicts: Vec::new(),
    }
}

fn normalize_diff_path(path: &str) -> String {
    path.trim()
        .trim_matches('"')
        .strip_prefix("a/")
        .or_else(|| path.trim().trim_matches('"').strip_prefix("b/"))
        .unwrap_or_else(|| path.trim().trim_matches('"'))
        .to_string()
}

fn parse_file_header_line(file: &mut FileDiff, line: &str) {
    if line.starts_with("new file mode ") {
        file.kind = FileChangeKind::Created;
    } else if line.starts_with("deleted file mode ") {
        file.kind = FileChangeKind::Deleted;
    } else if let Some(path) = line.strip_prefix("rename from ") {
        file.kind = FileChangeKind::Renamed;
        file.old_path = Some(normalize_diff_path(path));
    } else if let Some(path) = line.strip_prefix("rename to ") {
        file.kind = FileChangeKind::Renamed;
        file.path = normalize_diff_path(path);
    } else if let Some(path) = line.strip_prefix("--- ") {
        let path = normalize_diff_path(path);
        if path == "/dev/null" {
            file.kind = FileChangeKind::Created;
            file.old_path = None;
        } else {
            file.old_path = (path != file.path).then_some(path);
        }
    } else if let Some(path) = line.strip_prefix("+++ ") {
        let path = normalize_diff_path(path);
        if path == "/dev/null" {
            file.kind = FileChangeKind::Deleted;
        } else {
            file.path = path;
        }
    } else if line.starts_with("Binary files ") {
        file.binary = true;
    }
}

fn parse_hunk_header(line: &str) -> Option<(usize, usize, usize, usize, String)> {
    let rest = line.strip_prefix("@@ ")?;
    let (ranges, _) = rest.split_once(" @@")?;
    let mut parts = ranges.split_whitespace();
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    let (old_start, old_lines) = parse_hunk_range(old)?;
    let (new_start, new_lines) = parse_hunk_range(new)?;
    Some((old_start, old_lines, new_start, new_lines, line.to_string()))
}

fn parse_hunk_range(range: &str) -> Option<(usize, usize)> {
    let (start, lines) = range.split_once(',').unwrap_or((range, "1"));
    Some((start.parse().ok()?, lines.parse().ok()?))
}

fn parse_diff_line(line: &str, hunk: &mut ParsedHunk) -> bool {
    let Some(prefix) = line.chars().next() else {
        return false;
    };
    let text = line.get(1..).unwrap_or_default().to_string();
    match prefix {
        ' ' => {
            hunk.hunk.lines.push(DiffLine {
                kind: DiffLineKind::Context,
                old_line: Some(hunk.old_line),
                new_line: Some(hunk.new_line),
                text,
            });
            hunk.old_line += 1;
            hunk.new_line += 1;
            true
        }
        '+' => {
            hunk.hunk.lines.push(DiffLine {
                kind: DiffLineKind::Added,
                old_line: None,
                new_line: Some(hunk.new_line),
                text,
            });
            hunk.new_line += 1;
            true
        }
        '-' => {
            hunk.hunk.lines.push(DiffLine {
                kind: DiffLineKind::Removed,
                old_line: Some(hunk.old_line),
                new_line: None,
                text,
            });
            hunk.old_line += 1;
            true
        }
        '\\' => true,
        _ => false,
    }
}

fn apply_status_to_file_diffs(files: &mut [FileDiff], work_dir: &Path, changes: &[FileChange]) {
    for file in files {
        let Some(change) = changes.iter().find(|change| {
            change.path == file.path || file.old_path.as_deref() == Some(&change.path)
        }) else {
            continue;
        };

        file.kind = change.kind;
        if change.kind == FileChangeKind::Conflicted {
            file.conflicts = read_conflicts(work_dir.join(&file.path));
        }
    }
}

fn synthesize_conflicted_file_diffs(
    files: &mut Vec<FileDiff>,
    work_dir: &Path,
    changes: &[FileChange],
) {
    let existing_paths = files
        .iter()
        .flat_map(|file| file.old_path.iter().chain(std::iter::once(&file.path)))
        .cloned()
        .collect::<HashSet<_>>();

    for change in changes {
        if change.kind != FileChangeKind::Conflicted || existing_paths.contains(&change.path) {
            continue;
        }
        files.push(synthesize_conflicted_file_diff(work_dir, &change.path));
    }
}

fn synthesize_conflicted_file_diff(work_dir: &Path, path: &str) -> FileDiff {
    let conflicts = read_conflicts(work_dir.join(path));
    FileDiff {
        old_path: None,
        path: path.to_string(),
        kind: FileChangeKind::Conflicted,
        binary: conflicts.is_empty(),
        hunks: Vec::new(),
        conflicts,
    }
}

fn read_conflicts(path: impl AsRef<Path>) -> Vec<ConflictBlock> {
    std::fs::read_to_string(path)
        .map(|content| parse_conflicts(&content))
        .unwrap_or_default()
}

fn parse_conflicts(content: &str) -> Vec<ConflictBlock> {
    enum ParsedConflict {
        Current {
            start_line: usize,
            current_label: String,
            current: Vec<ConflictLine>,
        },
        Incoming {
            start_line: usize,
            separator_line: usize,
            current_label: String,
            current: Vec<ConflictLine>,
            incoming: Vec<ConflictLine>,
        },
    }

    let mut conflicts = Vec::new();
    let mut current_conflict: Option<ParsedConflict> = None;

    for (ix, line) in content.lines().enumerate() {
        let line_number = ix + 1;
        if let Some(label) = conflict_marker_label(line, "<<<<<<<") {
            current_conflict = Some(ParsedConflict::Current {
                start_line: line_number,
                current_label: label,
                current: Vec::new(),
            });
            continue;
        }

        if line.starts_with("=======") {
            if let Some(ParsedConflict::Current {
                start_line,
                current_label,
                current,
            }) = current_conflict.take()
            {
                current_conflict = Some(ParsedConflict::Incoming {
                    start_line,
                    separator_line: line_number,
                    current_label,
                    current,
                    incoming: Vec::new(),
                });
            }
            continue;
        }

        if let Some(label) = conflict_marker_label(line, ">>>>>>>") {
            if let Some(ParsedConflict::Incoming {
                start_line,
                separator_line,
                current_label,
                current,
                incoming,
            }) = current_conflict.take()
            {
                conflicts.push(ConflictBlock {
                    start_line,
                    separator_line,
                    end_line: line_number,
                    current_label,
                    incoming_label: label,
                    current,
                    incoming,
                });
            }
            continue;
        }

        match current_conflict.as_mut() {
            Some(ParsedConflict::Current { current, .. }) => current.push(ConflictLine {
                line: line_number,
                text: line.to_string(),
            }),
            Some(ParsedConflict::Incoming { incoming, .. }) => incoming.push(ConflictLine {
                line: line_number,
                text: line.to_string(),
            }),
            None => {}
        }
    }

    conflicts
}

fn conflict_marker_label(line: &str, marker: &str) -> Option<String> {
    line.strip_prefix(marker)
        .map(str::trim)
        .map(ToString::to_string)
}

pub fn resolve_conflict_content(
    content: &str,
    start_line: usize,
    resolution: ConflictResolution,
) -> Option<String> {
    let conflicts = parse_conflicts(content);
    let conflict = conflicts
        .into_iter()
        .find(|conflict| conflict.start_line == start_line)?;
    let mut lines = content.lines().map(ToString::to_string).collect::<Vec<_>>();
    let replacement = match resolution {
        ConflictResolution::Current => conflict.current,
        ConflictResolution::Incoming => conflict.incoming,
        ConflictResolution::Both => conflict
            .current
            .into_iter()
            .chain(conflict.incoming)
            .collect::<Vec<_>>(),
    };
    let replacement = replacement
        .into_iter()
        .map(|line| line.text)
        .collect::<Vec<_>>();

    lines.splice(conflict.start_line - 1..conflict.end_line, replacement);
    let mut resolved = lines.join("\n");
    if content.ends_with('\n') {
        resolved.push('\n');
    }
    Some(resolved)
}

fn synthesize_created_file_diffs(
    files: &mut Vec<FileDiff>,
    work_dir: &Path,
    changes: &[FileChange],
) {
    let existing_paths = files
        .iter()
        .flat_map(|file| file.old_path.iter().chain(std::iter::once(&file.path)))
        .cloned()
        .collect::<HashSet<_>>();

    for change in changes {
        if change.kind != FileChangeKind::Created || existing_paths.contains(&change.path) {
            continue;
        }
        files.push(synthesize_created_file_diff(work_dir, &change.path));
    }
}

fn synthesize_created_file_diff(work_dir: &Path, path: &str) -> FileDiff {
    let Ok(content) = std::fs::read_to_string(work_dir.join(path)) else {
        return FileDiff {
            old_path: None,
            path: path.to_string(),
            kind: FileChangeKind::Created,
            binary: true,
            hunks: Vec::new(),
            conflicts: Vec::new(),
        };
    };

    let lines = content.lines().collect::<Vec<_>>();
    let diff_lines = lines
        .iter()
        .enumerate()
        .map(|(ix, line)| DiffLine {
            kind: DiffLineKind::Added,
            old_line: None,
            new_line: Some(ix + 1),
            text: (*line).to_string(),
        })
        .collect::<Vec<_>>();
    let hunks = if diff_lines.is_empty() {
        Vec::new()
    } else {
        vec![DiffHunk {
            old_start: 0,
            old_lines: 0,
            new_start: 1,
            new_lines: diff_lines.len(),
            header: format!("@@ -0,0 +1,{} @@", diff_lines.len()),
            lines: diff_lines,
        }]
    };

    FileDiff {
        old_path: None,
        path: path.to_string(),
        kind: FileChangeKind::Created,
        binary: false,
        hunks,
        conflicts: Vec::new(),
    }
}

fn parse_numstat(output: &str) -> HashMap<String, DiffStats> {
    output.lines().filter_map(parse_numstat_line).collect()
}

fn parse_numstat_line(line: &str) -> Option<(String, DiffStats)> {
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

fn read_branch_sync_status(path: &Path) -> Result<BranchSyncStatus, Error> {
    let output = git_output(
        path,
        &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
    )?;
    Ok(parse_branch_sync_status(&output).unwrap_or_default())
}

fn parse_branch_sync_status(output: &str) -> Option<BranchSyncStatus> {
    let mut parts = output.split_whitespace();
    let ahead = parts.next()?.parse().ok()?;
    let behind = parts.next()?.parse().ok()?;
    Some(BranchSyncStatus { ahead, behind })
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
            let files = parse_stash_files(&git_output(
                path,
                &[
                    "stash",
                    "show",
                    "--include-untracked",
                    "--numstat",
                    &stash_id,
                ],
            )?);
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

fn parse_stash_files(output: &str) -> Vec<StashFile> {
    output
        .lines()
        .filter_map(parse_numstat_line)
        .map(|(path, stats)| StashFile {
            path,
            insertions: stats.insertions,
            deletions: stats.deletions,
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
    let output = run_git_command(path, args, false)?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn run_git(path: &Path, args: &[&str]) -> Result<(), Error> {
    run_git_command(path, args, true).map(|_| ())
}

fn run_git_command(
    path: &Path,
    args: &[&str],
    allow_optional_locks: bool,
) -> Result<Output, Error> {
    let work_dir = git_work_dir(path)?;
    let mut command = std::process::Command::new("git");
    command.args(args).current_dir(work_dir);
    if !allow_optional_locks {
        // Background reads must not create .git/index.lock while a write is starting.
        command.env("GIT_OPTIONAL_LOCKS", "0");
    }

    let output = command
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
    fn parses_unified_diff_hunks() {
        let files = parse_unified_diff(concat!(
            "diff --git a/src/lib.rs b/src/lib.rs\n",
            "index 111..222 100644\n",
            "--- a/src/lib.rs\n",
            "+++ b/src/lib.rs\n",
            "@@ -1,3 +1,4 @@\n",
            " one\n",
            "-two\n",
            "+two changed\n",
            "+three\n",
            " four\n",
        ));

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "src/lib.rs");
        assert_eq!(files[0].kind, FileChangeKind::Modified);
        assert_eq!(files[0].hunks.len(), 1);
        assert_eq!(files[0].hunks[0].old_start, 1);
        assert_eq!(files[0].hunks[0].new_start, 1);
        assert_eq!(files[0].hunks[0].lines.len(), 5);
        assert_eq!(files[0].hunks[0].lines[1].kind, DiffLineKind::Removed);
        assert_eq!(files[0].hunks[0].lines[1].old_line, Some(2));
        assert_eq!(files[0].hunks[0].lines[2].kind, DiffLineKind::Added);
        assert_eq!(files[0].hunks[0].lines[2].new_line, Some(2));
    }

    #[test]
    fn parses_renamed_diff_paths() {
        let files = parse_unified_diff(concat!(
            "diff --git a/old.rs b/new.rs\n",
            "similarity index 90%\n",
            "rename from old.rs\n",
            "rename to new.rs\n",
            "--- a/old.rs\n",
            "+++ b/new.rs\n",
            "@@ -1 +1 @@\n",
            "-old\n",
            "+new\n",
        ));

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].kind, FileChangeKind::Renamed);
        assert_eq!(files[0].old_path.as_deref(), Some("old.rs"));
        assert_eq!(files[0].path, "new.rs");
    }

    #[test]
    fn parses_stash_files_with_stats() {
        let files =
            parse_stash_files("3\t1\tsrc/lib.rs\n-\t-\tassets/logo.png\n2\t0\told => new.rs\n");

        assert_eq!(files.len(), 3);
        assert_eq!(files[0].path, "src/lib.rs");
        assert_eq!(files[0].insertions, 3);
        assert_eq!(files[0].deletions, 1);
        assert_eq!(files[1].path, "assets/logo.png");
        assert_eq!(files[1].insertions, 0);
        assert_eq!(files[1].deletions, 0);
        assert_eq!(files[2].path, "new.rs");
        assert_eq!(files[2].insertions, 2);
        assert_eq!(files[2].deletions, 0);
    }

    #[test]
    fn parses_branch_sync_status() {
        assert_eq!(
            parse_branch_sync_status("2\t3\n"),
            Some(BranchSyncStatus {
                ahead: 2,
                behind: 3,
            })
        );
        assert_eq!(parse_branch_sync_status("bad\t3\n"), None);
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
    fn parses_conflict_blocks() {
        let conflicts = parse_conflicts(concat!(
            "before\n",
            "<<<<<<< HEAD\n",
            "current one\n",
            "current two\n",
            "=======\n",
            "incoming one\n",
            ">>>>>>> feature/auth\n",
            "after\n",
        ));

        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].start_line, 2);
        assert_eq!(conflicts[0].separator_line, 5);
        assert_eq!(conflicts[0].end_line, 7);
        assert_eq!(conflicts[0].current_label, "HEAD");
        assert_eq!(conflicts[0].incoming_label, "feature/auth");
        assert_eq!(conflicts[0].current[0].line, 3);
        assert_eq!(conflicts[0].current[0].text, "current one");
        assert_eq!(conflicts[0].incoming[0].line, 6);
        assert_eq!(conflicts[0].incoming[0].text, "incoming one");
    }

    #[test]
    fn resolves_conflict_content() {
        let content = concat!(
            "before\n",
            "<<<<<<< HEAD\n",
            "current\n",
            "=======\n",
            "incoming\n",
            ">>>>>>> feature\n",
            "after\n",
        );

        assert_eq!(
            resolve_conflict_content(content, 2, ConflictResolution::Current).as_deref(),
            Some("before\ncurrent\nafter\n"),
        );
        assert_eq!(
            resolve_conflict_content(content, 2, ConflictResolution::Incoming).as_deref(),
            Some("before\nincoming\nafter\n"),
        );
        assert_eq!(
            resolve_conflict_content(content, 2, ConflictResolution::Both).as_deref(),
            Some("before\ncurrent\nincoming\nafter\n"),
        );
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
