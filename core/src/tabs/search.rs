use std::error::Error as StdError;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use ignore::{DirEntry, WalkBuilder};

use super::editor::{EditorDocument, EditorError};

pub type Result<T> = std::result::Result<T, SearchError>;

const MAX_QUERY_BYTES: usize = 256;
const MAX_WALKED_ENTRIES: usize = 50_000;
/// Total bytes content search may scan across all files before it stops and
/// reports partial results.
const MAX_SCANNED_BYTES: usize = 512 * 1024 * 1024;
/// Files larger than this are not scanned; the results are reported as partial.
const MAX_SEARCH_FILE_BYTES: u64 = 256 * 1024 * 1024;
/// Leading bytes inspected for a NUL byte to skip binary files cheaply.
const BINARY_SNIFF_BYTES: usize = 8 * 1024;
const READ_BUFFER_BYTES: usize = 64 * 1024;
const MAX_RESULTS: usize = 250;
const MAX_MATCHES_PER_FILE: usize = 20;
const MAX_PREVIEW_CHARS: usize = 240;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchMode {
    Name,
    Content,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchMatch {
    path: String,
    line_number: Option<u32>,
    preview: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSearchResults {
    matches: Vec<SearchMatch>,
    limit_reached: bool,
}

pub struct WorkspaceSearch;

impl SearchMatch {
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn line_number(&self) -> Option<u32> {
        self.line_number
    }

    pub fn preview(&self) -> Option<&str> {
        self.preview.as_deref()
    }
}

impl WorkspaceSearchResults {
    pub fn matches(&self) -> &[SearchMatch] {
        &self.matches
    }

    pub fn limit_reached(&self) -> bool {
        self.limit_reached
    }
}

impl WorkspaceSearch {
    pub fn query(
        workspace_directory: impl AsRef<Path>,
        query: &str,
        mode: SearchMode,
    ) -> Result<WorkspaceSearchResults> {
        search_with_limits(
            workspace_directory.as_ref(),
            query,
            mode,
            SearchLimits::default(),
        )
    }

    pub fn document(workspace_directory: impl AsRef<Path>, path: &str) -> Result<EditorDocument> {
        EditorDocument::read(workspace_directory, path).map_err(SearchError::Document)
    }
}

#[derive(Clone, Copy)]
struct SearchLimits {
    walked_entries: usize,
    scanned_bytes: usize,
    results: usize,
    matches_per_file: usize,
}

impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            walked_entries: MAX_WALKED_ENTRIES,
            scanned_bytes: MAX_SCANNED_BYTES,
            results: MAX_RESULTS,
            matches_per_file: MAX_MATCHES_PER_FILE,
        }
    }
}

fn search_with_limits(
    workspace_directory: &Path,
    query: &str,
    mode: SearchMode,
    limits: SearchLimits,
) -> Result<WorkspaceSearchResults> {
    validate_root(workspace_directory)?;
    let query = query.trim();
    if query.len() > MAX_QUERY_BYTES {
        return Err(SearchError::QueryTooLong {
            max_bytes: MAX_QUERY_BYTES,
        });
    }
    if query.is_empty() {
        return Ok(WorkspaceSearchResults {
            matches: Vec::new(),
            limit_reached: false,
        });
    }

    let query = query.to_lowercase();
    let mut matches = Vec::new();
    let mut walked_entries = 0;
    let mut scanned_bytes = 0usize;
    let mut limit_reached = false;
    let mut builder = WalkBuilder::new(workspace_directory);
    builder
        .hidden(false)
        .follow_links(false)
        .filter_entry(|entry| !is_git_directory(entry));

    for entry in builder.build() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                limit_reached = true;
                continue;
            }
        };
        if entry.depth() == 0 {
            continue;
        }

        walked_entries += 1;
        if walked_entries > limits.walked_entries {
            limit_reached = true;
            break;
        }
        if !entry
            .file_type()
            .is_some_and(|file_type| file_type.is_file())
        {
            continue;
        }

        let Some(path) = relative_utf8_path(workspace_directory, entry.path()) else {
            continue;
        };
        match mode {
            SearchMode::Name => {
                let file_name = entry.file_name().to_string_lossy().to_lowercase();
                if file_name.contains(&query) {
                    matches.push(SearchMatch {
                        path,
                        line_number: None,
                        preview: None,
                    });
                }
            }
            SearchMode::Content => {
                if exceeds_search_file_limit(&entry) {
                    limit_reached = true;
                    continue;
                }

                let scan = scan_file_content(
                    entry.path(),
                    &path,
                    &query,
                    ScanLimits {
                        matches: limits.matches_per_file.min(limits.results - matches.len()),
                        bytes: limits.scanned_bytes - scanned_bytes,
                    },
                );
                scanned_bytes += scan.scanned_bytes;
                matches.extend(scan.matches);
                match scan.end {
                    ScanEnd::Complete => {}
                    ScanEnd::MatchLimit => limit_reached = true,
                    ScanEnd::ByteBudget => {
                        limit_reached = true;
                        break;
                    }
                }
            }
        }

        if matches.len() == limits.results {
            limit_reached = true;
            break;
        }
    }

    matches.sort_by(|left, right| {
        left.path
            .to_lowercase()
            .cmp(&right.path.to_lowercase())
            .then(left.line_number.cmp(&right.line_number))
    });

    Ok(WorkspaceSearchResults {
        matches,
        limit_reached,
    })
}

fn validate_root(root: &Path) -> Result<()> {
    let metadata = fs::metadata(root).map_err(|source| SearchError::Io {
        path: root.to_path_buf(),
        source,
    })?;
    if metadata.is_dir() {
        Ok(())
    } else {
        Err(SearchError::WorkspaceNotDirectory(root.to_path_buf()))
    }
}

fn is_git_directory(entry: &DirEntry) -> bool {
    entry
        .file_type()
        .is_some_and(|file_type| file_type.is_dir())
        && entry.file_name() == ".git"
}

fn relative_utf8_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let components = relative
        .components()
        .map(|component| component.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()?;

    Some(components.join("/"))
}

#[derive(Clone, Copy)]
struct ScanLimits {
    matches: usize,
    bytes: usize,
}

#[derive(Default)]
struct ContentScan {
    matches: Vec<SearchMatch>,
    scanned_bytes: usize,
    end: ScanEnd,
}

/// Why a file scan stopped; anything but `Complete` means more matches may
/// exist.
#[derive(Default)]
enum ScanEnd {
    #[default]
    Complete,
    MatchLimit,
    ByteBudget,
}

fn exceeds_search_file_limit(entry: &DirEntry) -> bool {
    entry
        .metadata()
        .is_ok_and(|metadata| metadata.len() > MAX_SEARCH_FILE_BYTES)
}

/// Streams a text file line by line, collecting matches for the lowercase
/// `query`. Binary, non-UTF-8 and unreadable files yield no matches.
fn scan_file_content(
    path: &Path,
    relative_path: &str,
    query: &str,
    limits: ScanLimits,
) -> ContentScan {
    let mut scan = ContentScan::default();
    let Some(mut reader) = open_text_file(path) else {
        return scan;
    };
    let mut line = Vec::new();
    let mut line_number = 0usize;

    loop {
        line.clear();
        let Ok(read) = reader.read_until(b'\n', &mut line) else {
            scan.matches.clear();
            return scan;
        };
        if read == 0 {
            return scan;
        }
        if scan.scanned_bytes + read > limits.bytes {
            scan.end = ScanEnd::ByteBudget;
            return scan;
        }
        scan.scanned_bytes += read;
        line_number += 1;

        let Some(text) = text_line(&line) else {
            scan.matches.clear();
            return scan;
        };
        if !text.to_lowercase().contains(query) {
            continue;
        }
        if scan.matches.len() == limits.matches {
            scan.end = ScanEnd::MatchLimit;
            return scan;
        }
        scan.matches.push(SearchMatch {
            path: relative_path.to_owned(),
            line_number: u32::try_from(line_number).ok(),
            preview: Some(truncated_preview(text)),
        });
    }
}

/// Opens a regular file for buffered reading, rejecting files whose leading
/// bytes contain a NUL byte.
fn open_text_file(path: &Path) -> Option<BufReader<io::Take<File>>> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::with_capacity(READ_BUFFER_BYTES, file.take(MAX_SEARCH_FILE_BYTES));
    let head = reader.fill_buf().ok()?;
    let sniffed = &head[..head.len().min(BINARY_SNIFF_BYTES)];

    (!sniffed.contains(&0)).then_some(reader)
}

/// Decodes one raw line without its line ending, returning `None` for bytes
/// that mark the file as binary or non-UTF-8.
fn text_line(line: &[u8]) -> Option<&str> {
    let line = match line.strip_suffix(b"\n") {
        Some(line) => line.strip_suffix(b"\r").unwrap_or(line),
        None => line,
    };
    if line.contains(&0) {
        return None;
    }

    std::str::from_utf8(line).ok()
}

fn truncated_preview(line: &str) -> String {
    let line = line.trim();
    let mut chars = line.chars();
    let preview = chars.by_ref().take(MAX_PREVIEW_CHARS).collect::<String>();

    if chars.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

#[derive(Debug)]
pub enum SearchError {
    WorkspaceNotFound,
    TabNotFound,
    WorkspaceNotDirectory(PathBuf),
    QueryTooLong { max_bytes: usize },
    Document(EditorError),
    Io { path: PathBuf, source: io::Error },
}

impl fmt::Display for SearchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WorkspaceNotFound => formatter.write_str("workspace does not exist"),
            Self::TabNotFound => formatter.write_str("search tab does not exist"),
            Self::WorkspaceNotDirectory(path) => {
                write!(
                    formatter,
                    "workspace is not a directory: {}",
                    path.display()
                )
            }
            Self::QueryTooLong { max_bytes } => {
                write!(formatter, "search query exceeds the {max_bytes}-byte limit")
            }
            Self::Document(error) => write!(formatter, "could not load search result: {error}"),
            Self::Io { path, source } => {
                write!(formatter, "could not access {}: {source}", path.display())
            }
        }
    }
}

impl StdError for SearchError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Document(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn searches_names_case_insensitively_and_respects_ignores() {
        let root = test_directory("names");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("ignored")).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".gitignore"), "ignored/\n").unwrap();
        fs::write(root.join("src/SearchPanel.tsx"), "export {};").unwrap();
        fs::write(root.join("ignored/search.txt"), "ignored").unwrap();
        fs::write(root.join(".git/search.txt"), "ignored").unwrap();

        let results = WorkspaceSearch::query(&root, "search", SearchMode::Name).unwrap();

        assert_eq!(results.matches().len(), 1);
        assert_eq!(results.matches()[0].path(), "src/SearchPanel.tsx");
        assert_eq!(results.matches()[0].line_number(), None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn searches_content_with_line_numbers_and_skips_binary_files() {
        let root = test_directory("content");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("main.rs"), "first\nNeedle here\nneedle again\n").unwrap();
        fs::write(root.join("binary.dat"), b"needle\0binary").unwrap();

        let results = WorkspaceSearch::query(&root, "NEEDLE", SearchMode::Content).unwrap();

        assert_eq!(results.matches().len(), 2);
        assert_eq!(results.matches()[0].line_number(), Some(2));
        assert_eq!(results.matches()[0].preview(), Some("Needle here"));
        assert_eq!(results.matches()[1].line_number(), Some(3));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reports_when_test_limits_truncate_results() {
        let root = test_directory("limits");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("matches.txt"), "match\nmatch\n").unwrap();
        let limits = SearchLimits {
            walked_entries: 10,
            scanned_bytes: 1024,
            results: 1,
            matches_per_file: 10,
        };

        let results = search_with_limits(&root, "match", SearchMode::Content, limits).unwrap();

        assert_eq!(results.matches().len(), 1);
        assert!(results.limit_reached());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn finds_matches_in_files_larger_than_the_editor_limit() {
        let root = test_directory("large");
        fs::create_dir_all(&root).unwrap();
        let mut content = "filler line\n".repeat(200_000);
        content.push_str("the Needle is last\r\n");
        fs::write(root.join("large.log"), content).unwrap();

        let results = WorkspaceSearch::query(&root, "needle", SearchMode::Content).unwrap();

        assert_eq!(results.matches().len(), 1);
        assert_eq!(results.matches()[0].line_number(), Some(200_001));
        assert_eq!(results.matches()[0].preview(), Some("the Needle is last"));
        assert!(!results.limit_reached());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn skips_files_with_late_nul_bytes_or_invalid_utf8() {
        let root = test_directory("late-binary");
        fs::create_dir_all(&root).unwrap();
        let mut late_nul = format!("needle\n{}", "filler\n".repeat(2_000)).into_bytes();
        late_nul.extend_from_slice(b"tail\0");
        fs::write(root.join("late-nul.dat"), late_nul).unwrap();
        fs::write(root.join("latin1.txt"), b"needle\n\xff\n").unwrap();

        let results = WorkspaceSearch::query(&root, "needle", SearchMode::Content).unwrap();

        assert!(results.matches().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stops_scanning_at_per_file_match_and_byte_limits() {
        let root = test_directory("scan-limits");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("many.txt"), "match\n".repeat(10)).unwrap();
        let match_limits = SearchLimits {
            walked_entries: 10,
            scanned_bytes: 1024,
            results: 100,
            matches_per_file: 3,
        };
        let byte_limits = SearchLimits {
            scanned_bytes: 20,
            matches_per_file: 100,
            ..match_limits
        };

        let per_file =
            search_with_limits(&root, "match", SearchMode::Content, match_limits).unwrap();
        let budget = search_with_limits(&root, "match", SearchMode::Content, byte_limits).unwrap();

        assert_eq!(per_file.matches().len(), 3);
        assert!(per_file.limit_reached());
        assert_eq!(budget.matches().len(), 3);
        assert!(budget.limit_reached());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn empty_and_oversized_queries_are_handled_without_walking() {
        let root = test_directory("query");
        fs::create_dir_all(&root).unwrap();

        let empty = WorkspaceSearch::query(&root, "  ", SearchMode::Name).unwrap();
        assert!(empty.matches().is_empty());
        assert!(matches!(
            WorkspaceSearch::query(&root, &"x".repeat(MAX_QUERY_BYTES + 1), SearchMode::Name),
            Err(SearchError::QueryTooLong { .. })
        ));
        fs::remove_dir_all(root).unwrap();
    }

    fn test_directory(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("kosmos-search-{label}-{unique}"))
    }
}
