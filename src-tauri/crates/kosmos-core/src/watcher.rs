use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use ignore::WalkBuilder;
use notify_debouncer_mini::new_debouncer;

use kosmos_protocol::events::Event;

use crate::{CoreError, EventSink};

type SharedDebouncer =
    Arc<Mutex<Option<notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>>>>;

pub struct WatcherManager {
    events: Arc<dyn EventSink>,
    active: Mutex<Option<ActiveWatch>>,
}

struct ActiveWatch {
    debouncer: SharedDebouncer,
    root: PathBuf,
}

impl WatcherManager {
    pub fn new(events: Arc<dyn EventSink>) -> Self {
        Self {
            events,
            active: Mutex::new(None),
        }
    }

    pub fn watch(&self, path: &str) -> Result<(), CoreError> {
        // Quick check: if already watching this path, bail out.
        {
            let guard = self
                .active
                .lock()
                .map_err(|e| CoreError::Other(e.to_string()))?;
            if let Some(ref a) = *guard {
                if a.root == Path::new(path) {
                    return Ok(());
                }
            }
        } // mutex released — the setup below won't block other callers.

        let events = self.events.clone();
        let watch_path = PathBuf::from(path);
        let git_dir = watch_path.join(".git");
        let root_for_cb = watch_path.clone();

        // Share the debouncer with the event callback (via Weak to avoid a
        // reference cycle) so it can register watches for newly-created
        // directories as they appear.
        let shared: SharedDebouncer = Arc::new(Mutex::new(None));
        let shared_weak: Weak<_> = Arc::downgrade(&shared);

        let debouncer = new_debouncer(
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

                    // Register watches for any newly-created directories. The OS
                    // watcher is NonRecursive per-directory, so a freshly-created
                    // subtree would otherwise produce no further events until the
                    // workspace is reopened.
                    if let Some(strong) = shared_weak.upgrade() {
                        let mut new_dirs: Vec<PathBuf> = Vec::new();
                        for e in &fs_events {
                            if e.path.is_dir() {
                                collect_watchable_dirs(&root_for_cb, &e.path, &mut new_dirs);
                            }
                        }
                        if !new_dirs.is_empty() {
                            new_dirs.sort();
                            new_dirs.dedup();
                            if let Ok(mut guard) = strong.lock() {
                                if let Some(dbr) = guard.as_mut() {
                                    let w = dbr.watcher();
                                    for dir in &new_dirs {
                                        if let Err(e) =
                                            w.watch(dir, notify::RecursiveMode::NonRecursive)
                                        {
                                            tracing::debug!(
                                                "Failed to watch new dir {}: {e}",
                                                dir.display()
                                            );
                                        }
                                    }
                                }
                            }
                        }
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

        // Install the debouncer into the shared slot, then register the initial
        // watch set through the same lock path the callback uses. This ordering
        // matters so early events can already reach a Some(debouncer).
        *shared
            .lock()
            .map_err(|e| CoreError::Other(e.to_string()))? = Some(debouncer);

        {
            let mut guard = shared
                .lock()
                .map_err(|e| CoreError::Other(e.to_string()))?;
            if let Some(dbr) = guard.as_mut() {
                let watcher = dbr.watcher();
                for dir in &dirs {
                    if let Err(e) = watcher.watch(dir, notify::RecursiveMode::NonRecursive) {
                        tracing::debug!("Failed to watch {}: {e}", dir.display());
                    }
                }
            }
        }

        let mut guard = self
            .active
            .lock()
            .map_err(|e| CoreError::Other(e.to_string()))?;
        *guard = Some(ActiveWatch {
            debouncer: shared,
            root: watch_path,
        });
        Ok(())
    }

    pub fn unwatch(&self) -> Result<(), CoreError> {
        let mut guard = self
            .active
            .lock()
            .map_err(|e| CoreError::Other(e.to_string()))?;
        if let Some(active) = guard.take() {
            // Explicitly drop the debouncer (and its background thread) while the
            // Arc is still reachable. The callback's Weak ref then sees the slot
            // go empty on the next fire — preventing a cycle that would otherwise
            // keep the watcher alive.
            if let Ok(mut inner) = active.debouncer.lock() {
                *inner = None;
            }
        }
        Ok(())
    }
}

/// Walk a (newly-created) directory and collect every non-gitignored subdirectory,
/// including `start` itself. Used to register watches for each level of a freshly
/// created subtree so we don't miss events in deeper dirs. Skips anything under
/// `.git` to avoid git-induced feedback loops.
fn collect_watchable_dirs(root: &Path, start: &Path, out: &mut Vec<PathBuf>) {
    let git_dir = root.join(".git");
    for entry in WalkBuilder::new(start)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build()
        .filter_map(|e| e.ok())
    {
        let is_dir = entry.file_type().map_or(false, |ft| ft.is_dir());
        if is_dir && !entry.path().starts_with(&git_dir) {
            out.push(entry.path().to_path_buf());
        }
    }
}
