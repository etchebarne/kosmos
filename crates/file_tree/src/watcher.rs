use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

pub struct FsEvents {
    pub affected_paths: Vec<PathBuf>,
}

/// Directory names we don't recursively watch. They tend to be huge, change
/// constantly in ways the user doesn't care about in the file tree, and on
/// Linux each subdirectory costs an inotify watch.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".nuxt",
    ".turbo",
    ".cache",
    ".venv",
    "venv",
    "__pycache__",
    "vendor",
];

/// Install a recursive watch on `root`, skipping known-heavy directories.
///
/// On Linux the `notify` crate emulates recursive watches by walking the tree
/// and installing one inotify watch per directory. For a workspace with
/// `node_modules`/`target`/etc. that walk can take many seconds and may exhaust
/// `fs.inotify.max_user_watches`. Callers should run this off the main thread.
pub fn start(root: &Path, tx: Sender<FsEvents>) -> notify::Result<RecommendedWatcher> {
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        let Ok(event) = result else {
            return;
        };
        let _ = tx.send(FsEvents {
            affected_paths: event.paths,
        });
    })?;
    watch_tree(&mut watcher, root)?;
    Ok(watcher)
}

fn watch_tree(watcher: &mut RecommendedWatcher, dir: &Path) -> notify::Result<()> {
    watcher.watch(dir, RecursiveMode::NonRecursive)?;
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        // file_type from read_dir doesn't follow symlinks, so symlinked dirs
        // report is_dir() == false — we naturally skip them and avoid cycles.
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name();
        if SKIP_DIRS
            .iter()
            .any(|s| *s == name.to_string_lossy().as_ref())
        {
            continue;
        }
        // Best-effort: a child failing to watch (e.g. raced delete, permission
        // denied) shouldn't abort the rest of the install.
        let _ = watch_tree(watcher, &entry.path());
    }
    Ok(())
}
