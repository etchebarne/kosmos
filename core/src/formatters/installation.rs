use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};

use super::FormatterError;
use super::catalog::FormatterDefinition;

const INSTALL_TIMEOUT: Duration = Duration::from_secs(180);
const MANIFEST_SCHEMA_VERSION: u32 = 1;
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);
static NPM_RUNTIME_AVAILABLE: OnceLock<bool> = OnceLock::new();
static NODE_VERSION: OnceLock<Option<(u32, u32)>> = OnceLock::new();

#[derive(Clone, Debug)]
pub struct FormatterPaths {
    data_directory: PathBuf,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallationManifest {
    schema_version: u32,
    formatter_id: String,
    version: String,
    operating_system: String,
    architecture: String,
    source: String,
    integrity: String,
    executable: String,
}

impl FormatterPaths {
    pub fn new(data_directory: impl Into<PathBuf>) -> Self {
        Self {
            data_directory: data_directory.into(),
        }
    }

    fn formatter_directory(&self, definition: &FormatterDefinition) -> PathBuf {
        self.data_directory.join(definition.id)
    }

    fn version_directory(&self, definition: &FormatterDefinition, version: &str) -> PathBuf {
        self.formatter_directory(definition).join(version)
    }

    pub(super) fn prepare(&self) -> Result<(), FormatterError> {
        fs::create_dir_all(&self.data_directory).map_err(FormatterError::io)?;
        validate_directory(&self.data_directory)
    }
}

pub(super) fn installation_supported() -> bool {
    *NPM_RUNTIME_AVAILABLE.get_or_init(|| {
        command_succeeds("npm", "--version")
            && node_version().is_some_and(|version| version >= (22, 6))
    })
}

pub(super) fn installed_version(
    paths: &FormatterPaths,
    definition: &FormatterDefinition,
) -> Option<String> {
    let current = paths.version_directory(definition, definition.version);
    if validate_installation(&current, definition, definition.version) {
        return Some(definition.version.to_owned());
    }
    let entries = fs::read_dir(paths.formatter_directory(definition)).ok()?;
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .find(|version| {
            validate_installation(
                &paths.version_directory(definition, version),
                definition,
                version,
            )
        })
}

pub(super) fn installed_executable(
    paths: &FormatterPaths,
    definition: &FormatterDefinition,
) -> Option<PathBuf> {
    let version = installed_version(paths, definition)?;
    Some(
        paths
            .version_directory(definition, &version)
            .join(definition.executable),
    )
}

pub(super) fn install(
    paths: &FormatterPaths,
    definition: &FormatterDefinition,
) -> Result<(), FormatterError> {
    if !installation_supported() {
        return Err(FormatterError::UnsupportedPlatform);
    }
    paths.prepare()?;
    let formatter_directory = paths.formatter_directory(definition);
    fs::create_dir_all(&formatter_directory).map_err(FormatterError::io)?;
    validate_directory(&formatter_directory)?;
    let final_directory = paths.version_directory(definition, definition.version);
    if validate_installation(&final_directory, definition, definition.version) {
        return Ok(());
    }
    if final_directory.exists() {
        remove_entry(&final_directory)?;
    }

    let temporary = formatter_directory.join(format!(".install-{}", unique_suffix()));
    let result = install_npm(&temporary, &final_directory, definition);
    let _ = fs::remove_dir_all(&temporary);
    result?;
    clean_stale_versions(paths, definition);
    Ok(())
}

pub(super) fn uninstall(
    paths: &FormatterPaths,
    definition: &FormatterDefinition,
) -> Result<(), FormatterError> {
    if installed_version(paths, definition).is_none() {
        return Ok(());
    }
    paths.prepare()?;
    let directory = paths.formatter_directory(definition);
    let trash = paths
        .data_directory
        .join(format!(".remove-{}-{}", definition.id, unique_suffix()));
    fs::rename(&directory, &trash).map_err(FormatterError::io)?;
    remove_entry(&trash)
}

pub(super) fn clean_temporary_directories(paths: &FormatterPaths) {
    let Ok(formatters) = fs::read_dir(&paths.data_directory) else {
        return;
    };
    for formatter in formatters.flatten() {
        let name = formatter.file_name();
        if name.to_string_lossy().starts_with(".remove-")
            && formatter.file_type().is_ok_and(|kind| kind.is_dir())
        {
            let _ = fs::remove_dir_all(formatter.path());
            continue;
        }
        if !formatter.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let Ok(entries) = fs::read_dir(formatter.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if (name.starts_with(".install-") || name.starts_with(".remove-"))
                && entry.file_type().is_ok_and(|kind| kind.is_dir())
            {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
}

fn install_npm(
    temporary: &Path,
    final_directory: &Path,
    definition: &FormatterDefinition,
) -> Result<(), FormatterError> {
    fs::create_dir(temporary).map_err(FormatterError::io)?;
    let mut command = Command::new("npm");
    command
        .arg("install")
        .arg("--prefix")
        .arg(temporary)
        .args([
            "--ignore-scripts",
            "--no-audit",
            "--no-fund",
            "--save-exact",
            definition.npm_package,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    run_command(command)?;
    verify_integrity(temporary, definition)?;
    let executable = temporary.join(definition.executable);
    if !validated_executable(temporary, &executable) {
        return Err(FormatterError::InvalidInstallation(format!(
            "package did not provide `{}`",
            definition.executable
        )));
    }
    write_manifest(temporary, definition)?;
    fs::rename(temporary, final_directory).map_err(FormatterError::io)
}

fn run_command(mut command: Command) -> Result<(), FormatterError> {
    let mut child = command
        .spawn()
        .map_err(|error| FormatterError::Install(format!("npm could not start: {error}")))?;
    let started = Instant::now();
    loop {
        match child.try_wait().map_err(FormatterError::io)? {
            Some(status) if status.success() => return Ok(()),
            Some(status) => {
                return Err(FormatterError::Install(format!("npm exited with {status}")));
            }
            None if started.elapsed() < INSTALL_TIMEOUT => {
                thread::sleep(Duration::from_millis(100))
            }
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(FormatterError::Install(
                    "npm installation timed out".to_owned(),
                ));
            }
        }
    }
}

fn verify_integrity(
    directory: &Path,
    definition: &FormatterDefinition,
) -> Result<(), FormatterError> {
    let lockfile = fs::read(directory.join("package-lock.json")).map_err(FormatterError::io)?;
    let lockfile: serde_json::Value = serde_json::from_slice(&lockfile)
        .map_err(|error| FormatterError::InvalidInstallation(error.to_string()))?;
    let package_name = definition
        .npm_package
        .rsplit_once('@')
        .map(|(name, _)| name)
        .ok_or_else(|| FormatterError::InvalidInstallation("package is not pinned".to_owned()))?;
    let key = format!("node_modules/{package_name}");
    let integrity = lockfile
        .pointer("/packages")
        .and_then(serde_json::Value::as_object)
        .and_then(|packages| packages.get(&key))
        .and_then(|package| package.get("integrity"))
        .and_then(serde_json::Value::as_str);
    if integrity == Some(definition.npm_integrity) {
        Ok(())
    } else {
        Err(FormatterError::ChecksumMismatch)
    }
}

fn write_manifest(
    directory: &Path,
    definition: &FormatterDefinition,
) -> Result<(), FormatterError> {
    let manifest = InstallationManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        formatter_id: definition.id.to_owned(),
        version: definition.version.to_owned(),
        operating_system: std::env::consts::OS.to_owned(),
        architecture: std::env::consts::ARCH.to_owned(),
        source: format!("npm:{}", definition.npm_package),
        integrity: definition.npm_integrity.to_owned(),
        executable: definition.executable.to_owned(),
    };
    let bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| FormatterError::InvalidInstallation(error.to_string()))?;
    let mut file = File::create(directory.join("installation.json")).map_err(FormatterError::io)?;
    file.write_all(&bytes).map_err(FormatterError::io)?;
    file.sync_all().map_err(FormatterError::io)
}

fn validate_installation(
    directory: &Path,
    definition: &FormatterDefinition,
    version: &str,
) -> bool {
    if !is_safe_component(version) {
        return false;
    }
    let manifest = fs::read(directory.join("installation.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<InstallationManifest>(&bytes).ok());
    let Some(manifest) = manifest else {
        return false;
    };
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION
        || manifest.formatter_id != definition.id
        || manifest.version != version
        || manifest.operating_system != std::env::consts::OS
        || manifest.architecture != std::env::consts::ARCH
        || manifest.executable != definition.executable
        || !manifest.source.starts_with("npm:")
        || !manifest.integrity.starts_with("sha512-")
    {
        return false;
    }
    if version == definition.version
        && (manifest.source != format!("npm:{}", definition.npm_package)
            || manifest.integrity != definition.npm_integrity)
    {
        return false;
    }
    validated_executable(directory, &directory.join(&manifest.executable))
}

fn clean_stale_versions(paths: &FormatterPaths, definition: &FormatterDefinition) {
    let Ok(entries) = fs::read_dir(paths.formatter_directory(definition)) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name() != definition.version
            && entry.file_type().is_ok_and(|kind| kind.is_dir())
        {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

fn validated_executable(directory: &Path, executable: &Path) -> bool {
    let (Ok(directory), Ok(executable)) =
        (fs::canonicalize(directory), fs::canonicalize(executable))
    else {
        return false;
    };
    executable.starts_with(&directory)
        && fs::metadata(executable).is_ok_and(|metadata| {
            metadata.file_type().is_file() && metadata.permissions().mode() & 0o111 != 0
        })
}

fn command_succeeds(command: &str, argument: &str) -> bool {
    bounded_command_output(command, argument).is_some_and(|(status, _)| status.success())
}

pub(super) fn node_version() -> Option<(u32, u32)> {
    *NODE_VERSION.get_or_init(|| {
        bounded_command_output("node", "--version")
            .filter(|(status, _)| status.success())
            .and_then(|(_, output)| String::from_utf8(output).ok())
            .and_then(|version| {
                let mut parts = version.trim().trim_start_matches('v').split('.');
                Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
            })
    })
}

fn bounded_command_output(
    command: &str,
    argument: &str,
) -> Option<(std::process::ExitStatus, Vec<u8>)> {
    let mut child = Command::new(command)
        .arg(argument)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .ok()?;
    let process_group = Pid::from_raw(i32::try_from(child.id()).ok()?);
    let mut stdout = child.stdout.take()?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut output = Vec::new();
        let result = stdout
            .by_ref()
            .take(64 * 1024)
            .read_to_end(&mut output)
            .map(|_| output);
        let _ = sender.send(result);
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait().ok()? {
            Some(status) => break status,
            None if started.elapsed() < Duration::from_secs(2) => {
                thread::sleep(Duration::from_millis(10));
            }
            None => {
                let _ = killpg(process_group, Signal::SIGKILL);
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let _ = killpg(process_group, Signal::SIGKILL);
    let output = receiver
        .recv_timeout(Duration::from_millis(500))
        .ok()?
        .ok()?;
    Some((status, output))
}

fn validate_directory(path: &Path) -> Result<(), FormatterError> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_dir()) {
        Ok(())
    } else {
        Err(FormatterError::InvalidInstallation(format!(
            "managed path is not a directory: {}",
            path.display()
        )))
    }
}

fn remove_entry(path: &Path) -> Result<(), FormatterError> {
    let metadata = fs::symlink_metadata(path).map_err(FormatterError::io)?;
    if metadata.file_type().is_dir() {
        fs::remove_dir_all(path).map_err(FormatterError::io)
    } else {
        fs::remove_file(path).map_err(FormatterError::io)
    }
}

fn unique_suffix() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
    )
}

fn is_safe_component(value: &str) -> bool {
    let mut components = Path::new(value).components();
    matches!(components.next(), Some(Component::Normal(component)) if component == value)
        && components.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_must_be_single_normal_components() {
        assert!(is_safe_component("3.9.5"));
        assert!(!is_safe_component("../outside"));
        assert!(!is_safe_component("nested/version"));
    }
}
