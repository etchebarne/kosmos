use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock, mpsc};
use std::thread;
use std::time::Duration;

use notify::{ErrorKind, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::tabs::git::repository_watch_paths;
use crate::tree::{WorkspaceId, WorkspaceList};

const CHANGE_DEBOUNCE: Duration = Duration::from_millis(100);
const MAX_CHANGE_LATENCY: Duration = Duration::from_secs(1);

type WatchTargets = HashMap<PathBuf, HashSet<WorkspaceId>>;

#[derive(Default)]
struct WorkspaceWatchTargets {
    git_paths: HashSet<PathBuf>,
    worktree_paths: HashSet<PathBuf>,
    workspace_ids: WatchTargets,
}

pub struct WorkspaceChangeWatcher {
    git_watcher: RecommendedWatcher,
    targets: Arc<RwLock<WatchTargets>>,
    watched_git_paths: HashMap<PathBuf, RecursiveMode>,
    watched_worktree_paths: HashMap<PathBuf, RecursiveMode>,
    git_paths: HashSet<PathBuf>,
    worktree_paths: HashSet<PathBuf>,
    worktree_watcher: RecommendedWatcher,
    workspace_directories: HashMap<WorkspaceId, PathBuf>,
    registrations_dirty: Arc<AtomicBool>,
}

impl WorkspaceChangeWatcher {
    pub fn new() -> notify::Result<(Self, mpsc::Receiver<Vec<WorkspaceId>>)> {
        let targets = Arc::new(RwLock::new(WatchTargets::new()));
        let pending_changes = Arc::new(std::sync::Mutex::new(HashSet::new()));
        let (raw_changes, raw_change_receiver) = mpsc::sync_channel(1);
        let registrations_dirty = Arc::new(AtomicBool::new(false));
        let git_watcher = event_watcher(
            Arc::clone(&targets),
            Arc::clone(&pending_changes),
            raw_changes.clone(),
            Arc::clone(&registrations_dirty),
        )?;
        let worktree_watcher = event_watcher(
            Arc::clone(&targets),
            Arc::clone(&pending_changes),
            raw_changes,
            Arc::clone(&registrations_dirty),
        )?;
        let (changes, change_receiver) = mpsc::channel();

        spawn_debouncer(raw_change_receiver, pending_changes, changes);

        Ok((
            Self {
                git_watcher,
                targets,
                watched_git_paths: HashMap::new(),
                watched_worktree_paths: HashMap::new(),
                git_paths: HashSet::new(),
                worktree_paths: HashSet::new(),
                worktree_watcher,
                workspace_directories: HashMap::new(),
                registrations_dirty,
            },
            change_receiver,
        ))
    }

    pub fn reconcile(&mut self, workspaces: &WorkspaceList) -> notify::Result<()> {
        let registrations_dirty = self.registrations_dirty.swap(false, Ordering::AcqRel);
        if registrations_dirty {
            reset_watcher(&mut self.git_watcher, &mut self.watched_git_paths);
            reset_watcher(&mut self.worktree_watcher, &mut self.watched_worktree_paths);
        }

        let workspace_directories = workspace_directories(workspaces);
        if registrations_dirty || workspace_directories != self.workspace_directories {
            let next_targets = watch_targets(workspaces);

            if let Ok(mut targets) = self.targets.write() {
                *targets = next_targets.workspace_ids;
            }
            self.git_paths = next_targets.git_paths;
            self.worktree_paths = next_targets.worktree_paths;
            self.workspace_directories = workspace_directories;
        }

        let git_error = reconcile_registrations(
            &mut self.git_watcher,
            &mut self.watched_git_paths,
            &registration_paths(&self.git_paths),
        )
        .err();
        let worktree_error = reconcile_registrations(
            &mut self.worktree_watcher,
            &mut self.watched_worktree_paths,
            &registration_paths(&self.worktree_paths),
        )
        .err();

        git_error.or(worktree_error).map_or(Ok(()), Err)
    }
}

fn workspace_directories(workspaces: &WorkspaceList) -> HashMap<WorkspaceId, PathBuf> {
    workspaces
        .workspaces()
        .iter()
        .map(|workspace| (workspace.id(), workspace.directory().to_path_buf()))
        .collect()
}

fn watch_targets(workspaces: &WorkspaceList) -> WorkspaceWatchTargets {
    let mut targets = WorkspaceWatchTargets::default();

    for workspace in workspaces.workspaces() {
        let directory = normalized_existing_path(workspace.directory());
        let Ok(paths) = repository_watch_paths(&directory) else {
            targets.worktree_paths.insert(directory.clone());
            targets
                .workspace_ids
                .entry(directory)
                .or_default()
                .insert(workspace.id());
            continue;
        };
        let worktree = normalized_existing_path(&paths.worktree);

        targets.worktree_paths.insert(worktree.clone());
        targets
            .workspace_ids
            .entry(worktree)
            .or_default()
            .insert(workspace.id());

        for path in paths.metadata {
            let path = normalized_existing_path(&path);
            targets.git_paths.insert(path.clone());
            targets
                .workspace_ids
                .entry(path)
                .or_default()
                .insert(workspace.id());
        }
    }

    targets
}

fn normalized_existing_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn registration_paths(paths: &HashSet<PathBuf>) -> HashMap<PathBuf, RecursiveMode> {
    let recursive_paths = paths
        .iter()
        .filter(|path| {
            !paths
                .iter()
                .any(|other| *path != other && path.starts_with(other))
        })
        .cloned()
        .collect::<HashSet<_>>();
    let mut registrations = recursive_paths
        .iter()
        .cloned()
        .map(|path| (path, RecursiveMode::Recursive))
        .collect::<HashMap<_, _>>();

    for path in &recursive_paths {
        if let Some(parent) = path.parent()
            && !recursive_paths.contains(parent)
        {
            registrations
                .entry(parent.to_path_buf())
                .or_insert(RecursiveMode::NonRecursive);
        }
    }

    registrations
}

fn registration_conflicts(
    path: &Path,
    mode: RecursiveMode,
    registrations: &HashMap<PathBuf, RecursiveMode>,
) -> bool {
    registrations.iter().any(|(other, other_mode)| {
        path == other
            || (mode == RecursiveMode::Recursive
                && *other_mode == RecursiveMode::Recursive
                && (path.starts_with(other) || other.starts_with(path)))
    })
}

fn reconcile_registrations(
    watcher: &mut RecommendedWatcher,
    watched_paths: &mut HashMap<PathBuf, RecursiveMode>,
    desired_paths: &HashMap<PathBuf, RecursiveMode>,
) -> notify::Result<()> {
    let mut first_error = None;
    let mut removed_paths = watched_paths
        .iter()
        .filter(|(path, mode)| desired_paths.get(*path) != Some(*mode))
        .map(|(path, _)| path.clone())
        .collect::<Vec<_>>();
    removed_paths.sort_by_key(|path| std::cmp::Reverse(path.components().count()));

    for path in removed_paths {
        match watcher.unwatch(&path) {
            Ok(()) => {
                watched_paths.remove(&path);
            }
            Err(error)
                if matches!(
                    &error.kind,
                    ErrorKind::PathNotFound | ErrorKind::WatchNotFound
                ) =>
            {
                watched_paths.remove(&path);
            }
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }

    let mut added_paths = desired_paths
        .iter()
        .filter(|(path, mode)| watched_paths.get(*path) != Some(*mode))
        .filter(|(path, mode)| !registration_conflicts(path, **mode, watched_paths))
        .map(|(path, mode)| (path.clone(), *mode))
        .collect::<Vec<_>>();
    added_paths.sort_by_key(|(path, _)| path.components().count());

    for (path, mode) in added_paths {
        match watcher.watch(&path, mode) {
            Ok(()) => {
                watched_paths.insert(path, mode);
            }
            Err(error) => {
                let _ = watcher.unwatch(&path);
                first_error.get_or_insert(error);
            }
        }
    }

    first_error.map_or(Ok(()), Err)
}

fn reset_watcher(
    watcher: &mut RecommendedWatcher,
    watched_paths: &mut HashMap<PathBuf, RecursiveMode>,
) {
    for path in watched_paths.keys() {
        let _ = watcher.unwatch(path);
    }
    watched_paths.clear();
}

fn event_watcher(
    targets: Arc<RwLock<WatchTargets>>,
    pending_changes: Arc<std::sync::Mutex<HashSet<WorkspaceId>>>,
    raw_changes: mpsc::SyncSender<()>,
    registrations_dirty: Arc<AtomicBool>,
) -> notify::Result<RecommendedWatcher> {
    notify::recommended_watcher(move |event: notify::Result<Event>| {
        let event = match event {
            Ok(event) => event,
            Err(_) => {
                registrations_dirty.store(true, Ordering::Release);
                let workspace_ids = targets
                    .read()
                    .map(|targets| affected_workspace_ids(&targets, &[]))
                    .unwrap_or_default();
                queue_workspace_changes(&pending_changes, &raw_changes, workspace_ids);
                return;
            }
        };
        if !is_change_event(&event.kind) {
            return;
        }

        let (workspace_ids, watched_root_changed) = targets
            .read()
            .map(|targets| {
                let watched_root_changed = can_change_watched_root(&event.kind)
                    && event.paths.iter().any(|path| targets.contains_key(path));

                (
                    affected_workspace_ids(&targets, &event.paths),
                    watched_root_changed,
                )
            })
            .unwrap_or_default();
        if watched_root_changed {
            registrations_dirty.store(true, Ordering::Release);
        }
        queue_workspace_changes(&pending_changes, &raw_changes, workspace_ids);
    })
}

fn affected_workspace_ids(targets: &WatchTargets, paths: &[PathBuf]) -> Vec<WorkspaceId> {
    let mut workspace_ids = HashSet::new();

    if paths.is_empty() {
        workspace_ids.extend(targets.values().flatten().copied());
    } else {
        for path in paths {
            for (target, target_workspace_ids) in targets {
                if path.starts_with(target) || target.starts_with(path) {
                    workspace_ids.extend(target_workspace_ids.iter().copied());
                }
            }
        }
    }

    let mut workspace_ids = workspace_ids.into_iter().collect::<Vec<_>>();
    workspace_ids.sort_by_key(|workspace_id| workspace_id.value());
    workspace_ids
}

fn is_change_event(kind: &EventKind) -> bool {
    !matches!(kind, EventKind::Access(_))
}

fn can_change_watched_root(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Modify(notify::event::ModifyKind::Name(_))
    )
}

fn queue_workspace_changes(
    pending_changes: &std::sync::Mutex<HashSet<WorkspaceId>>,
    raw_changes: &mpsc::SyncSender<()>,
    workspace_ids: Vec<WorkspaceId>,
) {
    if let Ok(mut pending_changes) = pending_changes.lock() {
        pending_changes.extend(workspace_ids);
        if !pending_changes.is_empty() {
            let _ = raw_changes.try_send(());
        }
    }
}

fn spawn_debouncer(
    raw_changes: mpsc::Receiver<()>,
    pending_changes: Arc<std::sync::Mutex<HashSet<WorkspaceId>>>,
    changes: mpsc::Sender<Vec<WorkspaceId>>,
) {
    thread::spawn(move || {
        while raw_changes.recv().is_ok() {
            let started_at = std::time::Instant::now();

            loop {
                let elapsed = started_at.elapsed();
                if elapsed >= MAX_CHANGE_LATENCY {
                    break;
                }
                let timeout = CHANGE_DEBOUNCE.min(MAX_CHANGE_LATENCY - elapsed);

                match raw_changes.recv_timeout(timeout) {
                    Ok(()) => {}
                    Err(mpsc::RecvTimeoutError::Timeout) => break,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }

            let mut workspace_ids = pending_changes
                .lock()
                .map(|mut pending_changes| pending_changes.drain().collect::<Vec<_>>())
                .unwrap_or_default();
            if workspace_ids.is_empty() {
                continue;
            }
            workspace_ids.sort_by_key(|workspace_id| workspace_id.value());
            if changes.send(workspace_ids).is_err() {
                break;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::State;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn maps_repository_changes_to_every_workspace_using_it() {
        let repository = PathBuf::from("/repo");
        let targets = HashMap::from([(
            repository.clone(),
            HashSet::from([WorkspaceId::new(1), WorkspaceId::new(2)]),
        )]);

        assert_eq!(
            affected_workspace_ids(&targets, &[repository.join("src/main.rs")]),
            vec![WorkspaceId::new(1), WorkspaceId::new(2)]
        );
    }

    #[test]
    fn ignores_changes_outside_watched_repositories() {
        let targets =
            HashMap::from([(PathBuf::from("/repo"), HashSet::from([WorkspaceId::new(1)]))]);

        assert!(affected_workspace_ids(&targets, &[PathBuf::from("/other/file")]).is_empty());
    }

    #[test]
    fn registers_only_outermost_recursive_watch_paths() {
        let paths = HashSet::from([
            PathBuf::from("/repo"),
            PathBuf::from("/repo/nested"),
            PathBuf::from("/worktrees/git-dir"),
        ]);

        assert_eq!(
            registration_paths(&paths),
            HashMap::from([
                (PathBuf::from("/"), RecursiveMode::NonRecursive),
                (PathBuf::from("/repo"), RecursiveMode::Recursive),
                (PathBuf::from("/worktrees"), RecursiveMode::NonRecursive),
                (
                    PathBuf::from("/worktrees/git-dir"),
                    RecursiveMode::Recursive
                )
            ])
        );
    }

    #[test]
    fn emits_workspace_change_after_a_file_write() {
        let directory = test_directory("file-write");
        let mut state = State::new();
        let workspace_id = state.open_workspace(&directory);
        let (mut watcher, changes) = WorkspaceChangeWatcher::new().expect("watcher should start");
        watcher
            .reconcile(state.workspaces())
            .expect("workspace should be watched");

        fs::write(directory.join("changed.txt"), "changed").expect("file should be written");

        assert_eq!(
            changes
                .recv_timeout(Duration::from_secs(3))
                .expect("workspace change should arrive"),
            vec![workspace_id]
        );

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn watches_the_repository_root_for_nested_workspaces() {
        let repository = test_directory("repository-root");
        let workspace = repository.join("nested/workspace");
        fs::create_dir_all(&workspace).expect("nested workspace should be created");
        assert!(
            Command::new("git")
                .arg("init")
                .arg(&repository)
                .output()
                .expect("git should run")
                .status
                .success()
        );
        let mut state = State::new();
        let workspace_id = state.open_workspace(&workspace);
        let (mut watcher, changes) = WorkspaceChangeWatcher::new().expect("watcher should start");
        watcher
            .reconcile(state.workspaces())
            .expect("workspace should be watched");

        fs::write(repository.join("sibling.txt"), "changed").expect("file should be written");

        assert_eq!(
            changes
                .recv_timeout(Duration::from_secs(3))
                .expect("repository change should arrive"),
            vec![workspace_id]
        );

        let _ = fs::remove_dir_all(repository);
    }

    #[test]
    fn emits_workspace_changes_for_external_index_updates() {
        let directory = test_directory("git-index");
        run_git(&directory, &["init"]);
        fs::write(directory.join("tracked.txt"), "initial").expect("file should be written");
        run_git(&directory, &["add", "tracked.txt"]);
        run_git(
            &directory,
            &[
                "-c",
                "user.name=Kosmos Test",
                "-c",
                "user.email=kosmos@example.com",
                "commit",
                "-m",
                "initial",
            ],
        );
        fs::write(directory.join("tracked.txt"), "changed").expect("file should be updated");

        let mut state = State::new();
        let workspace_id = state.open_workspace(&directory);
        let (mut watcher, changes) = WorkspaceChangeWatcher::new().expect("watcher should start");
        watcher
            .reconcile(state.workspaces())
            .expect("workspace should be watched");
        reset_watcher(
            &mut watcher.worktree_watcher,
            &mut watcher.watched_worktree_paths,
        );

        run_git(&directory, &["add", "tracked.txt"]);
        assert_eq!(
            changes
                .recv_timeout(Duration::from_secs(3))
                .expect("staging change should arrive"),
            vec![workspace_id]
        );

        run_git(&directory, &["reset", "HEAD", "--", "tracked.txt"]);
        assert_eq!(
            changes
                .recv_timeout(Duration::from_secs(3))
                .expect("unstaging change should arrive"),
            vec![workspace_id]
        );

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn restores_watching_after_a_workspace_is_closed_and_reopened() {
        let directory = test_directory("reopen");
        let mut state = State::new();
        let first_workspace_id = state.open_workspace(&directory);
        let (mut watcher, changes) = WorkspaceChangeWatcher::new().expect("watcher should start");
        watcher
            .reconcile(state.workspaces())
            .expect("workspace should be watched");

        state.close_workspace(Some(first_workspace_id));
        watcher
            .reconcile(state.workspaces())
            .expect("closed workspace should be unwatched");
        let reopened_workspace_id = state.open_workspace(&directory);
        watcher
            .reconcile(state.workspaces())
            .expect("reopened workspace should be watched");
        fs::write(directory.join("changed.txt"), "changed").expect("file should be written");

        assert_eq!(
            changes
                .recv_timeout(Duration::from_secs(3))
                .expect("workspace change should arrive"),
            vec![reopened_workspace_id]
        );

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn restores_watching_after_a_workspace_directory_is_recreated() {
        let directory = test_directory("recreate");
        let mut state = State::new();
        let workspace_id = state.open_workspace(&directory);
        let (mut watcher, changes) = WorkspaceChangeWatcher::new().expect("watcher should start");
        watcher
            .reconcile(state.workspaces())
            .expect("workspace should be watched");

        fs::remove_dir_all(&directory).expect("workspace should be removed");
        assert_eq!(
            changes
                .recv_timeout(Duration::from_secs(3))
                .expect("workspace removal should arrive"),
            vec![workspace_id]
        );
        assert!(watcher.reconcile(state.workspaces()).is_err());

        fs::create_dir_all(&directory).expect("workspace should be recreated");
        assert_eq!(
            changes
                .recv_timeout(Duration::from_secs(3))
                .expect("workspace recreation should arrive"),
            vec![workspace_id]
        );
        watcher
            .reconcile(state.workspaces())
            .expect("recreated workspace should be watched");
        fs::write(directory.join("changed.txt"), "changed").expect("file should be written");
        assert_eq!(
            changes
                .recv_timeout(Duration::from_secs(3))
                .expect("workspace change should arrive"),
            vec![workspace_id]
        );

        let _ = fs::remove_dir_all(directory);
    }

    fn test_directory(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "kosmos-workspace-changes-{}-{name}-{nanos}",
            std::process::id()
        ));

        fs::create_dir_all(&directory).expect("test directory should be created");
        directory
    }

    fn run_git(directory: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(directory)
            .args(args)
            .output()
            .expect("git should run");

        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
