use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};

const GIT_DIRECTORY_NAME: &str = ".git";
const GITIGNORE_FILE_NAME: &str = ".gitignore";
/// Git metadata that changes constantly without affecting anything Kosmos shows: object
/// storage, reflogs and transient lock files. Index and ref updates still pass through.
const NOISY_GIT_DIRECTORY_NAMES: &[&str] = &["objects", "logs", "lfs"];
const GIT_LOCK_EXTENSION: &str = "lock";

/// Drops filesystem events that cannot change what a workspace displays, so dependency
/// installs, build output and git object writes do not trigger workspace refreshes.
#[derive(Default)]
pub(crate) struct WorkspaceChangeFilter {
    worktrees: Vec<WorktreeIgnore>,
    git_directories: Vec<PathBuf>,
}

struct WorktreeIgnore {
    root: PathBuf,
    gitignore: Gitignore,
}

impl WorkspaceChangeFilter {
    pub(crate) fn new(
        worktrees: impl IntoIterator<Item = PathBuf>,
        git_directories: impl IntoIterator<Item = PathBuf>,
    ) -> Self {
        Self {
            worktrees: worktrees.into_iter().map(WorktreeIgnore::load).collect(),
            git_directories: git_directories.into_iter().collect(),
        }
    }

    pub(crate) fn reload_ignore_rules(&mut self) {
        for worktree in &mut self.worktrees {
            *worktree = WorktreeIgnore::load(std::mem::take(&mut worktree.root));
        }
    }

    pub(crate) fn is_relevant(&self, paths: &[PathBuf]) -> bool {
        paths.is_empty() || paths.iter().any(|path| !self.is_ignored(path))
    }

    pub(crate) fn touches_ignore_rules(paths: &[PathBuf]) -> bool {
        paths
            .iter()
            .any(|path| path.file_name() == Some(OsStr::new(GITIGNORE_FILE_NAME)))
    }

    fn is_ignored(&self, path: &Path) -> bool {
        self.is_noisy_git_path(path) || self.worktrees.iter().any(|worktree| worktree.ignores(path))
    }

    fn is_noisy_git_path(&self, path: &Path) -> bool {
        self.git_directories
            .iter()
            .filter_map(|directory| path.strip_prefix(directory).ok())
            .chain(worktree_git_relative_path(path))
            .any(is_noisy_git_relative_path)
    }
}

impl WorktreeIgnore {
    fn load(root: PathBuf) -> Self {
        let mut builder = GitignoreBuilder::new(&root);
        let _ = builder.add(root.join(GITIGNORE_FILE_NAME));
        let gitignore = builder.build().unwrap_or_else(|_| Gitignore::empty());

        Self { root, gitignore }
    }

    fn ignores(&self, path: &Path) -> bool {
        let Ok(relative_path) = path.strip_prefix(&self.root) else {
            return false;
        };

        !relative_path.as_os_str().is_empty()
            && self
                .gitignore
                .matched_path_or_any_parents(relative_path, false)
                .is_ignore()
    }
}

fn worktree_git_relative_path(path: &Path) -> Option<&Path> {
    let mut components = path.components();

    while let Some(component) = components.next() {
        if component == Component::Normal(OsStr::new(GIT_DIRECTORY_NAME)) {
            return Some(components.as_path());
        }
    }

    None
}

fn is_noisy_git_relative_path(path: &Path) -> bool {
    let in_noisy_directory = path
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
        .is_some_and(|name| NOISY_GIT_DIRECTORY_NAMES.contains(&name));

    in_noisy_directory || path.extension() == Some(OsStr::new(GIT_LOCK_EXTENSION))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn ignores_changes_matched_by_the_root_gitignore() {
        let root = test_root("gitignore");
        fs::write(root.join(".gitignore"), "node_modules/\ntarget\n*.log\n")
            .expect("gitignore should be written");
        let filter = WorkspaceChangeFilter::new([root.clone()], []);

        assert!(!filter.is_relevant(&[root.join("node_modules/pkg/index.js")]));
        assert!(!filter.is_relevant(&[root.join("target/debug/build/out.o")]));
        assert!(!filter.is_relevant(&[root.join("packages/app/debug.log")]));
        assert!(filter.is_relevant(&[root.join("src/main.rs")]));
        assert!(filter.is_relevant(&[root.join("target/debug/out.o"), root.join("src/main.rs")]));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ignores_git_object_and_lock_churn_but_keeps_index_and_ref_updates() {
        let root = test_root("git-metadata");
        let git_directory = PathBuf::from("/worktrees/repo.git");
        let filter = WorkspaceChangeFilter::new([root.clone()], [git_directory.clone()]);

        assert!(!filter.is_relevant(&[root.join(".git/objects/ab/cdef")]));
        assert!(!filter.is_relevant(&[root.join(".git/index.lock")]));
        assert!(!filter.is_relevant(&[git_directory.join("logs/HEAD")]));
        assert!(filter.is_relevant(&[root.join(".git/index")]));
        assert!(filter.is_relevant(&[git_directory.join("refs/heads/main")]));
        assert!(filter.is_relevant(&[root.join(".git/index.lock"), root.join(".git/index")]));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn events_without_paths_are_relevant() {
        assert!(WorkspaceChangeFilter::default().is_relevant(&[]));
    }

    #[test]
    fn detects_gitignore_changes() {
        assert!(WorkspaceChangeFilter::touches_ignore_rules(&[
            PathBuf::from("/repo/packages/app/.gitignore")
        ]));
        assert!(!WorkspaceChangeFilter::touches_ignore_rules(&[
            PathBuf::from("/repo/src/main.rs")
        ]));
    }

    fn test_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kosmos-workspace-change-filter-{}-{name}-{nanos}",
            std::process::id()
        ));

        fs::create_dir_all(&root).expect("test root should be created");
        root
    }
}
