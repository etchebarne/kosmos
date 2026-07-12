use std::ffi::{CString, OsStr, OsString};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path};
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

use crate::tabs::editor::MAX_EDITOR_FILE_BYTES;

static NEXT_TEMP_FILE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FileIdentity {
    device: u64,
    inode: u64,
}

#[derive(Debug)]
pub(super) enum SecureReplaceError {
    Safe(String),
    InstalledChanged {
        message: String,
        installed_identity: FileIdentity,
    },
}

impl From<String> for SecureReplaceError {
    fn from(message: String) -> Self {
        Self::Safe(message)
    }
}

#[derive(Debug)]
pub(super) struct SecureEditFile {
    directories: Vec<DirectoryIdentity>,
    file_name: OsString,
    original_identity: FileIdentity,
    mode: libc::mode_t,
}

#[derive(Debug)]
struct DirectoryIdentity {
    name: Option<OsString>,
    identity: FileIdentity,
}

impl SecureEditFile {
    pub(super) fn snapshot(
        canonical_workspace_root: &Path,
        relative_path: &str,
    ) -> Result<(Self, String), String> {
        if !canonical_workspace_root.is_absolute() {
            return Err("workspace root is not absolute".to_owned());
        }
        let relative = Path::new(relative_path);
        let file_name = relative
            .file_name()
            .ok_or_else(|| "workspace edit target has no file name".to_owned())?
            .to_os_string();
        let mut components = canonical_components(canonical_workspace_root)?;
        components.extend(parent_components(relative)?);
        let (parent, directories) = open_directory_chain(&components, None)?;
        let file = open_file_at(parent.as_raw_fd(), &file_name, libc::O_RDONLY)?;
        let stat = file_stat(file.as_raw_fd())?;
        ensure_regular_file(&stat)?;
        let content = read_text(file)?;
        Ok((
            Self {
                directories,
                file_name,
                original_identity: identity(&stat),
                mode: stat.st_mode & 0o7777,
            },
            content,
        ))
    }

    pub(super) fn original_identity(&self) -> FileIdentity {
        self.original_identity
    }

    pub(super) fn validate(
        &self,
        expected_identity: FileIdentity,
        expected_hash: &str,
    ) -> Result<(), String> {
        let parent = self.open_parent()?;
        validate_file_at(
            parent.as_raw_fd(),
            &self.file_name,
            expected_identity,
            expected_hash,
        )
    }

    pub(super) fn replace(
        &self,
        expected_identity: FileIdentity,
        expected_hash: &str,
        content: &str,
    ) -> Result<FileIdentity, SecureReplaceError> {
        let parent = self.open_parent()?;
        validate_file_at(
            parent.as_raw_fd(),
            &self.file_name,
            expected_identity,
            expected_hash,
        )?;
        let (temp_name, mut temp_file) = create_temp_file(parent.as_raw_fd(), &self.file_name)?;
        let temp_identity = identity(&file_stat(temp_file.as_raw_fd())?);
        let write_result = (|| {
            set_mode(temp_file.as_raw_fd(), self.mode)?;
            temp_file
                .write_all(content.as_bytes())
                .map_err(|error| error.to_string())?;
            temp_file.sync_all().map_err(|error| error.to_string())
        })();
        drop(temp_file);
        if let Err(error) = write_result {
            let _ = unlink_at(parent.as_raw_fd(), &temp_name);
            return Err(error.into());
        }

        // This is the final validation before the single atomic replacement operation.
        if let Err(error) = validate_file_at(
            parent.as_raw_fd(),
            &self.file_name,
            expected_identity,
            expected_hash,
        ) {
            let _ = unlink_at(parent.as_raw_fd(), &temp_name);
            return Err(error.into());
        }
        if let Err(error) = rename_exchange(
            parent.as_raw_fd(),
            &temp_name,
            parent.as_raw_fd(),
            &self.file_name,
        ) {
            let _ = unlink_at(parent.as_raw_fd(), &temp_name);
            return Err(error.into());
        }

        let displaced = validate_file_at(
            parent.as_raw_fd(),
            &temp_name,
            expected_identity,
            expected_hash,
        );
        let installed_hash = content_hash(content.as_bytes());
        let installed = validate_file_at(
            parent.as_raw_fd(),
            &self.file_name,
            temp_identity,
            &installed_hash,
        );
        if let Err(error) = installed {
            return Err(SecureReplaceError::InstalledChanged {
                message: format!(
                    "installed workspace edit target changed during verification: {error}"
                ),
                installed_identity: temp_identity,
            });
        }
        let ancestors = self.open_parent().and_then(|current| {
            validate_file_at(
                current.as_raw_fd(),
                &self.file_name,
                temp_identity,
                &installed_hash,
            )
        });
        if let Err(error) = displaced.and(ancestors) {
            let recovery = self.restore_exchange(
                parent.as_raw_fd(),
                &temp_name,
                temp_identity,
                &installed_hash,
            );
            return match recovery {
                Ok(()) => Err(SecureReplaceError::Safe(format!(
                    "workspace edit target changed during atomic replacement: {error}"
                ))),
                Err(recovery) => Err(SecureReplaceError::InstalledChanged {
                    message: format!(
                        "workspace edit target changed during atomic replacement: {error}; atomic replacement recovery failed: {recovery}"
                    ),
                    installed_identity: temp_identity,
                }),
            };
        }

        // The target is committed once the verified exchange succeeds. Cleanup/directory fsync
        // cannot safely turn that success into an apparent uncommitted state.
        let _ = unlink_at(parent.as_raw_fd(), &temp_name);
        let _ = sync_directory(&parent);
        Ok(temp_identity)
    }

    fn open_parent(&self) -> Result<OwnedFd, String> {
        let components = self
            .directories
            .iter()
            .skip(1)
            .map(|directory| {
                directory
                    .name
                    .clone()
                    .ok_or_else(|| "workspace edit directory identity is invalid".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let expected = self
            .directories
            .iter()
            .map(|directory| directory.identity)
            .collect::<Vec<_>>();
        open_directory_chain(&components, Some(&expected)).map(|(parent, _)| parent)
    }

    fn restore_exchange(
        &self,
        parent: RawFd,
        temp_name: &OsStr,
        installed_identity: FileIdentity,
        installed_hash: &str,
    ) -> Result<(), String> {
        validate_file_at(parent, &self.file_name, installed_identity, installed_hash)?;
        rename_exchange(parent, temp_name, parent, &self.file_name)?;
        unlink_at(parent, temp_name)
    }
}

pub(super) fn random_token() -> Result<String, String> {
    let mut bytes = [0_u8; 32];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|error| format!("could not obtain secure randomness: {error}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn canonical_components(path: &Path) -> Result<Vec<OsString>, String> {
    path.components()
        .filter_map(|component| match component {
            Component::RootDir => None,
            Component::Normal(component) => Some(Ok(component.to_os_string())),
            _ => Some(Err(
                "canonical workspace root contains an invalid component".to_owned(),
            )),
        })
        .collect()
}

fn parent_components(path: &Path) -> Result<Vec<OsString>, String> {
    path.parent()
        .into_iter()
        .flat_map(Path::components)
        .map(|component| match component {
            Component::Normal(component) => Ok(component.to_os_string()),
            _ => Err("workspace edit relative path contains an invalid component".to_owned()),
        })
        .collect()
}

fn open_directory_chain(
    components: &[OsString],
    expected: Option<&[FileIdentity]>,
) -> Result<(OwnedFd, Vec<DirectoryIdentity>), String> {
    let mut current = open_path(
        Path::new("/"),
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
    )?;
    let root_identity = identity(&file_stat(current.as_raw_fd())?);
    if expected.is_some_and(|expected| expected.first() != Some(&root_identity)) {
        return Err("filesystem root changed while applying workspace edit".to_owned());
    }
    let mut directories = vec![DirectoryIdentity {
        name: None,
        identity: root_identity,
    }];
    for (index, component) in components.iter().enumerate() {
        let next = open_file_at(
            current.as_raw_fd(),
            component,
            libc::O_RDONLY | libc::O_DIRECTORY,
        )?;
        let stat = file_stat(next.as_raw_fd())?;
        if (stat.st_mode & libc::S_IFMT) != libc::S_IFDIR {
            return Err(format!(
                "workspace edit ancestor {:?} is not a directory",
                component
            ));
        }
        let current_identity = identity(&stat);
        if expected.is_some_and(|expected| expected.get(index + 1) != Some(&current_identity)) {
            return Err(format!(
                "workspace edit ancestor {:?} was replaced",
                component
            ));
        }
        directories.push(DirectoryIdentity {
            name: Some(component.clone()),
            identity: current_identity,
        });
        current = next;
    }
    if expected.is_some_and(|expected| expected.len() != directories.len()) {
        return Err("workspace edit ancestor chain changed".to_owned());
    }
    Ok((current, directories))
}

fn open_path(path: &Path, flags: libc::c_int) -> Result<OwnedFd, String> {
    let path = c_string(path.as_os_str())?;
    let descriptor = unsafe { libc::open(path.as_ptr(), flags | libc::O_CLOEXEC) };
    owned_fd(descriptor)
}

fn open_file_at(parent: RawFd, name: &OsStr, flags: libc::c_int) -> Result<OwnedFd, String> {
    let name = c_string(name)?;
    let descriptor = unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    owned_fd(descriptor)
}

fn create_temp_file(parent: RawFd, file_name: &OsStr) -> Result<(OsString, File), String> {
    for _ in 0..100 {
        let id = NEXT_TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed);
        let name = OsString::from(format!(
            ".{}.kosmos-workspace-edit-{}-{id}",
            file_name.to_string_lossy(),
            std::process::id()
        ));
        let name_c = c_string(&name)?;
        let descriptor = unsafe {
            libc::openat(
                parent,
                name_c.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if descriptor >= 0 {
            return Ok((name, unsafe { File::from_raw_fd(descriptor) }));
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::AlreadyExists {
            return Err(error.to_string());
        }
    }
    Err("could not allocate a workspace edit temporary file".to_owned())
}

fn validate_file_at(
    parent: RawFd,
    name: &OsStr,
    expected_identity: FileIdentity,
    expected_hash: &str,
) -> Result<(), String> {
    let file = open_file_at(parent, name, libc::O_RDONLY)?;
    let stat = file_stat(file.as_raw_fd())?;
    ensure_regular_file(&stat)?;
    if identity(&stat) != expected_identity {
        return Err("workspace edit target was replaced".to_owned());
    }
    let content = read_bytes(file)?;
    if content_hash(&content) != expected_hash {
        return Err("workspace edit target content changed".to_owned());
    }
    Ok(())
}

fn read_text(file: OwnedFd) -> Result<String, String> {
    let bytes = read_bytes(file)?;
    String::from_utf8(bytes).map_err(|_| "workspace edit target is not valid UTF-8".to_owned())
}

fn read_bytes(file: OwnedFd) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::from(file)
        .take((MAX_EDITOR_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_EDITOR_FILE_BYTES {
        return Err(format!(
            "workspace edit target exceeds the {MAX_EDITOR_FILE_BYTES}-byte limit"
        ));
    }
    Ok(bytes)
}

fn file_stat(descriptor: RawFd) -> Result<libc::stat, String> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(descriptor, stat.as_mut_ptr()) } == -1 {
        return Err(io::Error::last_os_error().to_string());
    }
    Ok(unsafe { stat.assume_init() })
}

fn identity(stat: &libc::stat) -> FileIdentity {
    FileIdentity {
        device: stat.st_dev,
        inode: stat.st_ino,
    }
}

fn ensure_regular_file(stat: &libc::stat) -> Result<(), String> {
    if (stat.st_mode & libc::S_IFMT) == libc::S_IFREG {
        Ok(())
    } else {
        Err("workspace edit target is not a regular file".to_owned())
    }
}

fn set_mode(descriptor: RawFd, mode: libc::mode_t) -> Result<(), String> {
    if unsafe { libc::fchmod(descriptor, mode) } == -1 {
        Err(io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}

fn rename_exchange(
    old_parent: RawFd,
    old_name: &OsStr,
    new_parent: RawFd,
    new_name: &OsStr,
) -> Result<(), String> {
    let old_name = c_string(old_name)?;
    let new_name = c_string(new_name)?;
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            old_parent,
            old_name.as_ptr(),
            new_parent,
            new_name.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    };
    if result == -1 {
        Err(io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}

fn unlink_at(parent: RawFd, name: &OsStr) -> Result<(), String> {
    let name = c_string(name)?;
    if unsafe { libc::unlinkat(parent, name.as_ptr(), 0) } == -1 {
        Err(io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}

fn sync_directory(directory: &OwnedFd) -> Result<(), String> {
    if unsafe { libc::fsync(directory.as_raw_fd()) } == -1 {
        Err(io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}

fn c_string(value: &OsStr) -> Result<CString, String> {
    CString::new(value.as_bytes()).map_err(|_| "filesystem path contains a null byte".to_owned())
}

fn owned_fd(descriptor: libc::c_int) -> Result<OwnedFd, String> {
    if descriptor == -1 {
        Err(io::Error::last_os_error().to_string())
    } else {
        Ok(unsafe { OwnedFd::from_raw_fd(descriptor) })
    }
}

fn content_hash(content: &[u8]) -> String {
    Sha256::digest(content)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
