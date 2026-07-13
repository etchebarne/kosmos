use std::ffi::{CString, OsStr, OsString};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::tabs::editor::MAX_EDITOR_FILE_BYTES;

#[cfg(test)]
use std::cell::Cell;

const MAX_RESOURCE_SNAPSHOT_ENTRIES: usize = 4_096;
const MAX_RESOURCE_SNAPSHOT_BYTES: u64 = 64 * 1024 * 1024;
pub(super) const TRANSACTION_PATH_PREFIX: &str = ".kosmos-workspace-edit-";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SecureEditFault {
    PrepareBeforeIdentity,
    Prepare,
    InstallPrepared,
    Rename,
    StageRemove,
    RecursiveDiscard,
}

pub(super) fn prepared_file_name(operation: usize) -> String {
    format!("prepared-{operation}")
}

#[cfg(test)]
thread_local! {
    static NEXT_FAULT: Cell<Option<SecureEditFault>> = const { Cell::new(None) };
}

#[cfg(test)]
pub(super) fn fail_next_secure_edit_at(fault: SecureEditFault) {
    NEXT_FAULT.set(Some(fault));
}

fn inject_fault(fault: SecureEditFault) -> Result<(), String> {
    #[cfg(test)]
    if NEXT_FAULT.get() == Some(fault) {
        NEXT_FAULT.set(None);
        return Err(format!("injected {fault:?} post-syscall failure"));
    }
    let _ = fault;
    Ok(())
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct FileIdentity {
    pub(super) device: u64,
    pub(super) inode: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) enum SecurePathKind {
    File,
    Directory,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct SecurePathSnapshot {
    pub(super) identity: FileIdentity,
    pub(super) kind: SecurePathKind,
    pub(super) mode: libc::mode_t,
    pub(super) content: Option<String>,
    pub(super) fingerprint: String,
}

#[derive(Debug)]
pub(super) struct SecureTombstone {
    original_parent: String,
    recovery_name: OsString,
    backup_name: OsString,
    identity: FileIdentity,
    kind: SecurePathKind,
    fingerprint: String,
}

#[derive(Debug)]
pub(super) enum SecureMutationOutcome<T> {
    NotApplied,
    Applied(Box<T>),
    Unknown,
}

#[derive(Debug)]
pub(super) struct SecureMutationError<T> {
    pub(super) message: String,
    pub(super) outcome: SecureMutationOutcome<T>,
}

impl<T> From<String> for SecureMutationError<T> {
    fn from(message: String) -> Self {
        Self {
            message,
            outcome: SecureMutationOutcome::NotApplied,
        }
    }
}

#[derive(Debug)]
pub(super) struct SecureWorkspace {
    root: OwnedFd,
}

impl SecureWorkspace {
    pub(super) fn open(canonical_root: &Path) -> Result<Self, String> {
        if !canonical_root.is_absolute() {
            return Err("workspace root is not absolute".to_owned());
        }
        let root = open_path(
            canonical_root,
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        )?;
        Ok(Self { root })
    }

    pub(super) fn snapshot(&self, path: &str) -> Result<Option<SecurePathSnapshot>, String> {
        let (parent, name, _) = self.open_parent(path)?;
        let Some(stat) = stat_at_optional(parent.as_raw_fd(), &name)? else {
            return Ok(None);
        };
        let kind = path_kind(&stat)?;
        let (content, fingerprint) = if kind == SecurePathKind::File {
            let bytes = read_bytes(open_file_at(parent.as_raw_fd(), &name, libc::O_RDONLY)?)?;
            let content = String::from_utf8(bytes.clone())
                .map_err(|_| "workspace edit target is not valid UTF-8".to_owned())?;
            (Some(content), path_fingerprint(&stat, Some(&bytes), None)?)
        } else {
            let directory = open_file_at(
                parent.as_raw_fd(),
                &name,
                libc::O_RDONLY | libc::O_DIRECTORY,
            )?;
            (None, directory_fingerprint(&stat, directory.as_raw_fd())?)
        };
        Ok(Some(SecurePathSnapshot {
            identity: identity(&stat),
            kind,
            mode: stat.st_mode & 0o7777,
            content,
            fingerprint,
        }))
    }

    pub(super) fn validate_snapshot(
        &self,
        path: &str,
        expected: &SecurePathSnapshot,
    ) -> Result<(), String> {
        let current = self
            .snapshot(path)?
            .ok_or_else(|| "workspace edit target no longer exists".to_owned())?;
        if current.identity != expected.identity {
            return Err("workspace edit target was replaced".to_owned());
        }
        if current.kind != expected.kind || current.fingerprint != expected.fingerprint {
            return Err("workspace edit target changed after validation".to_owned());
        }
        Ok(())
    }

    pub(super) fn directory_empty(&self, path: &str) -> Result<bool, String> {
        let (parent, name, _) = self.open_parent(path)?;
        let directory = open_file_at(
            parent.as_raw_fd(),
            &name,
            libc::O_RDONLY | libc::O_DIRECTORY,
        )?;
        directory_is_empty(directory.as_raw_fd())
    }

    pub(super) fn prepare_file(
        &self,
        token: &str,
        operation: usize,
        mode: libc::mode_t,
        content: &str,
    ) -> Result<FileIdentity, SecureMutationError<FileIdentity>> {
        let recovery_name = OsString::from(format!("{TRANSACTION_PATH_PREFIX}{token}"));
        let recovery = ensure_directory_at(self.root.as_raw_fd(), &recovery_name, 0o700)?;
        let name = OsString::from(prepared_file_name(operation));
        let encoded = c_string(&name)?;
        let descriptor = unsafe {
            libc::openat(
                recovery.as_raw_fd(),
                encoded.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                mode,
            )
        };
        let file = owned_fd(descriptor)?;
        let stat = match descriptor_stat(file.as_raw_fd()) {
            Ok(stat) => stat,
            Err(message) => {
                let cleanup =
                    unlink_at(recovery.as_raw_fd(), &name).and_then(|()| sync_directory(&recovery));
                return Err(SecureMutationError {
                    message,
                    outcome: if cleanup.is_ok() {
                        SecureMutationOutcome::NotApplied
                    } else {
                        SecureMutationOutcome::Unknown
                    },
                });
            }
        };
        let prepared_identity = identity(&stat);
        let mut file = File::from(file);
        let write_result = file
            .write_all(content.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|error| error.to_string());
        if let Err(error) = write_result {
            drop(file);
            let cleanup =
                unlink_at(recovery.as_raw_fd(), &name).and_then(|()| sync_directory(&recovery));
            return Err(match cleanup {
                Ok(()) => SecureMutationError {
                    message: error,
                    outcome: SecureMutationOutcome::NotApplied,
                },
                Err(cleanup_error) => SecureMutationError {
                    message: format!(
                        "{error}; prepared-file cleanup could not be confirmed: {}",
                        cleanup_error
                    ),
                    outcome: SecureMutationOutcome::Applied(Box::new(prepared_identity)),
                },
            });
        }
        let durable_entry = sync_directory(&recovery)
            .and_then(|()| self.duplicate_root())
            .and_then(|root| sync_directory(&root));
        if let Err(message) = durable_entry {
            return Err(SecureMutationError {
                message,
                outcome: SecureMutationOutcome::Applied(Box::new(prepared_identity)),
            });
        }
        if let Err(message) = inject_fault(SecureEditFault::PrepareBeforeIdentity) {
            return Err(SecureMutationError {
                message,
                outcome: SecureMutationOutcome::Unknown,
            });
        }
        let completion = inject_fault(SecureEditFault::Prepare)
            .and_then(|()| file_stat_at(recovery.as_raw_fd(), encoded.as_ptr()))
            .and_then(|stat| {
                if identity(&stat) == prepared_identity {
                    Ok(())
                } else {
                    Err("workspace edit prepared file was replaced".to_owned())
                }
            });
        if let Err(message) = completion {
            return Err(SecureMutationError {
                message,
                outcome: SecureMutationOutcome::Applied(Box::new(prepared_identity)),
            });
        }
        Ok(prepared_identity)
    }

    pub(super) fn install_prepared_file(
        &self,
        path: &str,
        token: &str,
        operation: usize,
        expected: FileIdentity,
    ) -> Result<FileIdentity, SecureMutationError<FileIdentity>> {
        let recovery_name = OsString::from(format!("{TRANSACTION_PATH_PREFIX}{token}"));
        let recovery = open_file_at(
            self.root.as_raw_fd(),
            &recovery_name,
            libc::O_RDONLY | libc::O_DIRECTORY,
        )?;
        let prepared_name = OsString::from(prepared_file_name(operation));
        validate_identity_at(recovery.as_raw_fd(), &prepared_name, expected)?;
        let (parent, name, _) = self.open_parent(path)?;
        ensure_missing_at(parent.as_raw_fd(), &name)?;
        rename_noreplace(
            recovery.as_raw_fd(),
            &prepared_name,
            parent.as_raw_fd(),
            &name,
        )
        .map_err(SecureMutationError::from)?;
        if let Err(message) = inject_fault(SecureEditFault::InstallPrepared)
            .and_then(|()| validate_identity_at(parent.as_raw_fd(), &name, expected).map(|_| ()))
            .and_then(|_| sync_directory(&recovery))
            .and_then(|()| sync_directory(&parent))
        {
            let outcome = match exact_rename_observed(
                recovery.as_raw_fd(),
                &prepared_name,
                parent.as_raw_fd(),
                &name,
                expected,
            ) {
                Ok(true) => SecureMutationOutcome::Applied(Box::new(expected)),
                Ok(false) | Err(_) => SecureMutationOutcome::Unknown,
            };
            return Err(SecureMutationError { message, outcome });
        }
        Ok(expected)
    }

    pub(super) fn remove_prepared_file_if_matches(
        &self,
        token: &str,
        operation: usize,
        expected: FileIdentity,
    ) -> Result<(), String> {
        let recovery_name = OsString::from(format!("{TRANSACTION_PATH_PREFIX}{token}"));
        let Some(recovery_stat) = stat_at_optional(self.root.as_raw_fd(), &recovery_name)? else {
            return Ok(());
        };
        if path_kind(&recovery_stat)? != SecurePathKind::Directory {
            return Err("workspace edit recovery path is not a directory".to_owned());
        }
        let recovery = open_file_at(
            self.root.as_raw_fd(),
            &recovery_name,
            libc::O_RDONLY | libc::O_DIRECTORY,
        )?;
        let name = OsString::from(prepared_file_name(operation));
        let Some(stat) = stat_at_optional(recovery.as_raw_fd(), &name)? else {
            return Ok(());
        };
        if identity(&stat) != expected || path_kind(&stat)? != SecurePathKind::File {
            return Err("workspace edit prepared file was replaced".to_owned());
        }
        unlink_at(recovery.as_raw_fd(), &name)?;
        sync_directory(&recovery)
    }

    pub(super) fn remove_prepared_file_without_identity(
        &self,
        token: &str,
        operation: usize,
        recorded_name: &str,
        expected_hash: &str,
    ) -> Result<(), String> {
        let expected_name = prepared_file_name(operation);
        if recorded_name != expected_name {
            return Err("workspace edit prepared filename is invalid".to_owned());
        }
        let recovery_name = OsString::from(format!("{TRANSACTION_PATH_PREFIX}{token}"));
        let Some(recovery_stat) = stat_at_optional(self.root.as_raw_fd(), &recovery_name)? else {
            return Ok(());
        };
        if path_kind(&recovery_stat)? != SecurePathKind::Directory
            || recovery_stat.st_uid != unsafe { libc::geteuid() }
            || recovery_stat.st_mode & 0o077 != 0
        {
            return Err("workspace edit recovery directory ownership is invalid".to_owned());
        }
        let recovery = open_file_at(
            self.root.as_raw_fd(),
            &recovery_name,
            libc::O_RDONLY | libc::O_DIRECTORY,
        )?;
        let name = OsString::from(recorded_name);
        let Some(stat) = stat_at_optional(recovery.as_raw_fd(), &name)? else {
            return Ok(());
        };
        if path_kind(&stat)? != SecurePathKind::File
            || stat.st_uid != recovery_stat.st_uid
            || stat.st_nlink != 1
        {
            return Err("workspace edit prepared file ownership is invalid".to_owned());
        }
        let bytes = read_bytes(open_file_at(recovery.as_raw_fd(), &name, libc::O_RDONLY)?)?;
        let actual_hash = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        if actual_hash != expected_hash {
            return Err("workspace edit prepared file content changed before recovery".to_owned());
        }
        unlink_at(recovery.as_raw_fd(), &name)?;
        sync_directory(&recovery)
    }

    pub(super) fn remove_created(&self, path: &str, expected: FileIdentity) -> Result<(), String> {
        let (parent, name, _) = self.open_parent(path)?;
        validate_identity_at(parent.as_raw_fd(), &name, expected)?;
        unlink_at(parent.as_raw_fd(), &name)?;
        sync_directory(&parent)
    }

    pub(super) fn rename(
        &self,
        source: &str,
        destination: &str,
        expected: FileIdentity,
    ) -> Result<FileIdentity, SecureMutationError<FileIdentity>> {
        let (source_parent, source_name, _) = self.open_parent(source)?;
        let (destination_parent, destination_name, _) = self.open_parent(destination)?;
        validate_identity_at(source_parent.as_raw_fd(), &source_name, expected)?;
        ensure_missing_at(destination_parent.as_raw_fd(), &destination_name)?;
        rename_noreplace(
            source_parent.as_raw_fd(),
            &source_name,
            destination_parent.as_raw_fd(),
            &destination_name,
        )
        .map_err(SecureMutationError::from)?;
        let completion = inject_fault(SecureEditFault::Rename)
            .and_then(|()| {
                validate_identity_at(destination_parent.as_raw_fd(), &destination_name, expected)
                    .map(|_| ())
            })
            .and_then(|()| sync_directory(&source_parent))
            .and_then(|()| {
                if source_parent.as_raw_fd() != destination_parent.as_raw_fd() {
                    sync_directory(&destination_parent)
                } else {
                    Ok(())
                }
            });
        if let Err(message) = completion {
            let outcome = match exact_rename_observed(
                source_parent.as_raw_fd(),
                &source_name,
                destination_parent.as_raw_fd(),
                &destination_name,
                expected,
            ) {
                Ok(true) => SecureMutationOutcome::Applied(Box::new(expected)),
                Ok(false) | Err(_) => SecureMutationOutcome::Unknown,
            };
            return Err(SecureMutationError { message, outcome });
        }
        Ok(expected)
    }

    pub(super) fn stage_remove(
        &self,
        path: &str,
        expected: &SecurePathSnapshot,
        token: &str,
        operation: usize,
    ) -> Result<SecureTombstone, SecureMutationError<SecureTombstone>> {
        let (parent, name, parent_path) = self.open_parent(path)?;
        self.validate_snapshot(path, expected)?;
        let stat = validate_identity_at(parent.as_raw_fd(), &name, expected.identity)?;
        let kind = path_kind(&stat)?;
        let recovery_name = OsString::from(format!("{TRANSACTION_PATH_PREFIX}{token}"));
        let recovery = ensure_directory_at(self.root.as_raw_fd(), &recovery_name, 0o700)?;
        let tombstone_name = OsString::from(format!("backup-{operation}"));
        ensure_missing_at(recovery.as_raw_fd(), &tombstone_name)?;
        let tombstone = SecureTombstone {
            original_parent: parent_path,
            recovery_name,
            backup_name: tombstone_name,
            identity: expected.identity,
            kind,
            fingerprint: expected.fingerprint.clone(),
        };
        rename_noreplace(
            parent.as_raw_fd(),
            &name,
            recovery.as_raw_fd(),
            &tombstone.backup_name,
        )
        .map_err(SecureMutationError::from)?;
        let completion = inject_fault(SecureEditFault::StageRemove)
            .and_then(|()| {
                validate_identity_at(
                    recovery.as_raw_fd(),
                    &tombstone.backup_name,
                    expected.identity,
                )
                .map(|_| ())
            })
            .and_then(|()| sync_directory(&parent))
            .and_then(|()| sync_directory(&recovery))
            .and_then(|()| self.duplicate_root())
            .and_then(|root| sync_directory(&root));
        if let Err(message) = completion {
            let outcome = match exact_rename_observed(
                parent.as_raw_fd(),
                &name,
                recovery.as_raw_fd(),
                &tombstone.backup_name,
                expected.identity,
            ) {
                Ok(true) => SecureMutationOutcome::Applied(Box::new(tombstone)),
                Ok(false) | Err(_) => SecureMutationOutcome::Unknown,
            };
            return Err(SecureMutationError { message, outcome });
        }
        Ok(tombstone)
    }

    pub(super) fn restore(&self, path: &str, tombstone: &SecureTombstone) -> Result<(), String> {
        let (target_parent, target_name, target_parent_path) = self.open_parent(path)?;
        if target_parent_path != tombstone.original_parent {
            return Err("workspace edit tombstone parent changed".to_owned());
        }
        let recovery = self.open_recovery_directory_from_tombstone(tombstone)?;
        validate_tombstone_at(recovery.as_raw_fd(), tombstone)?;
        ensure_missing_at(target_parent.as_raw_fd(), &target_name)?;
        rename_noreplace(
            recovery.as_raw_fd(),
            &tombstone.backup_name,
            target_parent.as_raw_fd(),
            &target_name,
        )?;
        validate_identity_at(target_parent.as_raw_fd(), &target_name, tombstone.identity)?;
        sync_directory(&target_parent)?;
        sync_directory(&recovery)
    }

    pub(super) fn discard(
        &self,
        tombstone: &SecureTombstone,
        recursive: bool,
    ) -> Result<(), String> {
        let parent = self.open_recovery_directory_from_tombstone(tombstone)?;
        validate_tombstone_at(parent.as_raw_fd(), tombstone)?;
        self.discard_validated(tombstone, recursive)
    }

    fn discard_validated(
        &self,
        tombstone: &SecureTombstone,
        recursive: bool,
    ) -> Result<(), String> {
        let Some(recovery_stat) =
            stat_at_optional(self.root.as_raw_fd(), &tombstone.recovery_name)?
        else {
            return Ok(());
        };
        if path_kind(&recovery_stat)? != SecurePathKind::Directory {
            return Err("workspace edit recovery path is not a directory".to_owned());
        }
        let parent = self.open_recovery_directory_from_tombstone(tombstone)?;
        let Some(stat) = stat_at_optional(parent.as_raw_fd(), &tombstone.backup_name)? else {
            return Ok(());
        };
        if identity(&stat) != tombstone.identity || path_kind(&stat)? != tombstone.kind {
            return Err("workspace edit tombstone identity changed during cleanup".to_owned());
        }
        match tombstone.kind {
            SecurePathKind::File => unlink_at(parent.as_raw_fd(), &tombstone.backup_name)?,
            SecurePathKind::Directory if recursive => {
                let directory = open_file_at(
                    parent.as_raw_fd(),
                    &tombstone.backup_name,
                    libc::O_RDONLY | libc::O_DIRECTORY,
                )?;
                remove_directory_contents(directory.as_raw_fd())?;
                unlink_directory_at(parent.as_raw_fd(), &tombstone.backup_name)?;
            }
            SecurePathKind::Directory => {
                unlink_directory_at(parent.as_raw_fd(), &tombstone.backup_name)?
            }
        }
        sync_directory(&parent)
    }

    pub(super) fn remove_recovery_directory(&self, token: &str) -> Result<(), String> {
        let name = OsString::from(format!("{TRANSACTION_PATH_PREFIX}{token}"));
        let Some(stat) = stat_at_optional(self.root.as_raw_fd(), &name)? else {
            return Ok(());
        };
        if path_kind(&stat)? != SecurePathKind::Directory {
            return Err("workspace edit recovery path is not a directory".to_owned());
        }
        let directory = open_file_at(
            self.root.as_raw_fd(),
            &name,
            libc::O_RDONLY | libc::O_DIRECTORY,
        )?;
        if !directory_is_empty(directory.as_raw_fd())? {
            return Err("workspace edit recovery directory is not empty".to_owned());
        }
        unlink_directory_at(self.root.as_raw_fd(), &name)?;
        let root = self.duplicate_root()?;
        sync_directory(&root)
    }

    pub(super) fn restore_recovery_backup(
        &self,
        path: &str,
        token: &str,
        operation: usize,
        expected: &SecurePathSnapshot,
    ) -> Result<(), String> {
        let tombstone = recovery_tombstone(path, token, operation, expected)?;
        if self.recovery_backup_exists(&tombstone, true)? {
            if self.snapshot(path)?.is_some() {
                return Err("workspace edit recovery target is occupied".to_owned());
            }
            return self.restore(path, &tombstone);
        }
        let current = self.snapshot(path)?.ok_or_else(|| {
            "workspace edit backup and restored target are both missing".to_owned()
        })?;
        if snapshot_matches(expected, &current) {
            Ok(())
        } else {
            Err("workspace edit restored target changed".to_owned())
        }
    }

    pub(super) fn discard_recovery_backup(
        &self,
        path: &str,
        token: &str,
        operation: usize,
        expected: &SecurePathSnapshot,
        recursive: bool,
    ) -> Result<(), String> {
        let tombstone = recovery_tombstone(path, token, operation, expected)?;
        if !self.recovery_backup_exists(&tombstone, false)? {
            return Ok(());
        }
        self.discard_validated(&tombstone, recursive)
    }

    pub(super) fn validate_recovery_backup(
        &self,
        path: &str,
        token: &str,
        operation: usize,
        expected: &SecurePathSnapshot,
    ) -> Result<(), String> {
        let tombstone = recovery_tombstone(path, token, operation, expected)?;
        if self.recovery_backup_exists(&tombstone, true)? {
            Ok(())
        } else {
            Err("workspace edit recovery backup disappeared before cleanup".to_owned())
        }
    }

    fn open_parent(&self, path: &str) -> Result<(OwnedFd, OsString, String), String> {
        let relative = Path::new(path);
        let file_name = relative
            .file_name()
            .ok_or_else(|| "workspace edit target has no file name".to_owned())?
            .to_os_string();
        let parent = relative.parent().unwrap_or_else(|| Path::new(""));
        let parent_path = parent
            .to_str()
            .ok_or_else(|| "workspace edit path is not valid UTF-8".to_owned())?
            .to_owned();
        Ok((self.open_directory(&parent_path)?, file_name, parent_path))
    }

    fn open_directory(&self, path: &str) -> Result<OwnedFd, String> {
        let mut current = self.duplicate_root()?;
        if path.is_empty() {
            return Ok(current);
        }
        for component in Path::new(path).components() {
            let Component::Normal(component) = component else {
                return Err("workspace edit relative path contains an invalid component".to_owned());
            };
            current = open_file_at(
                current.as_raw_fd(),
                component,
                libc::O_RDONLY | libc::O_DIRECTORY,
            )?;
        }
        Ok(current)
    }

    fn duplicate_root(&self) -> Result<OwnedFd, String> {
        owned_fd(unsafe { libc::fcntl(self.root.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) })
    }

    fn open_recovery_directory_from_tombstone(
        &self,
        tombstone: &SecureTombstone,
    ) -> Result<OwnedFd, String> {
        open_file_at(
            self.root.as_raw_fd(),
            &tombstone.recovery_name,
            libc::O_RDONLY | libc::O_DIRECTORY,
        )
    }

    fn recovery_backup_exists(
        &self,
        tombstone: &SecureTombstone,
        validate_fingerprint: bool,
    ) -> Result<bool, String> {
        let Some(recovery_stat) =
            stat_at_optional(self.root.as_raw_fd(), &tombstone.recovery_name)?
        else {
            return Ok(false);
        };
        if path_kind(&recovery_stat)? != SecurePathKind::Directory {
            return Err("workspace edit recovery path is not a directory".to_owned());
        }
        let recovery = self.open_recovery_directory_from_tombstone(tombstone)?;
        if stat_at_optional(recovery.as_raw_fd(), &tombstone.backup_name)?.is_none() {
            return Ok(false);
        }
        if validate_fingerprint {
            validate_tombstone_at(recovery.as_raw_fd(), tombstone)?;
        } else {
            let stat = validate_identity_at(
                recovery.as_raw_fd(),
                &tombstone.backup_name,
                tombstone.identity,
            )?;
            if path_kind(&stat)? != tombstone.kind {
                return Err("workspace edit tombstone kind changed".to_owned());
            }
        }
        Ok(true)
    }
}

fn recovery_tombstone(
    path: &str,
    token: &str,
    operation: usize,
    expected: &SecurePathSnapshot,
) -> Result<SecureTombstone, String> {
    let parent = Path::new(path)
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .to_str()
        .ok_or_else(|| "workspace edit path is not valid UTF-8".to_owned())?
        .to_owned();
    Ok(SecureTombstone {
        original_parent: parent,
        recovery_name: OsString::from(format!("{TRANSACTION_PATH_PREFIX}{token}")),
        backup_name: OsString::from(format!("backup-{operation}")),
        identity: expected.identity,
        kind: expected.kind,
        fingerprint: expected.fingerprint.clone(),
    })
}

fn snapshot_matches(expected: &SecurePathSnapshot, current: &SecurePathSnapshot) -> bool {
    expected.identity == current.identity
        && expected.kind == current.kind
        && expected.fingerprint == current.fingerprint
}

pub(super) fn random_token() -> Result<String, String> {
    let mut bytes = [0_u8; 32];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|error| format!("could not obtain secure randomness: {error}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
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

fn ensure_directory_at(parent: RawFd, name: &OsStr, mode: libc::mode_t) -> Result<OwnedFd, String> {
    let encoded = c_string(name)?;
    if unsafe { libc::mkdirat(parent, encoded.as_ptr(), mode) } == -1 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::AlreadyExists {
            return Err(error.to_string());
        }
    }
    open_file_at(parent, name, libc::O_RDONLY | libc::O_DIRECTORY)
}

fn path_fingerprint(
    stat: &libc::stat,
    bytes: Option<&[u8]>,
    directory_hash: Option<&[u8]>,
) -> Result<String, String> {
    let mut hash = Sha256::new();
    hash.update(stat.st_dev.to_le_bytes());
    hash.update(stat.st_ino.to_le_bytes());
    hash.update(stat.st_mode.to_le_bytes());
    if directory_hash.is_none() {
        hash.update(stat.st_size.to_le_bytes());
        hash.update(stat.st_mtime.to_le_bytes());
        hash.update(stat.st_mtime_nsec.to_le_bytes());
    }
    if let Some(bytes) = bytes {
        hash.update(Sha256::digest(bytes));
    }
    if let Some(directory_hash) = directory_hash {
        hash.update(directory_hash);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn directory_fingerprint(stat: &libc::stat, directory: RawFd) -> Result<String, String> {
    let mut entries = 0_usize;
    let mut bytes = 0_u64;
    let digest = directory_digest(directory, &mut entries, &mut bytes)?;
    path_fingerprint(stat, None, Some(&digest))
}

fn directory_digest(
    directory: RawFd,
    entries: &mut usize,
    bytes: &mut u64,
) -> Result<Vec<u8>, String> {
    let mut hash = Sha256::new();
    for name in read_directory_names(directory)? {
        *entries += 1;
        if *entries > MAX_RESOURCE_SNAPSHOT_ENTRIES {
            return Err(format!(
                "workspace edit directory exceeds the {MAX_RESOURCE_SNAPSHOT_ENTRIES}-entry snapshot limit"
            ));
        }
        let stat = stat_at_optional(directory, &name)?
            .ok_or_else(|| "workspace edit directory entry disappeared".to_owned())?;
        let kind = path_kind(&stat)?;
        hash.update(name.as_os_str().as_bytes());
        hash.update([0]);
        hash.update(stat.st_dev.to_le_bytes());
        hash.update(stat.st_ino.to_le_bytes());
        hash.update(stat.st_mode.to_le_bytes());
        if kind == SecurePathKind::File {
            hash.update(stat.st_size.to_le_bytes());
            hash.update(stat.st_mtime.to_le_bytes());
            hash.update(stat.st_mtime_nsec.to_le_bytes());
        }
        match kind {
            SecurePathKind::File => {
                let file = open_file_at(directory, &name, libc::O_RDONLY)?;
                let size = u64::try_from(stat.st_size)
                    .map_err(|_| "workspace edit file size is invalid".to_owned())?;
                *bytes = bytes.checked_add(size).ok_or_else(|| {
                    "workspace edit directory snapshot size overflowed".to_owned()
                })?;
                if *bytes > MAX_RESOURCE_SNAPSHOT_BYTES {
                    return Err(format!(
                        "workspace edit directory exceeds the {MAX_RESOURCE_SNAPSHOT_BYTES}-byte snapshot limit"
                    ));
                }
                hash.update(hash_file(file)?);
            }
            SecurePathKind::Directory => {
                let child = open_file_at(directory, &name, libc::O_RDONLY | libc::O_DIRECTORY)?;
                hash.update(directory_digest(child.as_raw_fd(), entries, bytes)?);
            }
        }
    }
    Ok(hash.finalize().to_vec())
}

fn read_directory_names(directory: RawFd) -> Result<Vec<OsString>, String> {
    let duplicate = owned_fd(unsafe { libc::fcntl(directory, libc::F_DUPFD_CLOEXEC, 0) })?;
    let stream = unsafe { libc::fdopendir(duplicate.as_raw_fd()) };
    if stream.is_null() {
        return Err(io::Error::last_os_error().to_string());
    }
    std::mem::forget(duplicate);
    let result = (|| {
        let mut names = Vec::new();
        loop {
            unsafe { *libc::__errno_location() = 0 };
            let entry = unsafe { libc::readdir(stream) };
            if entry.is_null() {
                let errno = unsafe { *libc::__errno_location() };
                if errno != 0 {
                    return Err(io::Error::from_raw_os_error(errno).to_string());
                }
                break;
            }
            let name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) };
            if name.to_bytes() != b"." && name.to_bytes() != b".." {
                names.push(OsStr::from_bytes(name.to_bytes()).to_os_string());
            }
        }
        names.sort_by(|left, right| {
            left.as_os_str()
                .as_bytes()
                .cmp(right.as_os_str().as_bytes())
        });
        Ok(names)
    })();
    unsafe { libc::closedir(stream) };
    result
}

fn hash_file(file: OwnedFd) -> Result<Vec<u8>, String> {
    let mut file = File::from(file);
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            return Ok(hash.finalize().to_vec());
        }
        hash.update(&buffer[..count]);
    }
}

fn stat_at_optional(parent: RawFd, name: &OsStr) -> Result<Option<libc::stat>, String> {
    let stat = stat_at_optional_raw(parent, name)?;
    if stat
        .as_ref()
        .is_some_and(|stat| (stat.st_mode & libc::S_IFMT) == libc::S_IFLNK)
    {
        return Err("workspace edit final symlinks are not supported".to_owned());
    }
    Ok(stat)
}

fn stat_at_optional_raw(parent: RawFd, name: &OsStr) -> Result<Option<libc::stat>, String> {
    let name = c_string(name)?;
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    let result = unsafe {
        libc::fstatat(
            parent,
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == 0 {
        let stat = unsafe { stat.assume_init() };
        return Ok(Some(stat));
    }
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::NotFound {
        Ok(None)
    } else {
        Err(error.to_string())
    }
}

fn file_stat_at(parent: RawFd, name: *const libc::c_char) -> Result<libc::stat, String> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstatat(parent, name, stat.as_mut_ptr(), libc::AT_SYMLINK_NOFOLLOW) } == -1 {
        return Err(io::Error::last_os_error().to_string());
    }
    Ok(unsafe { stat.assume_init() })
}

fn descriptor_stat(descriptor: RawFd) -> Result<libc::stat, String> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(descriptor, stat.as_mut_ptr()) } == -1 {
        return Err(io::Error::last_os_error().to_string());
    }
    Ok(unsafe { stat.assume_init() })
}

fn validate_identity_at(
    parent: RawFd,
    name: &OsStr,
    expected: FileIdentity,
) -> Result<libc::stat, String> {
    let stat = stat_at_optional(parent, name)?
        .ok_or_else(|| "workspace edit target no longer exists".to_owned())?;
    if identity(&stat) != expected {
        return Err("workspace edit target was replaced".to_owned());
    }
    Ok(stat)
}

fn validate_tombstone_at(parent: RawFd, tombstone: &SecureTombstone) -> Result<libc::stat, String> {
    let stat = validate_identity_at(parent, &tombstone.backup_name, tombstone.identity)?;
    if path_kind(&stat)? != tombstone.kind {
        return Err("workspace edit tombstone kind changed".to_owned());
    }
    let fingerprint = match tombstone.kind {
        SecurePathKind::File => {
            let bytes = read_bytes(open_file_at(
                parent,
                &tombstone.backup_name,
                libc::O_RDONLY,
            )?)?;
            path_fingerprint(&stat, Some(&bytes), None)?
        }
        SecurePathKind::Directory => {
            let directory = open_file_at(
                parent,
                &tombstone.backup_name,
                libc::O_RDONLY | libc::O_DIRECTORY,
            )?;
            directory_fingerprint(&stat, directory.as_raw_fd())?
        }
    };
    if fingerprint != tombstone.fingerprint {
        return Err("workspace edit tombstone changed before recovery or cleanup".to_owned());
    }
    Ok(stat)
}

fn ensure_missing_at(parent: RawFd, name: &OsStr) -> Result<(), String> {
    if stat_at_optional(parent, name)?.is_some() {
        Err("workspace edit destination already exists".to_owned())
    } else {
        Ok(())
    }
}

fn exact_rename_observed(
    source_parent: RawFd,
    source_name: &OsStr,
    destination_parent: RawFd,
    destination_name: &OsStr,
    expected: FileIdentity,
) -> Result<bool, String> {
    let source = stat_at_optional(source_parent, source_name)?;
    let destination = stat_at_optional(destination_parent, destination_name)?;
    Ok(source.is_none() && destination.is_some_and(|stat| identity(&stat) == expected))
}

fn path_kind(stat: &libc::stat) -> Result<SecurePathKind, String> {
    match stat.st_mode & libc::S_IFMT {
        libc::S_IFREG => Ok(SecurePathKind::File),
        libc::S_IFDIR => Ok(SecurePathKind::Directory),
        _ => Err("workspace edit target must be a regular file or directory".to_owned()),
    }
}

fn rename_noreplace(
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
            libc::RENAME_NOREPLACE,
        )
    };
    if result == -1 {
        Err(io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}

fn unlink_directory_at(parent: RawFd, name: &OsStr) -> Result<(), String> {
    let name = c_string(name)?;
    if unsafe { libc::unlinkat(parent, name.as_ptr(), libc::AT_REMOVEDIR) } == -1 {
        Err(io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}

fn remove_directory_contents(directory: RawFd) -> Result<(), String> {
    let duplicate = unsafe { libc::fcntl(directory, libc::F_DUPFD_CLOEXEC, 0) };
    let duplicate = owned_fd(duplicate)?;
    let stream = unsafe { libc::fdopendir(duplicate.as_raw_fd()) };
    if stream.is_null() {
        return Err(io::Error::last_os_error().to_string());
    }
    std::mem::forget(duplicate);
    let result = (|| {
        loop {
            unsafe { *libc::__errno_location() = 0 };
            let entry = unsafe { libc::readdir(stream) };
            if entry.is_null() {
                let errno = unsafe { *libc::__errno_location() };
                return if errno == 0 {
                    Ok(())
                } else {
                    Err(io::Error::from_raw_os_error(errno).to_string())
                };
            }
            let name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) };
            if name.to_bytes() == b"." || name.to_bytes() == b".." {
                continue;
            }
            let name = OsStr::from_bytes(name.to_bytes());
            let stat = stat_at_optional_raw(directory, name)?
                .ok_or_else(|| "workspace edit tombstone entry disappeared".to_owned())?;
            if path_kind(&stat) == Ok(SecurePathKind::Directory) {
                let child = open_file_at(directory, name, libc::O_RDONLY | libc::O_DIRECTORY)?;
                remove_directory_contents(child.as_raw_fd())?;
                unlink_directory_at(directory, name)?;
                inject_fault(SecureEditFault::RecursiveDiscard)?;
            } else {
                unlink_at(directory, name)?;
                inject_fault(SecureEditFault::RecursiveDiscard)?;
            }
        }
    })();
    unsafe { libc::closedir(stream) };
    result
}

fn directory_is_empty(directory: RawFd) -> Result<bool, String> {
    let duplicate = unsafe { libc::fcntl(directory, libc::F_DUPFD_CLOEXEC, 0) };
    let duplicate = owned_fd(duplicate)?;
    let stream = unsafe { libc::fdopendir(duplicate.as_raw_fd()) };
    if stream.is_null() {
        return Err(io::Error::last_os_error().to_string());
    }
    std::mem::forget(duplicate);
    let result = loop {
        unsafe { *libc::__errno_location() = 0 };
        let entry = unsafe { libc::readdir(stream) };
        if entry.is_null() {
            let errno = unsafe { *libc::__errno_location() };
            break if errno == 0 {
                Ok(true)
            } else {
                Err(io::Error::from_raw_os_error(errno).to_string())
            };
        }
        let name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) };
        if name.to_bytes() != b"." && name.to_bytes() != b".." {
            break Ok(false);
        }
    };
    unsafe { libc::closedir(stream) };
    result
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

fn identity(stat: &libc::stat) -> FileIdentity {
    FileIdentity {
        device: stat.st_dev,
        inode: stat.st_ino,
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
