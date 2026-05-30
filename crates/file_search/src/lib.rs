use std::{
    collections::hash_map::DefaultHasher,
    fmt,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use fff_search::{
    FFFMode, FilePicker, FilePickerOptions, FileSearchConfig, FrecencyTracker, FuzzySearchOptions,
    PaginationArgs, QueryParser, QueryTracker, SharedFilePicker, SharedFrecency,
    SharedQueryTracker,
};

#[derive(Debug)]
pub enum Error {
    FilePickerMissing,
    Fff(fff_search::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FilePickerMissing => write!(f, "file search index is not available"),
            Self::Fff(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<fff_search::Error> for Error {
    fn from(error: fff_search::Error) -> Self {
        Self::Fff(error)
    }
}

#[derive(Clone, Debug)]
pub struct FileSearchResult {
    pub name: String,
    pub relative_path: String,
    pub absolute_path: PathBuf,
    pub score: i32,
    pub size: u64,
    pub is_binary: bool,
}

#[derive(Clone, Debug)]
pub struct FileSearchSnapshot {
    pub query: String,
    pub total_matched: usize,
    pub total_files: usize,
    pub scanned_files_count: usize,
    pub is_scanning: bool,
    pub is_watcher_ready: bool,
    pub results: Vec<FileSearchResult>,
}

pub struct FileSearchIndex {
    root: PathBuf,
    picker: SharedFilePicker,
    frecency: SharedFrecency,
    query_tracker: SharedQueryTracker,
}

impl FileSearchIndex {
    pub fn new(root: PathBuf) -> Result<Self, Error> {
        let picker = SharedFilePicker::default();
        let frecency = SharedFrecency::default();
        let query_tracker = SharedQueryTracker::default();

        install_persistent_trackers(&root, &frecency, &query_tracker);

        FilePicker::new_with_shared_state(
            picker.clone(),
            frecency.clone(),
            FilePickerOptions {
                base_path: root.to_string_lossy().into_owned(),
                mode: FFFMode::Neovim,
                watch: true,
                follow_symlinks: false,
                enable_mmap_cache: false,
                enable_content_indexing: false,
                ..Default::default()
            },
        )?;

        Ok(Self {
            root,
            picker,
            frecency,
            query_tracker,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn status(&self, query: &str) -> Result<FileSearchSnapshot, Error> {
        let picker_guard = self.picker.read()?;
        let picker = picker_guard.as_ref().ok_or(Error::FilePickerMissing)?;
        let progress = picker.get_scan_progress();

        Ok(FileSearchSnapshot {
            query: query.to_string(),
            total_matched: 0,
            total_files: picker.live_file_count(),
            scanned_files_count: progress.scanned_files_count,
            is_scanning: progress.is_scanning || picker.is_post_scan_active(),
            is_watcher_ready: progress.is_watcher_ready,
            results: Vec::new(),
        })
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<FileSearchSnapshot, Error> {
        if query.trim().is_empty() {
            return self.status(query);
        }

        let picker_guard = self.picker.read()?;
        let picker = picker_guard.as_ref().ok_or(Error::FilePickerMissing)?;
        let progress = picker.get_scan_progress();
        let parser: QueryParser<FileSearchConfig> = QueryParser::default();
        let parsed_query = parser.parse(query);
        let query_tracker_guard = self.query_tracker.read()?;
        let result = picker.fuzzy_search(
            &parsed_query,
            query_tracker_guard.as_ref(),
            FuzzySearchOptions {
                max_threads: search_threads(),
                current_file: None,
                project_path: Some(&self.root),
                combo_boost_score_multiplier: 100,
                min_combo_count: 2,
                pagination: PaginationArgs { offset: 0, limit },
            },
        );

        let results = result
            .items
            .iter()
            .zip(result.scores.iter())
            .map(|(item, score)| FileSearchResult {
                name: item.file_name(picker),
                relative_path: item.relative_path(picker),
                absolute_path: item.absolute_path(picker, picker.base_path()),
                score: score.total,
                size: item.size,
                is_binary: item.is_binary(),
            })
            .collect();

        Ok(FileSearchSnapshot {
            query: query.to_string(),
            total_matched: result.total_matched,
            total_files: result.total_files,
            scanned_files_count: progress.scanned_files_count,
            is_scanning: progress.is_scanning || picker.is_post_scan_active(),
            is_watcher_ready: progress.is_watcher_ready,
            results,
        })
    }

    pub fn track_open(&self, query: &str, path: &Path) {
        if let Ok(frecency_guard) = self.frecency.read()
            && let Some(frecency) = frecency_guard.as_ref()
        {
            let _ = frecency.track_access(path);
            if let Ok(mut picker_guard) = self.picker.write()
                && let Some(picker) = picker_guard.as_mut()
            {
                let _ = picker.update_single_file_frecency(path, frecency);
            }
        }

        let query = query.trim();
        if query.is_empty() {
            return;
        }

        if let Ok(mut query_tracker_guard) = self.query_tracker.write()
            && let Some(query_tracker) = query_tracker_guard.as_mut()
        {
            let _ = query_tracker.track_query_completion(query, &self.root, path);
        }
    }
}

fn search_threads() -> usize {
    1
}

fn install_persistent_trackers(
    root: &Path,
    frecency: &SharedFrecency,
    query_tracker: &SharedQueryTracker,
) {
    let Some(cache_dir) = workspace_cache_dir(root) else {
        return;
    };

    if let Ok(tracker) = FrecencyTracker::open(cache_dir.join("frecency")) {
        let _ = frecency.init(tracker);
    }

    if let Ok(tracker) = QueryTracker::open(cache_dir.join("queries")) {
        let _ = query_tracker.init(tracker);
    }
}

fn workspace_cache_dir(root: &Path) -> Option<PathBuf> {
    let mut hasher = DefaultHasher::new();
    root.hash(&mut hasher);
    let key = format!("{:016x}", hasher.finish());
    let dir = cache_home()?.join("kosmos").join("fff").join(key);
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn cache_home() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("XDG_CACHE_HOME") {
        return Some(PathBuf::from(path));
    }

    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".cache"))
        .or_else(|| Some(std::env::temp_dir()))
}
