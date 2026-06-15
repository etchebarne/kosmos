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

#[derive(Debug)]
pub enum FileTreeError {
    WorkspaceUnavailable,
    Io { path: PathBuf, source: io::Error },
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
            Self::Io { path, source } => write!(formatter, "{}: {source}", path.display()),
        }
    }
}

impl StdError for FileTreeError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::WorkspaceUnavailable => None,
            Self::Io { source, .. } => Some(source),
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
