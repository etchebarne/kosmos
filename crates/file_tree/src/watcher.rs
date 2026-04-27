use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

pub struct FsEvents {
    pub affected_paths: Vec<PathBuf>,
}

pub fn start(root: &Path, tx: Sender<FsEvents>) -> notify::Result<RecommendedWatcher> {
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        let Ok(event) = result else {
            return;
        };
        let _ = tx.send(FsEvents {
            affected_paths: event.paths,
        });
    })?;
    watcher.watch(root, RecursiveMode::Recursive)?;
    Ok(watcher)
}
