use std::cmp::Ordering;
use std::collections::HashSet;
use std::error::Error as StdError;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::tree::{TabId, WorkspaceId};

pub type Result<T> = std::result::Result<T, FileTreeError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileTree {
    root: PathBuf,
    paths: Vec<String>,
    expanded_paths: Vec<String>,
}

impl FileTree {
    pub fn scan(root: impl Into<PathBuf>) -> Result<Self> {
        Self::scan_with_expanded_paths(root, &[])
    }

    pub fn scan_with_expanded_paths(
        root: impl Into<PathBuf>,
        expanded_paths: &[String],
    ) -> Result<Self> {
        let root = root.into();
        ensure_directory(&root)?;

        let mut paths = Vec::new();
        let mut directory_paths = HashSet::new();
        collect_paths(&root, "", &mut paths, &mut directory_paths)?;
        let expanded_paths = normalize_expanded_paths(expanded_paths)
            .into_iter()
            .filter(|path| directory_paths.contains(path))
            .collect();

        Ok(Self {
            root,
            paths,
            expanded_paths,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn paths(&self) -> &[String] {
        &self.paths
    }

    pub fn expanded_paths(&self) -> &[String] {
        &self.expanded_paths
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileTreeViewState {
    workspace_id: WorkspaceId,
    tab_id: TabId,
    expanded_paths: Vec<String>,
}

impl FileTreeViewState {
    pub fn new(workspace_id: WorkspaceId, tab_id: TabId, expanded_paths: Vec<String>) -> Self {
        Self {
            workspace_id,
            tab_id,
            expanded_paths: normalize_expanded_paths(&expanded_paths),
        }
    }

    pub fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }

    pub fn tab_id(&self) -> TabId {
        self.tab_id
    }

    pub fn expanded_paths(&self) -> &[String] {
        &self.expanded_paths
    }
}

#[derive(Debug)]
pub enum FileTreeError {
    Io { path: PathBuf, error: io::Error },
    RootNotDirectory(PathBuf),
    TabNotFound,
    WorkspaceNotFound,
}

impl fmt::Display for FileTreeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, error } => {
                write!(formatter, "could not read {}: {error}", path.display())
            }
            Self::RootNotDirectory(path) => {
                write!(formatter, "{} is not a directory", path.display())
            }
            Self::TabNotFound => formatter.write_str("file tree tab does not exist"),
            Self::WorkspaceNotFound => formatter.write_str("workspace does not exist"),
        }
    }
}

impl StdError for FileTreeError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io { error, .. } => Some(error),
            Self::RootNotDirectory(_) | Self::TabNotFound | Self::WorkspaceNotFound => None,
        }
    }
}

fn ensure_directory(root: &Path) -> Result<()> {
    let metadata = fs::metadata(root).map_err(|error| io_error(root, error))?;

    if metadata.is_dir() {
        Ok(())
    } else {
        Err(FileTreeError::RootNotDirectory(root.to_path_buf()))
    }
}

fn collect_paths(
    directory: &Path,
    relative_directory: &str,
    paths: &mut Vec<String>,
    directory_paths: &mut HashSet<String>,
) -> Result<()> {
    for entry in sorted_entries(directory)? {
        let relative_path = relative_path(relative_directory, &entry.name);

        if entry.is_directory {
            paths.push(format!("{relative_path}/"));
            directory_paths.insert(format!("{relative_path}/"));
            collect_paths(&entry.path, &relative_path, paths, directory_paths)?;
        } else {
            paths.push(relative_path);
        }
    }

    Ok(())
}

fn sorted_entries(directory: &Path) -> Result<Vec<FileTreeEntry>> {
    let entries = fs::read_dir(directory).map_err(|error| io_error(directory, error))?;
    let mut file_tree_entries = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|error| io_error(directory, error))?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| io_error(&path, error))?;

        file_tree_entries.push(FileTreeEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            path,
            is_directory: file_type.is_dir(),
        });
    }

    file_tree_entries.sort_by(compare_entries);

    Ok(file_tree_entries)
}

fn compare_entries(left: &FileTreeEntry, right: &FileTreeEntry) -> Ordering {
    match (left.is_directory, right.is_directory) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => left
            .name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.name.cmp(&right.name)),
    }
}

fn relative_path(relative_directory: &str, name: &str) -> String {
    if relative_directory.is_empty() {
        name.to_owned()
    } else {
        format!("{relative_directory}/{name}")
    }
}

fn io_error(path: impl Into<PathBuf>, error: io::Error) -> FileTreeError {
    FileTreeError::Io {
        path: path.into(),
        error,
    }
}

fn normalize_expanded_paths(paths: &[String]) -> Vec<String> {
    let mut normalized_paths = paths
        .iter()
        .filter_map(|path| normalize_expanded_path(path))
        .collect::<Vec<_>>();

    normalized_paths.sort();
    normalized_paths.dedup();

    normalized_paths
}

fn normalize_expanded_path(path: &str) -> Option<String> {
    if path.is_empty() || path.starts_with('/') || path.contains('\\') || path.contains('\0') {
        return None;
    }

    let path = path.strip_suffix('/').unwrap_or(path);
    if path.is_empty() {
        return None;
    }

    if path
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return None;
    }

    Some(format!("{path}/"))
}

#[derive(Debug)]
struct FileTreeEntry {
    name: String,
    path: PathBuf,
    is_directory: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn scans_directories_and_files_as_relative_tree_paths() {
        let root = test_root("scan");
        fs::create_dir_all(root.join("src/components")).expect("test directory should be created");
        fs::create_dir(root.join("empty")).expect("empty test directory should be created");
        fs::write(root.join("README.md"), b"readme").expect("readme should be written");
        fs::write(root.join("src/main.rs"), b"fn main() {}").expect("main should be written");
        fs::write(root.join("src/components/Button.tsx"), b"export {}")
            .expect("component should be written");

        let tree = FileTree::scan(&root).expect("file tree should scan");

        assert_eq!(tree.root(), root.as_path());
        assert_eq!(
            tree.paths(),
            &[
                "empty/",
                "src/",
                "src/components/",
                "src/components/Button.tsx",
                "src/main.rs",
                "README.md",
            ]
        );
        assert!(tree.expanded_paths().is_empty());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn scan_returns_normalized_existing_expanded_directories() {
        let root = test_root("expanded");
        fs::create_dir_all(root.join("src/components")).expect("test directory should be created");
        fs::create_dir(root.join("target")).expect("target test directory should be created");
        fs::write(root.join("src/main.rs"), b"fn main() {}").expect("main should be written");

        let expanded_paths = vec![
            "src".to_owned(),
            "src/components/".to_owned(),
            "missing/".to_owned(),
            "../outside/".to_owned(),
        ];
        let tree = FileTree::scan_with_expanded_paths(&root, &expanded_paths)
            .expect("file tree should scan");

        assert_eq!(tree.expanded_paths(), &["src/", "src/components/"]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn view_state_normalizes_expanded_paths() {
        let state = FileTreeViewState::new(
            WorkspaceId::new(1),
            TabId::new(2),
            vec![
                "src".to_owned(),
                "src/".to_owned(),
                "/absolute/".to_owned(),
                "src/../target/".to_owned(),
                "target/".to_owned(),
            ],
        );

        assert_eq!(state.workspace_id(), WorkspaceId::new(1));
        assert_eq!(state.tab_id(), TabId::new(2));
        assert_eq!(state.expanded_paths(), &["src/", "target/"]);
    }

    #[test]
    fn rejects_non_directory_roots() {
        let root = test_root("file-root");
        fs::create_dir_all(&root).expect("test directory should be created");
        let file = root.join("file.txt");
        fs::write(&file, b"not a directory").expect("test file should be written");

        let error = FileTree::scan(&file).expect_err("file roots should be rejected");

        assert!(matches!(error, FileTreeError::RootNotDirectory(path) if path == file));

        let _ = fs::remove_dir_all(root);
    }

    fn test_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kosmos-core-file-tree-{}-{name}-{nanos}",
            std::process::id()
        ));

        fs::create_dir_all(&root).expect("test root should be created");

        root
    }
}
