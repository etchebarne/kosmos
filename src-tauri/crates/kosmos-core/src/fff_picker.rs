//! Workspace file picker backed by [`fff-search`][fff].
//!
//! [`FffPicker`] wraps the heavy bits of the fff SDK — persistent frecency DB,
//! background scanner, filesystem watcher — behind a small, thread-safe handle
//! that the Tauri host and the remote agent can both hold.
//!
//! [fff]: https://github.com/dmtrKovalenko/fff.nvim
//!
//! ## Lifecycle
//!
//! - [`FffPicker::new`] opens (or creates) the frecency LMDB database.
//! - [`FffPicker::set_workspace`] spins up a new `FilePicker` for the given
//!   root. Calling it again for a different root replaces the active picker.
//! - [`FffPicker::search`] runs a fuzzy query against the current picker,
//!   returning scored hits plus match indices recomputed on the top-N with
//!   `nucleo-matcher` for UI highlighting.
//! - [`FffPicker::track_access`] records an access in the frecency DB so
//!   subsequent searches boost recently-opened files.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use fff_search::{
    FilePicker, FilePickerOptions, FrecencyTracker, FuzzySearchOptions, GrepMode,
    GrepSearchOptions, PaginationArgs, QueryParser, SharedFrecency, SharedPicker,
    parse_grep_query,
};

use crate::search::ContentMatch;
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config as NucleoConfig, Matcher as NucleoMatcher, Utf32Str};
use serde::{Deserialize, Serialize};

/// A single fuzzy-matched file, ready for the UI.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FffHit {
    /// Absolute path on disk (local or remote, depending on who ran the search).
    pub path: String,
    /// Bare filename (no directories).
    pub name: String,
    /// Path relative to the workspace root.
    pub relative_path: String,
    /// Combined score from fff (higher is better).
    pub score: i32,
    /// Byte indices into `relative_path` that matched the query.
    ///
    /// Recomputed with `nucleo-matcher` for the top-N results only — fff's
    /// own scorer drops indices. An empty vector means "no highlight info".
    pub indices: Vec<u32>,
}

/// Shared, reusable workspace picker.
///
/// Clones are cheap — internally this is an `Arc` on the shared state.
#[derive(Clone)]
pub struct FffPicker {
    picker: SharedPicker,
    frecency: SharedFrecency,
    /// Guards the currently-active workspace root. Used to avoid re-initializing
    /// the picker when the same workspace is set repeatedly.
    current_root: std::sync::Arc<Mutex<Option<PathBuf>>>,
}

impl FffPicker {
    /// Open the frecency database at `frecency_db_path`. The parent directory
    /// is created if missing.
    pub fn new(frecency_db_path: impl Into<PathBuf>) -> Result<Self, String> {
        let db_path = frecency_db_path.into();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("creating frecency db dir {parent:?}: {e}"))?;
        }

        let picker = SharedPicker::default();
        let frecency = SharedFrecency::default();

        let tracker = FrecencyTracker::new(&db_path, false)
            .map_err(|e| format!("opening frecency db at {db_path:?}: {e}"))?;
        frecency
            .init(tracker)
            .map_err(|e| format!("initializing shared frecency: {e}"))?;

        Ok(Self {
            picker,
            frecency,
            current_root: std::sync::Arc::new(Mutex::new(None)),
        })
    }

    /// Index (or re-index) the given workspace root. Idempotent for the same
    /// `base_path` — subsequent calls are no-ops.
    pub fn set_workspace(&self, base_path: &Path) -> Result<(), String> {
        let canonical = base_path.to_path_buf();
        {
            let mut current = self.current_root.lock().unwrap();
            if current.as_deref() == Some(base_path) {
                return Ok(());
            }
            *current = Some(canonical.clone());
        }

        let options = FilePickerOptions {
            base_path: canonical.to_string_lossy().into_owned(),
            watch: true,
            ..Default::default()
        };

        FilePicker::new_with_shared_state(self.picker.clone(), self.frecency.clone(), options)
            .map_err(|e| format!("initializing fff FilePicker: {e}"))?;
        Ok(())
    }

    /// Run a fuzzy search against the current workspace. Returns up to
    /// `limit` hits sorted by fff's combined score (frecency + fuzzy).
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<FffHit>, String> {
        let limit = limit.max(1);
        let guard = self
            .picker
            .read()
            .map_err(|e| format!("reading picker: {e}"))?;
        let Some(picker) = guard.as_ref() else {
            return Ok(Vec::new());
        };

        let parser = QueryParser::default();
        let parsed = parser.parse(query);

        let opts = FuzzySearchOptions {
            pagination: PaginationArgs { offset: 0, limit },
            ..Default::default()
        };

        let result = FilePicker::fuzzy_search(picker.get_files(), &parsed, None, opts);

        // Recover match indices for highlighting by re-running nucleo-matcher
        // on the (already filtered) top-N relative paths. Cheap — at most
        // `limit` entries and nucleo is a single pass per string.
        let mut matcher = NucleoMatcher::new(NucleoConfig::DEFAULT.match_paths());
        let pattern = Pattern::new(
            query.trim(),
            CaseMatching::Smart,
            Normalization::Smart,
            AtomKind::Fuzzy,
        );

        let hits: Vec<FffHit> = result
            .items
            .iter()
            .zip(result.scores.iter())
            .map(|(item, score)| {
                let relative_path = item.relative_path.to_string();
                let indices = highlight_indices(&mut matcher, &pattern, &relative_path);
                FffHit {
                    path: item.path.to_string_lossy().into_owned(),
                    name: item.file_name.to_string(),
                    relative_path,
                    score: score.total,
                    indices,
                }
            })
            .collect();

        Ok(hits)
    }

    /// Live content search (grep) over the indexed workspace.
    ///
    /// Uses fff's mmap-backed, rayon-parallel grep engine, reusing the file
    /// index built by [`set_workspace`](Self::set_workspace). Picks between
    /// literal and regex mode via `use_regex`. Returns at most `max_results`
    /// matches — the file list is searched in frecency order so the most
    /// relevant files come back first.
    pub fn grep(
        &self,
        query: &str,
        max_results: usize,
        use_regex: bool,
    ) -> Result<Vec<ContentMatch>, String> {
        let limit = max_results.max(1);
        let guard = self
            .picker
            .read()
            .map_err(|e| format!("reading picker: {e}"))?;
        let Some(picker) = guard.as_ref() else {
            return Ok(Vec::new());
        };

        let parsed = parse_grep_query(query);

        let options = GrepSearchOptions {
            max_file_size: 10 * 1024 * 1024,
            max_matches_per_file: 200,
            smart_case: true,
            file_offset: 0,
            page_limit: limit,
            mode: if use_regex {
                GrepMode::Regex
            } else {
                GrepMode::PlainText
            },
            time_budget_ms: 0,
            before_context: 0,
            after_context: 0,
            classify_definitions: false,
        };

        let result = picker.grep(&parsed, &options);

        // Map GrepMatch → ContentMatch. fff reports 1-based lines and 0-based
        // columns; our ContentMatch shape (and the frontend) expects both 1-based.
        let matches = result
            .matches
            .into_iter()
            .filter_map(|m| {
                let file = *result.files.get(m.file_index)?;
                Some(ContentMatch {
                    path: file.relative_path.to_string(),
                    line: m.line_number as u32,
                    col: (m.col as u32).saturating_add(1),
                    text: m.line_content,
                })
            })
            .collect();

        Ok(matches)
    }

    /// Record a file access so future searches boost this path via frecency.
    ///
    /// Writes to the LMDB frecency database *and* refreshes the picker's
    /// in-memory score for `path` — otherwise the next search would still rank
    /// using the pre-access score (scores are loaded once at scan time and
    /// only refreshed on rescan or git-status events). This mirrors how
    /// `fff.nvim` wires up its `track_access` entry point.
    pub fn track_access(&self, path: &Path) -> Result<(), String> {
        // 1. Persist the access to LMDB.
        {
            let guard = self
                .frecency
                .read()
                .map_err(|e| format!("reading frecency: {e}"))?;
            if let Some(tracker) = guard.as_ref() {
                tracker
                    .track_access(path)
                    .map_err(|e| format!("track_access: {e}"))?;
            }
        }

        // 2. Refresh the picker's in-memory score for this file so the next
        //    `fuzzy_search` reflects the bump. Best-effort: if the picker
        //    hasn't been initialized yet, or the file isn't in the index
        //    (e.g. just created), there's nothing to update.
        let mut picker_guard = self
            .picker
            .write()
            .map_err(|e| format!("writing picker: {e}"))?;
        let Some(picker) = picker_guard.as_mut() else {
            return Ok(());
        };

        let frecency_guard = self
            .frecency
            .read()
            .map_err(|e| format!("reading frecency: {e}"))?;
        if let Some(tracker) = frecency_guard.as_ref() {
            // Non-fatal: a missing path just means the file isn't indexed yet.
            let _ = picker.update_single_file_frecency(path, tracker);
        }
        Ok(())
    }
}

fn highlight_indices(
    matcher: &mut NucleoMatcher,
    pattern: &Pattern,
    haystack: &str,
) -> Vec<u32> {
    if pattern.atoms.is_empty() {
        return Vec::new();
    }
    let mut buf = Vec::new();
    let needle = Utf32Str::new(haystack, &mut buf);
    let mut indices = Vec::new();
    pattern.indices(needle, matcher, &mut indices);
    indices.sort_unstable();
    indices.dedup();
    indices
}
