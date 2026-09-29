use std::error::Error as StdError;
use std::fmt;
use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{self, Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use crate::tree::{TabId, WorkspaceId};

pub type Result<T> = std::result::Result<T, EditorError>;

pub const MAX_EDITOR_FILE_BYTES: usize = 32 * 1024 * 1024;

static NEXT_TEMP_FILE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditorViewState {
    workspace_id: WorkspaceId,
    tab_id: TabId,
    path: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditorDocument {
    path: String,
    content: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditorLocation {
    workspace_root: PathBuf,
    relative_path: String,
    absolute_path: PathBuf,
}

impl EditorViewState {
    pub fn new(workspace_id: WorkspaceId, tab_id: TabId, path: impl Into<String>) -> Self {
        Self {
            workspace_id,
            tab_id,
            path: path.into(),
        }
    }

    pub fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }

    pub fn tab_id(&self) -> TabId {
        self.tab_id
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub(crate) fn set_path(&mut self, path: impl Into<String>) {
        self.path = path.into();
    }
}

impl EditorDocument {
    pub fn read(workspace_directory: impl AsRef<Path>, path: &str) -> Result<Self> {
        let path = normalize_path(path)?;
        let file_path = resolve_regular_file(workspace_directory.as_ref(), &path)?;
        let file = File::open(&file_path).map_err(|error| io_error(file_path.clone(), error))?;
        let expected_len = file
            .metadata()
            .map(|metadata| metadata.len() as usize)
            .unwrap_or_default();
        // Reject oversized files before reading them.
        if expected_len > MAX_EDITOR_FILE_BYTES {
            return Err(file_too_large(path, expected_len));
        }
        let mut bytes = Vec::with_capacity(expected_len);

        file.take((MAX_EDITOR_FILE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| io_error(file_path, error))?;

        if bytes.len() > MAX_EDITOR_FILE_BYTES {
            return Err(file_too_large(path, bytes.len()));
        }

        let content =
            String::from_utf8(bytes).map_err(|_| EditorError::InvalidUtf8(path.clone()))?;

        Ok(Self { path, content })
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn content(&self) -> &str {
        &self.content
    }
}

fn file_too_large(path: String, size_bytes: usize) -> EditorError {
    EditorError::FileTooLarge {
        path,
        size_bytes,
        max_bytes: MAX_EDITOR_FILE_BYTES,
    }
}

/// Identifies a document's on-disk version without reading its content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DocumentFingerprint {
    len: u64,
    modified: SystemTime,
    inode: u64,
}

impl DocumentFingerprint {
    pub fn read(workspace_directory: impl AsRef<Path>, path: &str) -> Result<Self> {
        let path = normalize_path(path)?;
        let file_path = resolve_regular_file(workspace_directory.as_ref(), &path)?;
        let metadata =
            fs::metadata(&file_path).map_err(|error| io_error(file_path.clone(), error))?;
        let modified = metadata
            .modified()
            .map_err(|error| io_error(file_path, error))?;

        Ok(Self {
            len: metadata.len(),
            modified,
            inode: metadata.ino(),
        })
    }
}

impl EditorLocation {
    pub fn resolve(workspace_directory: impl AsRef<Path>, path: &str) -> Result<Self> {
        let relative_path = normalize_path(path)?;
        let file_path = resolve_regular_file(workspace_directory.as_ref(), &relative_path)?;
        let workspace_root = canonical_workspace_root(workspace_directory.as_ref())?;

        Ok(Self {
            workspace_root,
            relative_path,
            absolute_path: file_path,
        })
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }

    pub fn absolute_path(&self) -> &Path {
        &self.absolute_path
    }
}

pub fn save_document(
    workspace_directory: impl AsRef<Path>,
    path: &str,
    content: &str,
) -> Result<()> {
    if content.len() > MAX_EDITOR_FILE_BYTES {
        return Err(EditorError::ContentTooLarge {
            max_bytes: MAX_EDITOR_FILE_BYTES,
        });
    }

    let path = normalize_path(path)?;
    let file_path = resolve_regular_file(workspace_directory.as_ref(), &path)?;
    let permissions = fs::metadata(&file_path)
        .map_err(|error| io_error(file_path.clone(), error))?
        .permissions();

    atomic_write(&file_path, content.as_bytes(), permissions)
        .map_err(|error| io_error(file_path, error))
}

pub fn normalize_path(path: &str) -> Result<String> {
    if path.is_empty()
        || path.starts_with('/')
        || Path::new(path).is_absolute()
        || path.contains(['\\', '\0'])
    {
        return Err(EditorError::InvalidPath(path.to_owned()));
    }

    if path
        .split('/')
        .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(EditorError::InvalidPath(path.to_owned()));
    }

    Ok(path.to_owned())
}

/// Resolves `path` to the canonical regular file it names. Symlinks are
/// followed, so reads and atomic saves go through to the real target and never
/// replace the link itself, but the target must stay inside the workspace.
fn resolve_regular_file(workspace_directory: &Path, path: &str) -> Result<PathBuf> {
    let canonical_root = canonical_workspace_root(workspace_directory)?;
    let file_path = workspace_directory.join(path);
    let canonical_file = canonicalize_entry(&file_path, path)?;

    if !canonical_file.starts_with(&canonical_root) {
        return Err(EditorError::PathOutsideWorkspace(path.to_owned()));
    }

    let metadata =
        fs::metadata(&canonical_file).map_err(|error| io_error(canonical_file.clone(), error))?;

    if !metadata.file_type().is_file() {
        return Err(EditorError::NotRegularFile(path.to_owned()));
    }

    Ok(canonical_file)
}

fn canonical_workspace_root(workspace_directory: &Path) -> Result<PathBuf> {
    let root_metadata = fs::metadata(workspace_directory)
        .map_err(|error| io_error(workspace_directory.to_path_buf(), error))?;

    if !root_metadata.is_dir() {
        return Err(EditorError::WorkspaceNotDirectory(
            workspace_directory.to_path_buf(),
        ));
    }

    fs::canonicalize(workspace_directory)
        .map_err(|error| io_error(workspace_directory.to_path_buf(), error))
}

/// Canonicalizes an existing workspace entry, distinguishing a missing entry
/// from a dangling symlink so callers get a precise error.
fn canonicalize_entry(file_path: &Path, path: &str) -> Result<PathBuf> {
    let metadata = match fs::symlink_metadata(file_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(EditorError::FileNotFound(path.to_owned()));
        }
        Err(error) => return Err(io_error(file_path.to_path_buf(), error)),
    };

    match fs::canonicalize(file_path) {
        Ok(canonical) => Ok(canonical),
        Err(error)
            if error.kind() == io::ErrorKind::NotFound && metadata.file_type().is_symlink() =>
        {
            Err(EditorError::DanglingSymlink(path.to_owned()))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Err(EditorError::FileNotFound(path.to_owned()))
        }
        Err(error) => Err(io_error(file_path.to_path_buf(), error)),
    }
}

fn atomic_write(path: &Path, content: &[u8], permissions: Permissions) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file has no parent"))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file has no name"))?
        .to_string_lossy();

    for _ in 0..100 {
        let id = NEXT_TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed);
        let temp_path = parent.join(format!(
            ".{file_name}.kosmos-save-{}-{id}",
            std::process::id()
        ));
        let mut temp_file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let write_result = (|| {
            temp_file.set_permissions(permissions)?;
            temp_file.write_all(content)?;
            temp_file.sync_all()
        })();
        drop(temp_file);

        if let Err(error) = write_result {
            let _ = fs::remove_file(temp_path);
            return Err(error);
        }

        if let Err(error) = fs::rename(&temp_path, path) {
            let _ = fs::remove_file(temp_path);
            return Err(error);
        }

        return Ok(());
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate an editor save temporary file",
    ))
}

fn io_error(path: PathBuf, source: io::Error) -> EditorError {
    EditorError::Io { path, source }
}

#[derive(Debug)]
pub enum EditorError {
    WorkspaceNotFound,
    SourceTabNotFound,
    TabNotFound,
    WorkspaceNotDirectory(PathBuf),
    InvalidPath(String),
    FileNotFound(String),
    /// A symlink whose target does not exist.
    DanglingSymlink(String),
    NotRegularFile(String),
    PathOutsideWorkspace(String),
    FileTooLarge {
        path: String,
        size_bytes: usize,
        max_bytes: usize,
    },
    ContentTooLarge {
        max_bytes: usize,
    },
    InvalidUtf8(String),
    Io {
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for EditorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WorkspaceNotFound => formatter.write_str("workspace does not exist"),
            Self::SourceTabNotFound => formatter.write_str("editor source tab does not exist"),
            Self::TabNotFound => formatter.write_str("editor tab does not exist"),
            Self::WorkspaceNotDirectory(path) => {
                write!(
                    formatter,
                    "workspace is not a directory: {}",
                    path.display()
                )
            }
            Self::InvalidPath(path) => write!(formatter, "invalid editor path: {path:?}"),
            Self::FileNotFound(path) => write!(formatter, "editor file does not exist: {path}"),
            Self::DanglingSymlink(path) => {
                write!(formatter, "{path} is a link to a file that does not exist.")
            }
            Self::NotRegularFile(path) => {
                write!(formatter, "editor path is not a regular file: {path}")
            }
            Self::PathOutsideWorkspace(path) => {
                write!(
                    formatter,
                    "editor path resolves outside the workspace: {path}"
                )
            }
            Self::FileTooLarge {
                path,
                size_bytes,
                max_bytes,
            } => {
                let name = path.rsplit('/').next().unwrap_or(path);
                let size = crate::byte_size::format_byte_size(*size_bytes);
                let limit = crate::byte_size::format_byte_size(*max_bytes);
                write!(
                    formatter,
                    "{name} is too large to open ({size}; the limit is {limit})."
                )
            }
            Self::ContentTooLarge { max_bytes } => {
                let limit = crate::byte_size::format_byte_size(*max_bytes);
                write!(
                    formatter,
                    "The document is too large to save (the limit is {limit})."
                )
            }
            Self::InvalidUtf8(path) => {
                write!(formatter, "editor file is not valid UTF-8: {path}")
            }
            Self::Io { path, source } => {
                write!(formatter, "could not access {}: {source}", path.display())
            }
        }
    }
}

impl StdError for EditorError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_only_safe_workspace_relative_paths() {
        assert_eq!(normalize_path("src/main.rs").unwrap(), "src/main.rs");
        assert_eq!(normalize_path("資料.txt").unwrap(), "資料.txt");

        for path in [
            "",
            "/etc/passwd",
            "src\\main.rs",
            "src\0main.rs",
            ".",
            "..",
            "src/./main.rs",
            "src/../main.rs",
            "src//main.rs",
            "src/",
        ] {
            assert!(
                matches!(normalize_path(path), Err(EditorError::InvalidPath(_))),
                "path should be rejected: {path:?}"
            );
        }
    }

    #[test]
    fn reads_only_bounded_utf8_regular_files() {
        let root = test_directory("reads");
        fs::write(root.join("valid.txt"), "hello").unwrap();
        fs::write(root.join("invalid.txt"), [0xff]).unwrap();
        fs::write(
            root.join("large.txt"),
            vec![b'a'; MAX_EDITOR_FILE_BYTES + 1],
        )
        .unwrap();
        fs::create_dir(root.join("directory.txt")).unwrap();

        let document = EditorDocument::read(&root, "valid.txt").unwrap();
        assert_eq!(document.path(), "valid.txt");
        assert_eq!(document.content(), "hello");
        assert!(matches!(
            EditorDocument::read(&root, "invalid.txt"),
            Err(EditorError::InvalidUtf8(_))
        ));
        assert!(matches!(
            EditorDocument::read(&root, "large.txt"),
            Err(EditorError::FileTooLarge { .. })
        ));
        assert!(matches!(
            EditorDocument::read(&root, "directory.txt"),
            Err(EditorError::NotRegularFile(_))
        ));

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn opens_and_saves_through_symlinks_inside_the_workspace() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let root = test_directory("symlinks");
        fs::create_dir(root.join("nested")).unwrap();
        let target = root.join("nested/target.txt");
        fs::write(&target, "before").unwrap();
        fs::set_permissions(&target, Permissions::from_mode(0o640)).unwrap();
        symlink("nested/target.txt", root.join("link.txt")).unwrap();

        let document = EditorDocument::read(&root, "link.txt").unwrap();
        assert_eq!(document.path(), "link.txt");
        assert_eq!(document.content(), "before");

        save_document(&root, "link.txt", "after").unwrap();

        let link_metadata = fs::symlink_metadata(root.join("link.txt")).unwrap();
        assert!(link_metadata.file_type().is_symlink());
        assert_eq!(fs::read_to_string(&target).unwrap(), "after");
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o640
        );
        assert_eq!(
            EditorLocation::resolve(&root, "link.txt")
                .unwrap()
                .absolute_path(),
            fs::canonicalize(&target).unwrap()
        );

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinks_resolving_outside_the_workspace() {
        use std::os::unix::fs::symlink;

        let root = test_directory("outside-symlinks");
        let outside = test_directory("outside");
        fs::write(outside.join("outside.txt"), "outside").unwrap();
        symlink(outside.join("outside.txt"), root.join("link.txt")).unwrap();
        symlink(&outside, root.join("outside")).unwrap();

        assert!(matches!(
            EditorDocument::read(&root, "link.txt"),
            Err(EditorError::PathOutsideWorkspace(_))
        ));
        assert!(matches!(
            save_document(&root, "link.txt", "changed"),
            Err(EditorError::PathOutsideWorkspace(_))
        ));
        assert!(matches!(
            EditorDocument::read(&root, "outside/outside.txt"),
            Err(EditorError::PathOutsideWorkspace(_))
        ));
        assert_eq!(
            fs::read_to_string(outside.join("outside.txt")).unwrap(),
            "outside"
        );

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_dangling_symlinks_and_symlinks_to_directories() {
        use std::os::unix::fs::symlink;

        let root = test_directory("dangling-symlinks");
        fs::create_dir(root.join("directory")).unwrap();
        symlink("missing.txt", root.join("dangling.txt")).unwrap();
        symlink("directory", root.join("directory-link")).unwrap();

        assert!(matches!(
            EditorDocument::read(&root, "dangling.txt"),
            Err(EditorError::DanglingSymlink(_))
        ));
        assert!(matches!(
            save_document(&root, "dangling.txt", "content"),
            Err(EditorError::DanglingSymlink(_))
        ));
        assert!(!root.join("missing.txt").exists());
        assert!(matches!(
            EditorDocument::read(&root, "directory-link"),
            Err(EditorError::NotRegularFile(_))
        ));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn saves_existing_files_and_rejects_oversized_content() {
        let root = test_directory("save");
        let path = root.join("document.txt");
        fs::write(&path, "before").unwrap();

        save_document(&root, "document.txt", "after").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "after");
        assert!(matches!(
            save_document(
                &root,
                "document.txt",
                &"a".repeat(MAX_EDITOR_FILE_BYTES + 1)
            ),
            Err(EditorError::ContentTooLarge { .. })
        ));
        assert!(matches!(
            save_document(&root, "missing.txt", "content"),
            Err(EditorError::FileNotFound(_))
        ));

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_save_preserves_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let root = test_directory("permissions");
        let path = root.join("document.txt");
        fs::write(&path, "before").unwrap();
        fs::set_permissions(&path, Permissions::from_mode(0o640)).unwrap();

        save_document(&root, "document.txt", "after").unwrap();

        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );

        let _ = fs::remove_dir_all(root);
    }

    fn test_directory(name: &str) -> PathBuf {
        let id = NEXT_TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "kosmos-core-editor-{}-{name}-{id}",
            std::process::id()
        ));

        fs::create_dir_all(&root).unwrap();
        root
    }
}
