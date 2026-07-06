use std::cmp::Ordering;
use std::collections::HashSet;
use std::error::Error as StdError;
use std::fmt;
use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};

use crate::tree::{TabId, WorkspaceId};

pub type Result<T> = std::result::Result<T, FileTreeError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileTreeEntryKind {
    Directory,
    File,
}

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

    pub fn create_entry(
        root: impl AsRef<Path>,
        parent_path: Option<&str>,
        name: &str,
        kind: FileTreeEntryKind,
    ) -> Result<PathBuf> {
        let root = root.as_ref();
        ensure_directory(root)?;
        let parent = resolve_existing_directory(root, parent_path)?;
        let name = normalize_entry_name(name)?;
        let destination = parent.join(name);

        ensure_missing(&destination)?;

        match kind {
            FileTreeEntryKind::Directory => fs::create_dir(&destination)
                .map_err(|error| io_error(destination.clone(), error))?,
            FileTreeEntryKind::File => {
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&destination)
                    .map_err(|error| io_error(destination.clone(), error))?;
            }
        }

        Ok(destination)
    }

    pub fn rename_entry(
        root: impl AsRef<Path>,
        source_path: &str,
        destination_path: &str,
    ) -> Result<PathBuf> {
        let root = root.as_ref();
        ensure_directory(root)?;
        let source = resolve_existing_entry(root, source_path)?;
        let destination_relative_path =
            normalize_relative_path(destination_path, PathUsage::Entry)?;
        let destination = root.join(&destination_relative_path);

        if source == destination {
            return Ok(destination);
        }

        ensure_existing_parent_directory(root, &destination_relative_path)?;
        ensure_missing(&destination)?;
        ensure_not_descendant_move(&source, &destination)?;
        fs::rename(&source, &destination).map_err(|error| io_error(source, error))?;

        Ok(destination)
    }

    pub fn move_entries(
        root: impl AsRef<Path>,
        source_paths: &[String],
        target_directory_path: Option<&str>,
    ) -> Result<Vec<PathBuf>> {
        let root = root.as_ref();
        ensure_directory(root)?;
        let target_directory = resolve_existing_directory(root, target_directory_path)?;
        let moves = prepare_directory_transfers(root, source_paths, &target_directory)?;
        let destinations = moves
            .iter()
            .map(|entry_move| entry_move.destination.clone())
            .collect::<Vec<_>>();

        for entry_move in moves {
            if entry_move.source == entry_move.destination {
                continue;
            }

            fs::rename(&entry_move.source, &entry_move.destination)
                .map_err(|error| io_error(entry_move.source, error))?;
        }

        Ok(destinations)
    }

    pub fn copy_entries(
        root: impl AsRef<Path>,
        source_paths: &[String],
        target_directory_path: Option<&str>,
    ) -> Result<Vec<PathBuf>> {
        let root = root.as_ref();
        ensure_directory(root)?;
        let target_directory = resolve_existing_directory(root, target_directory_path)?;
        let transfers = prepare_copy_transfers(root, source_paths, &target_directory)?;
        let destinations = transfers
            .iter()
            .map(|entry_move| entry_move.destination.clone())
            .collect::<Vec<_>>();

        for entry_move in transfers {
            copy_entry(&entry_move.source, &entry_move.destination)?;
        }

        Ok(destinations)
    }

    pub fn delete_entry(root: impl AsRef<Path>, path: &str) -> Result<()> {
        let root = root.as_ref();
        ensure_directory(root)?;
        let path = resolve_existing_entry(root, path)?;

        delete_resolved_entry(path)
    }

    pub fn delete_entries(root: impl AsRef<Path>, paths: &[String]) -> Result<()> {
        let root = root.as_ref();
        ensure_directory(root)?;
        let paths = prepare_delete_paths(root, paths)?;

        for path in paths {
            delete_resolved_entry(path)?;
        }

        Ok(())
    }

    pub fn resolve_path(root: impl AsRef<Path>, path: Option<&str>) -> Result<PathBuf> {
        let root = root.as_ref();
        ensure_directory(root)?;

        match path {
            Some(path) if !path.trim().is_empty() => resolve_existing_entry(root, path),
            _ => Ok(root.to_path_buf()),
        }
    }
}

fn delete_resolved_entry(path: PathBuf) -> Result<()> {
    let file_type = fs::symlink_metadata(&path)
        .map_err(|error| io_error(path.clone(), error))?
        .file_type();

    if file_type.is_dir() {
        fs::remove_dir_all(&path).map_err(|error| io_error(path, error))
    } else {
        fs::remove_file(&path).map_err(|error| io_error(path, error))
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
    CannotMoveIntoSelf {
        source: PathBuf,
        destination: PathBuf,
    },
    DirectoryNotFound(PathBuf),
    EntryAlreadyExists(PathBuf),
    EntryNotFound(PathBuf),
    InvalidName(String),
    InvalidPath(String),
    Io {
        path: PathBuf,
        error: io::Error,
    },
    RootNotDirectory(PathBuf),
    TabNotFound,
    UnsupportedEntry(PathBuf),
    WorkspaceNotFound,
}

impl fmt::Display for FileTreeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CannotMoveIntoSelf {
                source,
                destination,
            } => write!(
                formatter,
                "cannot move {} into {}",
                source.display(),
                destination.display()
            ),
            Self::DirectoryNotFound(path) => {
                write!(formatter, "directory does not exist: {}", path.display())
            }
            Self::EntryAlreadyExists(path) => {
                write!(formatter, "entry already exists: {}", path.display())
            }
            Self::EntryNotFound(path) => {
                write!(formatter, "entry does not exist: {}", path.display())
            }
            Self::InvalidName(name) => write!(formatter, "invalid file name: {name}"),
            Self::InvalidPath(path) => write!(formatter, "invalid file tree path: {path}"),
            Self::Io { path, error } => {
                write!(formatter, "could not access {}: {error}", path.display())
            }
            Self::RootNotDirectory(path) => {
                write!(formatter, "{} is not a directory", path.display())
            }
            Self::TabNotFound => formatter.write_str("file tree tab does not exist"),
            Self::UnsupportedEntry(path) => {
                write!(formatter, "unsupported file tree entry: {}", path.display())
            }
            Self::WorkspaceNotFound => formatter.write_str("workspace does not exist"),
        }
    }
}

impl StdError for FileTreeError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io { error, .. } => Some(error),
            Self::CannotMoveIntoSelf { .. }
            | Self::DirectoryNotFound(_)
            | Self::EntryAlreadyExists(_)
            | Self::EntryNotFound(_)
            | Self::InvalidName(_)
            | Self::InvalidPath(_)
            | Self::RootNotDirectory(_)
            | Self::TabNotFound
            | Self::UnsupportedEntry(_)
            | Self::WorkspaceNotFound => None,
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

fn resolve_existing_directory(root: &Path, path: Option<&str>) -> Result<PathBuf> {
    match path {
        Some(path) if !path.trim().is_empty() => {
            let path = normalize_relative_path(path, PathUsage::Directory)?;
            resolve_directory(root, &path)
        }
        _ => Ok(root.to_path_buf()),
    }
}

fn resolve_existing_entry(root: &Path, path: &str) -> Result<PathBuf> {
    let path = normalize_relative_path(path, PathUsage::Entry)?;
    ensure_existing_parent_directory(root, &path)?;
    let resolved_path = root.join(path);

    if resolved_path.exists() {
        Ok(resolved_path)
    } else {
        Err(FileTreeError::EntryNotFound(resolved_path))
    }
}

fn resolve_directory(root: &Path, path: &Path) -> Result<PathBuf> {
    let mut current = root.to_path_buf();

    for segment in path.iter() {
        current.push(segment);
        let metadata = fs::symlink_metadata(&current)
            .map_err(|error| map_not_found_to_directory_not_found(current.clone(), error))?;
        let file_type = metadata.file_type();

        if !file_type.is_dir() || file_type.is_symlink() {
            return Err(FileTreeError::DirectoryNotFound(current));
        }
    }

    Ok(current)
}

fn ensure_existing_parent_directory(root: &Path, path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    resolve_existing_directory(root, parent.map(path_to_str))
}

fn ensure_missing(path: &Path) -> Result<()> {
    if path.exists() {
        Err(FileTreeError::EntryAlreadyExists(path.to_path_buf()))
    } else {
        Ok(())
    }
}

fn ensure_not_descendant_move(source: &Path, destination: &Path) -> Result<()> {
    if source != destination && destination.starts_with(source) {
        Err(FileTreeError::CannotMoveIntoSelf {
            source: source.to_path_buf(),
            destination: destination.to_path_buf(),
        })
    } else {
        Ok(())
    }
}

fn map_not_found_to_directory_not_found(path: PathBuf, error: io::Error) -> FileTreeError {
    if error.kind() == io::ErrorKind::NotFound {
        FileTreeError::DirectoryNotFound(path)
    } else {
        io_error(path, error)
    }
}

fn prepare_directory_transfers(
    root: &Path,
    source_paths: &[String],
    target_directory: &Path,
) -> Result<Vec<EntryTransfer>> {
    if source_paths.is_empty() {
        return Err(FileTreeError::InvalidPath("empty selection".to_owned()));
    }

    let mut transfers = Vec::new();
    let mut destinations = HashSet::new();

    for source_path in source_paths {
        let source = resolve_existing_entry(root, source_path)?;
        let destination = target_directory.join(entry_name(&source)?);

        ensure_not_descendant_move(&source, &destination)?;

        if source != destination {
            ensure_missing(&destination)?;
        }

        if !destinations.insert(destination.clone()) {
            return Err(FileTreeError::EntryAlreadyExists(destination));
        }

        transfers.push(EntryTransfer {
            source,
            destination,
        });
    }

    Ok(transfers)
}

fn prepare_copy_transfers(
    root: &Path,
    source_paths: &[String],
    target_directory: &Path,
) -> Result<Vec<EntryTransfer>> {
    if source_paths.is_empty() {
        return Err(FileTreeError::InvalidPath("empty selection".to_owned()));
    }

    let mut transfers = Vec::new();
    let mut destinations = HashSet::new();

    for source_path in source_paths {
        let source = resolve_existing_entry(root, source_path)?;
        let destination = target_directory.join(entry_name(&source)?);
        let source_is_directory = fs::symlink_metadata(&source)
            .map_err(|error| io_error(source.clone(), error))?
            .file_type()
            .is_dir();

        ensure_not_descendant_move(&source, &destination)?;

        let destination =
            available_copy_destination(&destination, source_is_directory, &destinations)?;
        destinations.insert(destination.clone());
        transfers.push(EntryTransfer {
            source,
            destination,
        });
    }

    Ok(transfers)
}

fn available_copy_destination(
    destination: &Path,
    is_directory: bool,
    reserved_destinations: &HashSet<PathBuf>,
) -> Result<PathBuf> {
    if !destination.exists() && !reserved_destinations.contains(destination) {
        return Ok(destination.to_path_buf());
    }

    for index in 2.. {
        let destination = copy_destination_with_suffix(destination, is_directory, index)?;

        if !destination.exists() && !reserved_destinations.contains(&destination) {
            return Ok(destination);
        }
    }

    unreachable!("copy destination suffix search is unbounded")
}

fn copy_destination_with_suffix(
    destination: &Path,
    is_directory: bool,
    index: usize,
) -> Result<PathBuf> {
    let name = entry_name(destination)?
        .to_str()
        .ok_or_else(|| FileTreeError::InvalidPath(destination.to_string_lossy().into_owned()))?;

    Ok(destination.with_file_name(copy_name_with_suffix(name, is_directory, index)))
}

fn copy_name_with_suffix(name: &str, is_directory: bool, index: usize) -> String {
    if is_directory {
        return format!("{name} {index}");
    }

    let path = Path::new(name);
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return format!("{name} {index}");
    };

    if extension.is_empty() {
        return format!("{name} {index}");
    }

    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(name);

    format!("{stem} {index}.{extension}")
}

fn prepare_delete_paths(root: &Path, source_paths: &[String]) -> Result<Vec<PathBuf>> {
    if source_paths.is_empty() {
        return Err(FileTreeError::InvalidPath("empty selection".to_owned()));
    }

    let mut paths = source_paths
        .iter()
        .map(|source_path| resolve_existing_entry(root, source_path))
        .collect::<Result<Vec<_>>>()?;

    paths.sort();
    paths.dedup();
    paths.sort_by_key(|path| path.components().count());

    let mut selected_paths = Vec::new();

    for path in paths {
        if selected_paths
            .iter()
            .any(|selected| path.starts_with(selected))
        {
            continue;
        }

        selected_paths.push(path);
    }

    Ok(selected_paths)
}

fn entry_name(path: &Path) -> Result<&std::ffi::OsStr> {
    path.file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| FileTreeError::InvalidPath(path.to_string_lossy().into_owned()))
}

fn copy_entry(source: &Path, destination: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source).map_err(|error| io_error(source, error))?;
    let file_type = metadata.file_type();

    if file_type.is_symlink() {
        return Err(FileTreeError::UnsupportedEntry(source.to_path_buf()));
    }

    if file_type.is_dir() {
        copy_directory(source, destination)
    } else if file_type.is_file() {
        fs::copy(source, destination)
            .map(|_| ())
            .map_err(|error| io_error(source, error))
    } else {
        Err(FileTreeError::UnsupportedEntry(source.to_path_buf()))
    }
}

fn copy_directory(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir(destination).map_err(|error| io_error(destination, error))?;

    for entry in fs::read_dir(source).map_err(|error| io_error(source, error))? {
        let entry = entry.map_err(|error| io_error(source, error))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());

        copy_entry(&source_path, &destination_path)?;
    }

    Ok(())
}

fn normalize_entry_name(name: &str) -> Result<&str> {
    let name = name.trim();

    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
    {
        Err(FileTreeError::InvalidName(name.to_owned()))
    } else {
        Ok(name)
    }
}

fn normalize_relative_path(path: &str, usage: PathUsage) -> Result<PathBuf> {
    let normalized_path = match usage {
        PathUsage::Directory => path.trim().strip_suffix('/').unwrap_or(path.trim()),
        PathUsage::Entry => path.trim().strip_suffix('/').unwrap_or(path.trim()),
    };

    if normalized_path.is_empty()
        || normalized_path.starts_with('/')
        || normalized_path.contains('\\')
        || normalized_path.contains('\0')
    {
        return Err(FileTreeError::InvalidPath(path.to_owned()));
    }

    if normalized_path
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(FileTreeError::InvalidPath(path.to_owned()));
    }

    Ok(normalized_path.split('/').collect())
}

fn path_to_str(path: &Path) -> &str {
    path.to_str()
        .expect("validated file tree paths must be valid UTF-8")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PathUsage {
    Directory,
    Entry,
}

#[derive(Debug)]
struct EntryTransfer {
    source: PathBuf,
    destination: PathBuf,
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

    #[test]
    fn creates_files_and_directories_under_validated_parent() {
        let root = test_root("create");
        fs::create_dir(root.join("src")).expect("parent should be created");

        let file = FileTree::create_entry(&root, Some("src/"), "main.rs", FileTreeEntryKind::File)
            .expect("file should be created");
        let directory = FileTree::create_entry(
            &root,
            Some("src"),
            "components",
            FileTreeEntryKind::Directory,
        )
        .expect("directory should be created");

        assert_eq!(file, root.join("src/main.rs"));
        assert!(file.is_file());
        assert_eq!(directory, root.join("src/components"));
        assert!(directory.is_dir());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_invalid_create_names_and_paths() {
        let root = test_root("invalid-create");

        let invalid_name =
            FileTree::create_entry(&root, None, "../outside.txt", FileTreeEntryKind::File)
                .expect_err("path-like names should be rejected");
        let invalid_parent = FileTree::create_entry(
            &root,
            Some("../outside"),
            "file.txt",
            FileTreeEntryKind::File,
        )
        .expect_err("traversal parents should be rejected");

        assert!(matches!(invalid_name, FileTreeError::InvalidName(_)));
        assert!(matches!(invalid_parent, FileTreeError::InvalidPath(_)));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn renames_entries_with_destination_validation() {
        let root = test_root("rename");
        fs::create_dir(root.join("src")).expect("parent should be created");
        fs::write(root.join("src/main.rs"), b"fn main() {}").expect("file should be written");

        let destination = FileTree::rename_entry(&root, "src/main.rs", "src/lib.rs")
            .expect("entry should be renamed");

        assert_eq!(destination, root.join("src/lib.rs"));
        assert!(!root.join("src/main.rs").exists());
        assert!(root.join("src/lib.rs").is_file());

        let invalid_destination = FileTree::rename_entry(&root, "src/lib.rs", "../lib.rs")
            .expect_err("traversal destinations should be rejected");
        assert!(matches!(invalid_destination, FileTreeError::InvalidPath(_)));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn moves_entries_to_target_directory() {
        let root = test_root("move");
        fs::create_dir(root.join("src")).expect("source parent should be created");
        fs::create_dir(root.join("dest")).expect("target parent should be created");
        fs::write(root.join("src/main.rs"), b"fn main() {}").expect("file should be written");

        let destinations =
            FileTree::move_entries(&root, &["src/main.rs".to_owned()], Some("dest/"))
                .expect("entry should move");

        assert_eq!(destinations, &[root.join("dest/main.rs")]);
        assert!(!root.join("src/main.rs").exists());
        assert!(root.join("dest/main.rs").is_file());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn copies_directories_recursively() {
        let root = test_root("copy");
        fs::create_dir_all(root.join("src/components")).expect("source should be created");
        fs::create_dir(root.join("dest")).expect("target should be created");
        fs::write(root.join("src/components/button.tsx"), b"export {}")
            .expect("file should be written");

        let destinations = FileTree::copy_entries(&root, &["src/".to_owned()], Some("dest"))
            .expect("entry should copy");

        assert_eq!(destinations, &[root.join("dest/src")]);
        assert!(root.join("src/components/button.tsx").is_file());
        assert!(root.join("dest/src/components/button.tsx").is_file());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn copy_uses_unique_names_when_destination_exists() {
        let root = test_root("copy-collision");
        fs::create_dir_all(root.join("src/components")).expect("source should be created");
        fs::write(root.join("src/components/button.tsx"), b"export {}")
            .expect("nested file should be written");
        fs::write(root.join("README.md"), b"readme").expect("file should be written");
        fs::write(root.join("README 2.md"), b"readme copy")
            .expect("existing copy should be written");

        let destinations =
            FileTree::copy_entries(&root, &["README.md".to_owned(), "src/".to_owned()], None)
                .expect("entries should copy with unique names");

        assert_eq!(
            destinations,
            &[root.join("README 3.md"), root.join("src 2")]
        );
        assert!(root.join("README.md").is_file());
        assert!(root.join("README 3.md").is_file());
        assert!(root.join("src/components/button.tsx").is_file());
        assert!(root.join("src 2/components/button.tsx").is_file());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn deletes_files_and_directories() {
        let root = test_root("delete");
        fs::create_dir_all(root.join("src/components")).expect("directory should be created");
        fs::write(root.join("README.md"), b"readme").expect("file should be written");

        FileTree::delete_entry(&root, "README.md").expect("file should be deleted");
        FileTree::delete_entry(&root, "src/").expect("directory should be deleted");

        assert!(!root.join("README.md").exists());
        assert!(!root.join("src").exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn deletes_multiple_entries_once_when_selection_contains_descendants() {
        let root = test_root("delete-many");
        fs::create_dir_all(root.join("src/components")).expect("directory should be created");
        fs::write(root.join("src/components/button.tsx"), b"export {}")
            .expect("nested file should be written");
        fs::write(root.join("README.md"), b"readme").expect("file should be written");

        FileTree::delete_entries(
            &root,
            &[
                "src/".to_owned(),
                "src/components/button.tsx".to_owned(),
                "README.md".to_owned(),
            ],
        )
        .expect("selected entries should be deleted");

        assert!(!root.join("src").exists());
        assert!(!root.join("README.md").exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn resolves_only_existing_root_relative_paths() {
        let root = test_root("resolve");
        fs::write(root.join("README.md"), b"readme").expect("file should be written");

        assert_eq!(FileTree::resolve_path(&root, None).unwrap(), root);
        assert_eq!(
            FileTree::resolve_path(&root, Some("README.md")).unwrap(),
            root.join("README.md")
        );

        let missing = FileTree::resolve_path(&root, Some("missing.md"))
            .expect_err("missing entries should be rejected");
        assert!(matches!(missing, FileTreeError::EntryNotFound(_)));

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
