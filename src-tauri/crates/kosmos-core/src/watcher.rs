use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ignore::WalkBuilder;
use notify_debouncer_mini::new_debouncer;

use kosmos_protocol::events::Event;

use crate::{CoreError, EventSink};

pub struct WatcherManager {
    events: Arc<dyn EventSink>,
    watcher: Mutex<
        Option<(
            notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
            PathBuf,
        )>,
    >,
}

impl WatcherManager {
    pub fn new(events: Arc<dyn EventSink>) -> Self {
        Self {
            events,
            watcher: Mutex::new(None),
        }
    }

    pub fn watch(&self, path: &str) -> Result<(), CoreError> {
        // Quick check: if already watching this path, bail out.
        {
            let guard = self
                .watcher
                .lock()
                .map_err(|e| CoreError::Other(e.to_string()))?;
            if let Some((_, ref current)) = *guard {
                if current == Path::new(path) {
                    return Ok(());
                }
            }
        } // mutex released — the setup below won't block other callers.

        let events = self.events.clone();
        let watch_path = PathBuf::from(path);
        let git_dir = watch_path.join(".git");

        let mut debouncer = new_debouncer(
            Duration::from_millis(500),
            move |result: Result<
                Vec<notify_debouncer_mini::DebouncedEvent>,
                notify::Error,
            >| {
                if let Ok(fs_events) = result {
                    // Filter out .git directory changes — running git commands modifies
                    // files there, which would trigger another refresh in a feedback loop.
                    let fs_events: Vec<_> = fs_events
                        .into_iter()
                        .filter(|e| !e.path.starts_with(&git_dir))
                        .collect();

                    if fs_events.is_empty() {
                        return;
                    }

                    // Since we only watch non-gitignored directories (via ignore::Walk),
                    // all events here are for trackable files. Emit GitChanged directly.
                    events.emit(Event::GitChanged);

                    let mut dirs: Vec<String> = fs_events
                        .iter()
                        .filter_map(|e| e.path.parent().map(|p| p.to_string_lossy().to_string()))
                        .collect();
                    dirs.sort();
                    dirs.dedup();

                    events.emit(Event::FileTreeChanged { dirs });

                    let mut files: Vec<String> = fs_events
                        .iter()
                        .map(|e| e.path.to_string_lossy().to_string())
                        .collect();
                    files.sort();
                    files.dedup();

                    if !files.is_empty() {
                        events.emit(Event::FileContentChanged { files });
                    }
                }
            },
        )
        .map_err(|e| CoreError::Other(e.to_string()))?;

        // Walk only non-gitignored directories using the `ignore` crate, which
        // respects nested .gitignore files, global gitignore, and .git/info/exclude.
        // This typically reduces watched directories by 10-20x on large repos
        // (e.g. 54K → 2.4K for a monorepo with node_modules).
        //
        // The ignore crate with hidden(false) walks into .git — we must exclude
        // .git and ALL its subdirectories (starts_with, not equality) to avoid
        // watching internal git files that change on every git command.
        let git_dir_prefix = watch_path.join(".git");
        let dirs: Vec<PathBuf> = WalkBuilder::new(path)
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .build()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map_or(false, |ft| ft.is_dir()))
            .filter(|e| !e.path().starts_with(&git_dir_prefix))
            .map(|e| e.into_path())
            .collect();

        let watcher = debouncer.watcher();
        for dir in &dirs {
            if let Err(e) = watcher.watch(dir, notify::RecursiveMode::NonRecursive) {
                tracing::debug!("Failed to watch {}: {e}", dir.display());
            }
        }

        // Re-acquire the mutex only to swap in the new watcher.
        let mut guard = self
            .watcher
            .lock()
            .map_err(|e| CoreError::Other(e.to_string()))?;
        *guard = Some((debouncer, watch_path));
        Ok(())
    }

    pub fn unwatch(&self) -> Result<(), CoreError> {
        let mut guard = self
            .watcher
            .lock()
            .map_err(|e| CoreError::Other(e.to_string()))?;
        *guard = None;
        Ok(())
    }
}
