use std::error::Error as StdError;
use std::fmt;
use std::fs::{self, FileType};
use std::io;
use std::path::{Path, PathBuf};

pub const DEFAULT_FILE_TREE_MAX_DEPTH: usize = 6;
pub const DEFAULT_FILE_TREE_MAX_ENTRIES_PER_DIRECTORY: usize = 500;
pub const MAX_FILE_TREE_MAX_DEPTH: usize = 16;
pub const MAX_FILE_TREE_MAX_ENTRIES_PER_DIRECTORY: usize = 5_000;

const DEFAULT_FILE_TREE_MAX_TOTAL_ENTRIES: usize = 10_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileTreeOptions {
    max_depth: usize,
    max_entries_per_directory: usize,
}

impl FileTreeOptions {
    pub fn new(max_depth: usize, max_entries_per_directory: usize) -> Self {
        Self {
            max_depth: max_depth.min(MAX_FILE_TREE_MAX_DEPTH),
            max_entries_per_directory: max_entries_per_directory
                .clamp(1, MAX_FILE_TREE_MAX_ENTRIES_PER_DIRECTORY),
        }
    }

    pub fn max_depth(self) -> usize {
        self.max_depth
    }

    pub fn max_entries_per_directory(self) -> usize {
        self.max_entries_per_directory
    }
}

impl Default for FileTreeOptions {
    fn default() -> Self {
        Self {
            max_depth: DEFAULT_FILE_TREE_MAX_DEPTH,
            max_entries_per_directory: DEFAULT_FILE_TREE_MAX_ENTRIES_PER_DIRECTORY,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileTree {
    root: FileTreeEntry,
}

impl FileTree {
    pub fn read(root: impl AsRef<Path>, options: FileTreeOptions) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let file_type = fs::symlink_metadata(&root)
            .map_err(|error| FileTreeError::io(root.clone(), error))?
            .file_type();
        let mut remaining_entries = DEFAULT_FILE_TREE_MAX_TOTAL_ENTRIES;

        Ok(Self {
            root: read_entry(root, file_type, 0, options, &mut remaining_entries),
        })
    }

    pub fn root(&self) -> &FileTreeEntry {
        &self.root
    }
}

pub fn create_entry(
    root: impl AsRef<Path>,
    parent: impl AsRef<Path>,
    name: &str,
    kind: FileTreeCreateKind,
) -> Result<PathBuf> {
    validate_entry_name(name)?;

    let root = canonical_workspace_root(root.as_ref())?;
    let parent = resolve_existing_directory(&root, parent.as_ref())?;
    let target = parent.join(name);

    if path_exists(&target) {
        return Err(FileTreeError::AlreadyExists(target));
    }

    match kind {
        FileTreeCreateKind::File => fs::File::create(&target)
            .map(|_| ())
            .map_err(|error| FileTreeError::io(target.clone(), error))?,
        FileTreeCreateKind::Directory => {
            fs::create_dir(&target).map_err(|error| FileTreeError::io(target.clone(), error))?
        }
    }

    Ok(target)
}

pub fn rename_entry(root: impl AsRef<Path>, path: impl AsRef<Path>, name: &str) -> Result<PathBuf> {
    validate_entry_name(name)?;

    let root = canonical_workspace_root(root.as_ref())?;
    let source = resolve_existing_entry(&root, path.as_ref())?;
    reject_workspace_root_operation(&root, &source)?;
    let target = source
        .parent()
        .expect("resolved entries must have a parent")
        .join(name);

    if path_exists(&target) {
        return Err(FileTreeError::AlreadyExists(target));
    }

    fs::rename(&source, &target).map_err(|error| FileTreeError::io(source, error))?;
    Ok(target)
}

pub fn delete_entry(root: impl AsRef<Path>, path: impl AsRef<Path>) -> Result<()> {
    let root = canonical_workspace_root(root.as_ref())?;
    let path = resolve_existing_entry(&root, path.as_ref())?;
    reject_workspace_root_operation(&root, &path)?;
    let file_type = fs::symlink_metadata(&path)
        .map_err(|error| FileTreeError::io(path.clone(), error))?
        .file_type();

    if file_type.is_dir() {
        fs::remove_dir_all(&path).map_err(|error| FileTreeError::io(path, error))
    } else {
        fs::remove_file(&path).map_err(|error| FileTreeError::io(path, error))
    }
}

pub fn move_entry(
    root: impl AsRef<Path>,
    path: impl AsRef<Path>,
    target_directory: impl AsRef<Path>,
) -> Result<PathBuf> {
    let root = canonical_workspace_root(root.as_ref())?;
    let source = resolve_existing_entry(&root, path.as_ref())?;
    reject_workspace_root_operation(&root, &source)?;
    let target_directory = resolve_existing_directory(&root, target_directory.as_ref())?;
    let target = target_directory.join(
        source
            .file_name()
            .ok_or_else(|| FileTreeError::InvalidPath(source.clone()))?,
    );

    if source == target {
        return Ok(source);
    }
    if source_is_directory(&source)? && target_directory.starts_with(&source) {
        return Err(FileTreeError::InvalidMove);
    }
    if path_exists(&target) {
        return Err(FileTreeError::AlreadyExists(target));
    }

    fs::rename(&source, &target).map_err(|error| FileTreeError::io(source, error))?;
    Ok(target)
}

pub fn copy_entry(
    root: impl AsRef<Path>,
    path: impl AsRef<Path>,
    target_directory: impl AsRef<Path>,
) -> Result<PathBuf> {
    let root = canonical_workspace_root(root.as_ref())?;
    let source = resolve_existing_entry(&root, path.as_ref())?;
    reject_workspace_root_operation(&root, &source)?;
    let target_directory = resolve_existing_directory(&root, target_directory.as_ref())?;

    if source_is_directory(&source)? && target_directory.starts_with(&source) {
        return Err(FileTreeError::InvalidCopy);
    }

    let target = available_copy_target(&source, &target_directory)?;

    copy_entry_to(&source, &target)?;
    Ok(target)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileTreeEntry {
    name: String,
    path: PathBuf,
    kind: FileTreeEntryKind,
    children: Vec<FileTreeEntry>,
    children_truncated: bool,
    read_error: Option<String>,
}

impl FileTreeEntry {
    fn new(name: String, path: PathBuf, kind: FileTreeEntryKind) -> Self {
        Self {
            name,
            path,
            kind,
            children: Vec::new(),
            children_truncated: false,
            read_error: None,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn kind(&self) -> FileTreeEntryKind {
        self.kind
    }

    pub fn children(&self) -> &[FileTreeEntry] {
        &self.children
    }

    pub fn children_truncated(&self) -> bool {
        self.children_truncated
    }

    pub fn read_error(&self) -> Option<&str> {
        self.read_error.as_deref()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileTreeEntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileTreeCreateKind {
    File,
    Directory,
}

#[derive(Debug)]
pub enum FileTreeError {
    WorkspaceUnavailable,
    AlreadyExists(PathBuf),
    InvalidCopy,
    InvalidMove,
    InvalidName(String),
    InvalidPath(PathBuf),
    Io { path: PathBuf, source: io::Error },
    NotDirectory(PathBuf),
    OutsideWorkspace(PathBuf),
    RootOperation,
}

impl FileTreeError {
    fn io(path: PathBuf, source: io::Error) -> Self {
        Self::Io { path, source }
    }
}

impl fmt::Display for FileTreeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WorkspaceUnavailable => formatter.write_str("workspace is not available"),
            Self::AlreadyExists(path) => write!(formatter, "{} already exists", path.display()),
            Self::InvalidCopy => formatter.write_str("entry cannot be copied there"),
            Self::InvalidMove => formatter.write_str("entry cannot be moved there"),
            Self::InvalidName(name) => write!(formatter, "invalid file name {name:?}"),
            Self::InvalidPath(path) => write!(formatter, "invalid path {}", path.display()),
            Self::Io { path, source } => write!(formatter, "{}: {source}", path.display()),
            Self::NotDirectory(path) => write!(formatter, "{} is not a directory", path.display()),
            Self::OutsideWorkspace(path) => {
                write!(formatter, "{} is outside the workspace", path.display())
            }
            Self::RootOperation => formatter.write_str("workspace root cannot be changed"),
        }
    }
}

impl StdError for FileTreeError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::WorkspaceUnavailable => None,
            Self::AlreadyExists(_) => None,
            Self::InvalidCopy => None,
            Self::InvalidMove => None,
            Self::InvalidName(_) => None,
            Self::InvalidPath(_) => None,
            Self::Io { source, .. } => Some(source),
            Self::NotDirectory(_) => None,
            Self::OutsideWorkspace(_) => None,
            Self::RootOperation => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, FileTreeError>;

fn read_entry(
    path: PathBuf,
    file_type: FileType,
    depth: usize,
    options: FileTreeOptions,
    remaining_entries: &mut usize,
) -> FileTreeEntry {
    let kind = entry_kind(file_type);
    let name = entry_name(&path);

    read_entry_from_parts(path, name, kind, depth, options, remaining_entries)
}

fn canonical_workspace_root(root: &Path) -> Result<PathBuf> {
    fs::canonicalize(root).map_err(|error| FileTreeError::io(root.to_path_buf(), error))
}

fn resolve_existing_entry(root: &Path, path: &Path) -> Result<PathBuf> {
    if is_workspace_root_path(root, path) {
        return Ok(root.to_path_buf());
    }

    let parent = path
        .parent()
        .ok_or_else(|| FileTreeError::InvalidPath(path.to_path_buf()))?;
    let parent =
        fs::canonicalize(parent).map_err(|error| FileTreeError::io(parent.to_path_buf(), error))?;

    ensure_in_workspace(root, &parent)?;

    let file_name = path
        .file_name()
        .ok_or_else(|| FileTreeError::InvalidPath(path.to_path_buf()))?;
    let resolved = parent.join(file_name);

    fs::symlink_metadata(&resolved).map_err(|error| FileTreeError::io(resolved.clone(), error))?;
    Ok(resolved)
}

fn is_workspace_root_path(root: &Path, path: &Path) -> bool {
    if path == root {
        return true;
    }

    let Some(root_name) = root.file_name() else {
        return false;
    };
    let Some(path_name) = path.file_name() else {
        return false;
    };
    if path_name != root_name {
        return false;
    }

    let Some(root_parent) = root.parent() else {
        return false;
    };
    let Some(path_parent) = path.parent() else {
        return false;
    };

    fs::canonicalize(path_parent).is_ok_and(|parent| parent == root_parent)
}

fn resolve_existing_directory(root: &Path, path: &Path) -> Result<PathBuf> {
    let path =
        fs::canonicalize(path).map_err(|error| FileTreeError::io(path.to_path_buf(), error))?;

    ensure_in_workspace(root, &path)?;
    if !path.is_dir() {
        return Err(FileTreeError::NotDirectory(path));
    }

    Ok(path)
}

fn ensure_in_workspace(root: &Path, path: &Path) -> Result<()> {
    if path == root || path.starts_with(root) {
        Ok(())
    } else {
        Err(FileTreeError::OutsideWorkspace(path.to_path_buf()))
    }
}

fn reject_workspace_root_operation(root: &Path, path: &Path) -> Result<()> {
    if path == root {
        Err(FileTreeError::RootOperation)
    } else {
        Ok(())
    }
}

fn validate_entry_name(name: &str) -> Result<()> {
    let invalid = name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(std::path::MAIN_SEPARATOR)
        || name.contains('/');

    if invalid {
        Err(FileTreeError::InvalidName(name.to_owned()))
    } else {
        Ok(())
    }
}

fn source_is_directory(path: &Path) -> Result<bool> {
    Ok(fs::symlink_metadata(path)
        .map_err(|error| FileTreeError::io(path.to_path_buf(), error))?
        .file_type()
        .is_dir())
}

fn path_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn available_copy_target(source: &Path, target_directory: &Path) -> Result<PathBuf> {
    let file_name = source
        .file_name()
        .ok_or_else(|| FileTreeError::InvalidPath(source.to_path_buf()))?;
    let target = target_directory.join(file_name);

    if target != source && !path_exists(&target) {
        return Ok(target);
    }

    let file_name = file_name.to_string_lossy();
    let mut index = 1;

    loop {
        let target = target_directory.join(numbered_copy_name(&file_name, index));
        if target != source && !path_exists(&target) {
            return Ok(target);
        }
        index += 1;
    }
}

fn numbered_copy_name(file_name: &str, index: usize) -> String {
    match file_name.rfind('.') {
        Some(dot_index) if dot_index > 0 && dot_index + 1 < file_name.len() => {
            let (name, extension) = file_name.split_at(dot_index);
            format!("{name} ({index}){extension}")
        }
        _ => format!("{file_name} ({index})"),
    }
}

fn copy_entry_to(source: &Path, target: &Path) -> Result<()> {
    let file_type = fs::symlink_metadata(source)
        .map_err(|error| FileTreeError::io(source.to_path_buf(), error))?
        .file_type();

    if file_type.is_dir() {
        copy_directory_to(source, target)
    } else if file_type.is_symlink() {
        copy_symlink_to(source, target)
    } else {
        fs::copy(source, target)
            .map(|_| ())
            .map_err(|error| FileTreeError::io(source.to_path_buf(), error))
    }
}

fn copy_directory_to(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir(target).map_err(|error| FileTreeError::io(target.to_path_buf(), error))?;

    for child in
        fs::read_dir(source).map_err(|error| FileTreeError::io(source.to_path_buf(), error))?
    {
        let child = child.map_err(|error| FileTreeError::io(source.to_path_buf(), error))?;
        let child_source = child.path();
        let child_target = target.join(child.file_name());

        if path_exists(&child_target) {
            return Err(FileTreeError::AlreadyExists(child_target));
        }

        copy_entry_to(&child_source, &child_target)?;
    }

    Ok(())
}

#[cfg(unix)]
fn copy_symlink_to(source: &Path, target: &Path) -> Result<()> {
    let link_target =
        fs::read_link(source).map_err(|error| FileTreeError::io(source.to_path_buf(), error))?;
    std::os::unix::fs::symlink(link_target, target)
        .map_err(|error| FileTreeError::io(target.to_path_buf(), error))
}

#[cfg(windows)]
fn copy_symlink_to(source: &Path, target: &Path) -> Result<()> {
    let link_target =
        fs::read_link(source).map_err(|error| FileTreeError::io(source.to_path_buf(), error))?;
    let metadata =
        fs::metadata(source).map_err(|error| FileTreeError::io(source.to_path_buf(), error))?;

    if metadata.is_dir() {
        std::os::windows::fs::symlink_dir(link_target, target)
    } else {
        std::os::windows::fs::symlink_file(link_target, target)
    }
    .map_err(|error| FileTreeError::io(target.to_path_buf(), error))
}

fn read_entry_from_parts(
    path: PathBuf,
    name: String,
    kind: FileTreeEntryKind,
    depth: usize,
    options: FileTreeOptions,
    remaining_entries: &mut usize,
) -> FileTreeEntry {
    let mut entry = FileTreeEntry::new(name, path, kind);

    if kind != FileTreeEntryKind::Directory {
        return entry;
    }

    if depth >= options.max_depth() {
        entry.children_truncated = true;
        return entry;
    }

    match read_children(entry.path(), depth + 1, options, remaining_entries) {
        Ok((children, children_truncated)) => {
            entry.children = children;
            entry.children_truncated = children_truncated;
        }
        Err(error) => entry.read_error = Some(error.to_string()),
    }

    entry
}

fn read_children(
    directory: &Path,
    depth: usize,
    options: FileTreeOptions,
    remaining_entries: &mut usize,
) -> io::Result<(Vec<FileTreeEntry>, bool)> {
    if *remaining_entries == 0 {
        return Ok((Vec::new(), true));
    }

    let mut children = Vec::new();
    let mut child_entries = Vec::new();
    let mut children_truncated = false;

    for child in fs::read_dir(directory)? {
        if child_entries.len() >= options.max_entries_per_directory() {
            children_truncated = true;
            break;
        }

        let Ok(child) = child else {
            continue;
        };
        let Ok(file_type) = child.file_type() else {
            continue;
        };

        let path = child.path();
        let kind = entry_kind(file_type);
        child_entries.push(ChildEntry {
            name: entry_name(&path),
            path,
            kind,
        });
    }

    child_entries.sort_by(compare_child_entries);

    for child in child_entries {
        if *remaining_entries == 0 {
            children_truncated = true;
            break;
        }

        *remaining_entries -= 1;
        let ChildEntry { name, path, kind } = child;
        children.push(read_entry_from_parts(
            path,
            name,
            kind,
            depth,
            options,
            remaining_entries,
        ));
    }

    Ok((children, children_truncated))
}

struct ChildEntry {
    name: String,
    path: PathBuf,
    kind: FileTreeEntryKind,
}

fn compare_child_entries(first: &ChildEntry, second: &ChildEntry) -> std::cmp::Ordering {
    compare_entry_parts(first.kind, &first.name, second.kind, &second.name)
}

fn compare_entry_parts(
    first_kind: FileTreeEntryKind,
    first_name: &str,
    second_kind: FileTreeEntryKind,
    second_name: &str,
) -> std::cmp::Ordering {
    first_kind
        .sort_order()
        .cmp(&second_kind.sort_order())
        .then_with(|| hidden_sort_order(first_name).cmp(&hidden_sort_order(second_name)))
        .then_with(|| first_name.cmp(second_name))
}

fn hidden_sort_order(name: &str) -> u8 {
    if name.starts_with('.') { 1 } else { 0 }
}

fn entry_kind(file_type: FileType) -> FileTreeEntryKind {
    if file_type.is_dir() {
        FileTreeEntryKind::Directory
    } else if file_type.is_file() {
        FileTreeEntryKind::File
    } else if file_type.is_symlink() {
        FileTreeEntryKind::Symlink
    } else {
        FileTreeEntryKind::Other
    }
}

impl FileTreeEntryKind {
    fn sort_order(self) -> u8 {
        match self {
            Self::Directory => 0,
            Self::File => 1,
            Self::Symlink => 2,
            Self::Other => 3,
        }
    }
}

fn entry_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn reads_directories_before_files() {
        let root = test_directory("sorted");
        fs::create_dir_all(root.join("src")).expect("directory should be created");
        fs::write(root.join("README.md"), "readme").expect("file should be written");
        fs::write(root.join("src").join("main.rs"), "fn main() {}")
            .expect("file should be written");

        let tree = FileTree::read(&root, FileTreeOptions::new(2, 100)).expect("tree should read");

        let names = tree
            .root()
            .children()
            .iter()
            .map(FileTreeEntry::name)
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["src", "README.md"]);
        assert_eq!(tree.root().children()[0].children()[0].name(), "main.rs");

        remove_test_directory(root);
    }

    #[test]
    fn marks_directories_truncated_at_max_depth() {
        let root = test_directory("depth");
        fs::create_dir_all(root.join("src").join("nested")).expect("directory should be created");

        let tree = FileTree::read(&root, FileTreeOptions::new(1, 100)).expect("tree should read");
        let src = &tree.root().children()[0];

        assert_eq!(src.name(), "src");
        assert!(src.children().is_empty());
        assert!(src.children_truncated());

        remove_test_directory(root);
    }

    #[test]
    fn creates_file_and_directory_entries() {
        let root = test_directory("create");
        let parent = root.join("src");
        fs::create_dir_all(&parent).expect("directory should be created");

        let file = create_entry(&root, &parent, "main.rs", FileTreeCreateKind::File)
            .expect("file should be created");
        let directory = create_entry(&root, &parent, "nested", FileTreeCreateKind::Directory)
            .expect("directory should be created");

        assert!(file.is_file());
        assert!(directory.is_dir());

        remove_test_directory(root);
    }

    #[test]
    fn renames_entries_without_changing_parent() {
        let root = test_directory("rename");
        let source = root.join("old.txt");
        fs::write(&source, "old").expect("file should be written");

        let target = rename_entry(&root, &source, "new.txt").expect("entry should be renamed");

        assert_eq!(target, root.join("new.txt"));
        assert!(!source.exists());
        assert!(target.is_file());

        remove_test_directory(root);
    }

    #[test]
    fn deletes_files_and_directories() {
        let root = test_directory("delete");
        let file = root.join("delete-me.txt");
        let directory = root.join("delete-me");
        fs::write(&file, "delete").expect("file should be written");
        fs::create_dir_all(directory.join("nested")).expect("directory should be created");

        delete_entry(&root, &file).expect("file should be deleted");
        delete_entry(&root, &directory).expect("directory should be deleted");

        assert!(!file.exists());
        assert!(!directory.exists());

        remove_test_directory(root);
    }

    #[test]
    fn moves_entries_into_target_directory() {
        let root = test_directory("move");
        let source = root.join("main.rs");
        let target_directory = root.join("src");
        fs::write(&source, "fn main() {}").expect("file should be written");
        fs::create_dir_all(&target_directory).expect("directory should be created");

        let target = move_entry(&root, &source, &target_directory).expect("entry should be moved");

        assert_eq!(target, target_directory.join("main.rs"));
        assert!(!source.exists());
        assert!(target.is_file());

        remove_test_directory(root);
    }

    #[test]
    fn copies_files_and_directories_into_target_directory() {
        let root = test_directory("copy");
        let file = root.join("main.rs");
        let source_directory = root.join("src");
        let target_directory = root.join("target");
        fs::write(&file, "fn main() {}").expect("file should be written");
        fs::create_dir_all(source_directory.join("nested")).expect("directory should be created");
        fs::write(
            source_directory.join("nested").join("lib.rs"),
            "pub fn lib() {}",
        )
        .expect("file should be written");
        fs::create_dir_all(&target_directory).expect("directory should be created");

        let copied_file =
            copy_entry(&root, &file, &target_directory).expect("file should be copied");
        let copied_directory = copy_entry(&root, &source_directory, &target_directory)
            .expect("directory should be copied");

        assert_eq!(copied_file, target_directory.join("main.rs"));
        assert_eq!(copied_directory, target_directory.join("src"));
        assert!(file.is_file());
        assert!(copied_file.is_file());
        assert!(copied_directory.join("nested").join("lib.rs").is_file());

        remove_test_directory(root);
    }

    #[test]
    fn copies_entries_with_numbered_names_when_target_exists() {
        let root = test_directory("copy-numbered");
        let file = root.join("main.rs");
        let directory = root.join("src");
        fs::write(&file, "fn main() {}").expect("file should be written");
        fs::write(root.join("main (1).rs"), "existing").expect("file should be written");
        fs::create_dir_all(directory.join("nested")).expect("directory should be created");

        let copied_file = copy_entry(&root, &file, &root).expect("file should be copied");
        let copied_directory =
            copy_entry(&root, &directory, &root).expect("directory should be copied");

        assert_eq!(copied_file, root.join("main (2).rs"));
        assert_eq!(copied_directory, root.join("src (1)"));
        assert!(file.is_file());
        assert!(copied_file.is_file());
        assert!(directory.is_dir());
        assert!(copied_directory.join("nested").is_dir());

        remove_test_directory(root);
    }

    #[cfg(unix)]
    #[test]
    fn treats_broken_symlink_targets_as_existing() {
        let root = test_directory("broken-symlink-targets");
        let source = root.join("source.txt");
        let target_directory = root.join("target");
        let broken = root.join("broken.txt");
        let move_source = root.join("move.txt");
        let move_target = target_directory.join("move.txt");
        let copy_source = root.join("copy.txt");
        let copy_target = target_directory.join("copy.txt");

        fs::write(&source, "source").expect("file should be written");
        fs::write(&move_source, "move").expect("file should be written");
        fs::write(&copy_source, "copy").expect("file should be written");
        fs::create_dir_all(&target_directory).expect("directory should be created");
        std::os::unix::fs::symlink(root.join("missing.txt"), &broken)
            .expect("symlink should be created");
        std::os::unix::fs::symlink(root.join("missing-move.txt"), &move_target)
            .expect("symlink should be created");
        std::os::unix::fs::symlink(root.join("missing-copy.txt"), &copy_target)
            .expect("symlink should be created");

        assert!(matches!(
            create_entry(&root, &root, "broken.txt", FileTreeCreateKind::File),
            Err(FileTreeError::AlreadyExists(_))
        ));
        assert!(matches!(
            rename_entry(&root, &source, "broken.txt"),
            Err(FileTreeError::AlreadyExists(_))
        ));
        assert!(matches!(
            move_entry(&root, &move_source, &target_directory),
            Err(FileTreeError::AlreadyExists(_))
        ));

        let copied = copy_entry(&root, &copy_source, &target_directory)
            .expect("copy should use a numbered target");
        assert_eq!(copied, target_directory.join("copy (1).txt"));
        assert!(
            fs::symlink_metadata(copy_target)
                .expect("symlink should still exist")
                .file_type()
                .is_symlink()
        );

        remove_test_directory(root);
    }

    #[test]
    fn rejects_root_entry_mutations() {
        let root = test_directory("root-guard");
        let target_directory = root.join("target");
        fs::create_dir_all(&target_directory).expect("directory should be created");

        assert!(matches!(
            rename_entry(&root, &root, "renamed"),
            Err(FileTreeError::RootOperation)
        ));
        assert!(matches!(
            delete_entry(&root, &root),
            Err(FileTreeError::RootOperation)
        ));
        assert!(matches!(
            move_entry(&root, &root, &target_directory),
            Err(FileTreeError::RootOperation)
        ));
        assert!(matches!(
            copy_entry(&root, &root, &target_directory),
            Err(FileTreeError::RootOperation)
        ));

        remove_test_directory(root);
    }

    #[test]
    fn rejects_paths_outside_workspace() {
        let root = test_directory("outside-root");
        let outside = test_directory("outside-target");
        let outside_file = outside.join("outside.txt");
        fs::write(&outside_file, "outside").expect("file should be written");

        assert!(matches!(
            create_entry(&root, &outside, "new.txt", FileTreeCreateKind::File),
            Err(FileTreeError::OutsideWorkspace(_))
        ));
        assert!(matches!(
            rename_entry(&root, &outside_file, "renamed.txt"),
            Err(FileTreeError::OutsideWorkspace(_))
        ));

        remove_test_directory(root);
        remove_test_directory(outside);
    }

    #[test]
    fn rejects_moving_directory_into_descendant() {
        let root = test_directory("move-descendant");
        let source = root.join("src");
        let descendant = source.join("nested");
        fs::create_dir_all(&descendant).expect("directory should be created");

        assert!(matches!(
            move_entry(&root, &source, &descendant),
            Err(FileTreeError::InvalidMove)
        ));

        remove_test_directory(root);
    }

    #[test]
    fn rejects_copying_directory_into_descendant() {
        let root = test_directory("copy-descendant");
        let source = root.join("src");
        let descendant = source.join("nested");
        fs::create_dir_all(&descendant).expect("directory should be created");

        assert!(matches!(
            copy_entry(&root, &source, &descendant),
            Err(FileTreeError::InvalidCopy)
        ));

        remove_test_directory(root);
    }

    fn test_directory(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "kosmos-core-file-tree-{}-{name}-{nanos}",
            std::process::id()
        ));

        fs::create_dir_all(&directory).expect("test directory should be created");
        directory
    }

    fn remove_test_directory(directory: PathBuf) {
        let _ = fs::remove_dir_all(directory);
    }
}
