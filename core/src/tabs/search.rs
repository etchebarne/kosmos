use std::error::Error as StdError;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use ignore::{DirEntry, WalkBuilder};

use super::editor::{EditorDocument, EditorError, MAX_EDITOR_FILE_BYTES};

pub type Result<T> = std::result::Result<T, SearchError>;

const MAX_QUERY_BYTES: usize = 256;
const MAX_WALKED_ENTRIES: usize = 50_000;
const MAX_CONTENT_BYTES: usize = 64 * 1024 * 1024;
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
    content_bytes: usize,
    results: usize,
    matches_per_file: usize,
}

impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            walked_entries: MAX_WALKED_ENTRIES,
            content_bytes: MAX_CONTENT_BYTES,
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
    let mut content_bytes = 0usize;
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
                let Some(bytes) = read_searchable_file(entry.path()) else {
                    continue;
                };
                if content_bytes.saturating_add(bytes.len()) > limits.content_bytes {
                    limit_reached = true;
                    continue;
                }
                content_bytes += bytes.len();
                let Ok(content) = std::str::from_utf8(&bytes) else {
                    continue;
                };
                if content.contains('\0') {
                    continue;
                }

                let mut file_matches = 0;
                for (line_index, line) in content.lines().enumerate() {
                    if !line.to_lowercase().contains(&query) {
                        continue;
                    }
                    if file_matches == limits.matches_per_file {
                        limit_reached = true;
                        break;
                    }

                    matches.push(SearchMatch {
                        path: path.clone(),
                        line_number: u32::try_from(line_index + 1).ok(),
                        preview: Some(truncated_preview(line)),
                    });
                    file_matches += 1;
                    if matches.len() == limits.results {
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

fn read_searchable_file(path: &Path) -> Option<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_EDITOR_FILE_BYTES as u64 {
        return None;
    }

    let file = File::open(path).ok()?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take((MAX_EDITOR_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;

    (bytes.len() <= MAX_EDITOR_FILE_BYTES).then_some(bytes)
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
            content_bytes: 1024,
            results: 1,
            matches_per_file: 10,
        };

        let results = search_with_limits(&root, "match", SearchMode::Content, limits).unwrap();

        assert_eq!(results.matches().len(), 1);
        assert!(results.limit_reached());
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
