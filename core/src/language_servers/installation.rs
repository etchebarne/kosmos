use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::LanguageServerError;
use super::catalog::{
    ArtifactCompression, LanguageServerArtifact, LanguageServerDefinition, NpmPackage,
};

const MAX_DOWNLOAD_BYTES: u64 = 64 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 256 * 1024 * 1024;
const NPM_INSTALL_TIMEOUT: Duration = Duration::from_secs(180);
const MANIFEST_SCHEMA_VERSION: u32 = 1;
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);
static NPM_RUNTIME_AVAILABLE: OnceLock<bool> = OnceLock::new();

#[derive(Clone, Debug)]
pub struct LanguageServerPaths {
    data_directory: PathBuf,
    cache_directory: PathBuf,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallationManifest {
    schema_version: u32,
    server_id: String,
    version: String,
    operating_system: String,
    architecture: String,
    source_url: String,
    sha256: String,
    executable: String,
}

impl LanguageServerPaths {
    pub fn new(data_directory: impl Into<PathBuf>, cache_directory: impl Into<PathBuf>) -> Self {
        Self {
            data_directory: data_directory.into(),
            cache_directory: cache_directory.into(),
        }
    }

    pub fn data_directory(&self) -> &Path {
        &self.data_directory
    }

    pub fn cache_directory(&self) -> &Path {
        &self.cache_directory
    }

    pub(crate) fn prepare(&self) -> Result<(), LanguageServerError> {
        fs::create_dir_all(&self.data_directory).map_err(LanguageServerError::io)?;
        fs::create_dir_all(&self.cache_directory).map_err(LanguageServerError::io)?;
        validate_managed_directory(&self.data_directory)?;
        validate_managed_directory(&self.cache_directory)
    }

    fn server_directory(&self, definition: &LanguageServerDefinition) -> PathBuf {
        self.data_directory.join(definition.id)
    }

    fn version_directory(&self, definition: &LanguageServerDefinition, version: &str) -> PathBuf {
        self.server_directory(definition).join(version)
    }
}

pub(crate) fn installed_version(
    paths: &LanguageServerPaths,
    definition: &LanguageServerDefinition,
    selected_version: Option<&str>,
) -> Option<String> {
    let version = selected_version?;
    if !is_safe_component(version) {
        return None;
    }
    let directory = paths.version_directory(definition, version);
    validate_installation(&directory, definition, version).then(|| version.to_owned())
}

pub(crate) fn installed_executable(
    paths: &LanguageServerPaths,
    definition: &LanguageServerDefinition,
    selected_version: Option<&str>,
) -> Option<PathBuf> {
    let version = installed_version(paths, definition, selected_version)?;
    Some(
        paths
            .version_directory(definition, &version)
            .join(definition.executable),
    )
}

pub(crate) fn installation_supported(definition: &LanguageServerDefinition) -> bool {
    super::catalog::current_artifact(definition).is_some()
        || (!definition.npm_packages.is_empty() && npm_runtime_available())
}

fn npm_runtime_available() -> bool {
    *NPM_RUNTIME_AVAILABLE.get_or_init(|| {
        let npm_available = Command::new("npm")
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        let node_major = Command::new("node")
            .arg("--version")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .and_then(|version| {
                version
                    .trim()
                    .trim_start_matches('v')
                    .split('.')
                    .next()
                    .and_then(|major| major.parse::<u32>().ok())
            });
        npm_available && node_major.is_some_and(|major| major >= 20)
    })
}

pub(crate) fn install(
    paths: &LanguageServerPaths,
    definition: &LanguageServerDefinition,
) -> Result<(), LanguageServerError> {
    paths.prepare()?;
    let server_directory = paths.server_directory(definition);
    fs::create_dir_all(&server_directory).map_err(LanguageServerError::io)?;
    validate_managed_directory(&server_directory)?;

    let final_directory = paths.version_directory(definition, definition.version);
    if validate_installation(&final_directory, definition, definition.version) {
        return Ok(());
    }
    if final_directory.exists() {
        remove_entry(&final_directory)?;
    }

    let suffix = unique_suffix();
    let download_path = paths
        .cache_directory
        .join(format!("{}-{suffix}.download", definition.id));
    let temporary_directory = server_directory.join(format!(".install-{suffix}"));
    let result = if let Some(artifact) = super::catalog::current_artifact(definition) {
        install_portable_into(
            &download_path,
            &temporary_directory,
            &final_directory,
            definition,
            artifact,
        )
    } else if !definition.npm_packages.is_empty() {
        install_npm_into(&temporary_directory, &final_directory, definition)
    } else {
        Err(LanguageServerError::UnsupportedPlatform)
    };

    let _ = fs::remove_file(&download_path);
    let _ = fs::remove_dir_all(&temporary_directory);
    result
}

pub(crate) fn uninstall(
    paths: &LanguageServerPaths,
    definition: &LanguageServerDefinition,
    version: &str,
) -> Result<Option<PathBuf>, LanguageServerError> {
    if !is_safe_component(version) {
        return Err(LanguageServerError::InvalidManifest(
            "selected version is not a safe path component".to_owned(),
        ));
    }
    let directory = paths.version_directory(definition, version);
    if !directory.exists() {
        return Ok(None);
    }

    let trash = paths
        .server_directory(definition)
        .join(format!(".remove-{}", unique_suffix()));
    fs::rename(&directory, &trash).map_err(LanguageServerError::io)?;
    Ok(Some(trash))
}

pub(crate) fn restore_uninstall(
    paths: &LanguageServerPaths,
    definition: &LanguageServerDefinition,
    version: &str,
    trash: &Path,
) -> Result<(), LanguageServerError> {
    if !is_safe_component(version) {
        return Err(LanguageServerError::InvalidManifest(
            "selected version is not a safe path component".to_owned(),
        ));
    }
    fs::rename(trash, paths.version_directory(definition, version)).map_err(LanguageServerError::io)
}

pub(crate) fn finish_uninstall(trash: Option<PathBuf>) {
    if let Some(trash) = trash {
        let _ = remove_entry(&trash);
    }
}

pub(crate) fn clean_temporary_directories(paths: &LanguageServerPaths) {
    if is_managed_directory(&paths.data_directory)
        && let Ok(server_directories) = fs::read_dir(&paths.data_directory)
    {
        for server_directory in server_directories.flatten() {
            if !server_directory.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let Ok(entries) = fs::read_dir(server_directory.path()) else {
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

    if !is_managed_directory(&paths.cache_directory) {
        return;
    }
    let Ok(downloads) = fs::read_dir(&paths.cache_directory) else {
        return;
    };
    for download in downloads.flatten() {
        if download
            .path()
            .extension()
            .is_some_and(|extension| extension == "download")
        {
            let _ = fs::remove_file(download.path());
        }
    }
}

pub(crate) fn clean_stale_versions(
    paths: &LanguageServerPaths,
    definition: &LanguageServerDefinition,
    selected_version: Option<&str>,
) {
    if !is_managed_directory(&paths.data_directory) {
        return;
    }
    let server_directory = paths.server_directory(definition);
    if !is_managed_directory(&server_directory) {
        return;
    }
    let Ok(entries) = fs::read_dir(server_directory) else {
        return;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let is_selected = selected_version.is_some_and(|version| name == version);
        if name != definition.version
            && !is_selected
            && entry.file_type().is_ok_and(|kind| kind.is_dir())
        {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

fn install_portable_into(
    download_path: &Path,
    temporary_directory: &Path,
    final_directory: &Path,
    definition: &LanguageServerDefinition,
    artifact: &LanguageServerArtifact,
) -> Result<(), LanguageServerError> {
    download(artifact, download_path)?;
    fs::create_dir(temporary_directory).map_err(LanguageServerError::io)?;

    let executable_path = temporary_directory.join(definition.executable);
    match artifact.compression {
        ArtifactCompression::Gzip => decompress_gzip(download_path, &executable_path)?,
    }
    let mut permissions = fs::metadata(&executable_path)
        .map_err(LanguageServerError::io)?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&executable_path, permissions).map_err(LanguageServerError::io)?;

    write_manifest(
        temporary_directory,
        &InstallationManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            server_id: definition.id.to_owned(),
            version: definition.version.to_owned(),
            operating_system: artifact.operating_system.to_owned(),
            architecture: artifact.architecture.to_owned(),
            source_url: artifact.url.to_owned(),
            sha256: artifact.sha256.to_owned(),
            executable: definition.executable.to_owned(),
        },
    )?;
    fs::rename(temporary_directory, final_directory).map_err(LanguageServerError::io)
}

fn install_npm_into(
    temporary_directory: &Path,
    final_directory: &Path,
    definition: &LanguageServerDefinition,
) -> Result<(), LanguageServerError> {
    fs::create_dir(temporary_directory).map_err(LanguageServerError::io)?;
    let mut command = Command::new("npm");
    command
        .arg("install")
        .arg("--prefix")
        .arg(temporary_directory)
        .args([
            "--ignore-scripts",
            "--no-audit",
            "--no-fund",
            "--save-exact",
        ])
        .args(definition.npm_packages.iter().map(|package| package.spec))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    run_command(command, NPM_INSTALL_TIMEOUT)?;
    verify_npm_integrities(temporary_directory, definition.npm_packages)?;

    let executable = temporary_directory.join(definition.executable);
    if !validated_executable(temporary_directory, &executable) {
        return Err(LanguageServerError::InvalidArtifact(format!(
            "npm package did not provide `{}`",
            definition.executable
        )));
    }
    write_manifest(
        temporary_directory,
        &InstallationManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            server_id: definition.id.to_owned(),
            version: definition.version.to_owned(),
            operating_system: std::env::consts::OS.to_owned(),
            architecture: std::env::consts::ARCH.to_owned(),
            source_url: npm_source(definition),
            sha256: "npm-package-lock".to_owned(),
            executable: definition.executable.to_owned(),
        },
    )?;
    fs::rename(temporary_directory, final_directory).map_err(LanguageServerError::io)
}

fn run_command(mut command: Command, timeout: Duration) -> Result<(), LanguageServerError> {
    let mut child = command.spawn().map_err(|error| {
        LanguageServerError::RuntimeUnavailable(format!("npm could not start: {error}"))
    })?;
    let started = Instant::now();
    loop {
        match child.try_wait().map_err(LanguageServerError::io)? {
            Some(status) if status.success() => return Ok(()),
            Some(status) => {
                return Err(LanguageServerError::PackageInstall(format!(
                    "npm exited with {status}"
                )));
            }
            None if started.elapsed() < timeout => thread::sleep(Duration::from_millis(100)),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(LanguageServerError::PackageInstall(
                    "npm installation timed out".to_owned(),
                ));
            }
        }
    }
}

fn download(
    artifact: &LanguageServerArtifact,
    destination: &Path,
) -> Result<(), LanguageServerError> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .build();
    let agent: ureq::Agent = config.into();
    let mut response = agent
        .get(artifact.url)
        .call()
        .map_err(|error| LanguageServerError::Download(error.to_string()))?;
    if response
        .headers()
        .get("content-length")
        .and_then(|length| length.to_str().ok())
        .and_then(|length| length.parse::<u64>().ok())
        .is_some_and(|length| length > MAX_DOWNLOAD_BYTES)
    {
        return Err(LanguageServerError::DownloadTooLarge);
    }

    let mut file = File::create(destination).map_err(LanguageServerError::io)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut downloaded = 0_u64;
    let mut body = response.body_mut().as_reader();

    loop {
        let count = body.read(&mut buffer).map_err(LanguageServerError::io)?;
        if count == 0 {
            break;
        }
        downloaded = downloaded
            .checked_add(count as u64)
            .ok_or(LanguageServerError::DownloadTooLarge)?;
        if downloaded > MAX_DOWNLOAD_BYTES {
            return Err(LanguageServerError::DownloadTooLarge);
        }
        hasher.update(&buffer[..count]);
        file.write_all(&buffer[..count])
            .map_err(LanguageServerError::io)?;
    }
    file.sync_all().map_err(LanguageServerError::io)?;

    let actual = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if actual != artifact.sha256 {
        return Err(LanguageServerError::ChecksumMismatch);
    }
    Ok(())
}

fn decompress_gzip(source: &Path, destination: &Path) -> Result<(), LanguageServerError> {
    let source = File::open(source).map_err(LanguageServerError::io)?;
    let decoder = GzDecoder::new(source);
    let mut decoder = decoder.take(MAX_EXPANDED_BYTES + 1);
    let mut destination = File::create(destination).map_err(LanguageServerError::io)?;
    let expanded_bytes = std::io::copy(&mut decoder, &mut destination)
        .map_err(|error| LanguageServerError::InvalidArtifact(error.to_string()))?;
    if expanded_bytes > MAX_EXPANDED_BYTES {
        return Err(LanguageServerError::InvalidArtifact(
            "expanded artifact exceeded the size limit".to_owned(),
        ));
    }
    destination.sync_all().map_err(LanguageServerError::io)
}

fn write_manifest(
    directory: &Path,
    manifest: &InstallationManifest,
) -> Result<(), LanguageServerError> {
    let bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|error| LanguageServerError::InvalidManifest(error.to_string()))?;
    let path = directory.join("installation.json");
    let mut file = File::create(path).map_err(LanguageServerError::io)?;
    file.write_all(&bytes).map_err(LanguageServerError::io)?;
    file.sync_all().map_err(LanguageServerError::io)
}

fn validate_installation(
    directory: &Path,
    definition: &LanguageServerDefinition,
    version: &str,
) -> bool {
    let manifest = fs::read(directory.join("installation.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<InstallationManifest>(&bytes).ok());
    let Some(manifest) = manifest else {
        return false;
    };
    let portable_source = manifest.source_url.starts_with("https://")
        && manifest.sha256.len() == 64
        && manifest.sha256.bytes().all(|byte| byte.is_ascii_hexdigit());
    let valid_npm_source =
        manifest.source_url.starts_with("npm:") && manifest.sha256 == "npm-package-lock";
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION
        || manifest.server_id != definition.id
        || manifest.version != version
        || manifest.operating_system != std::env::consts::OS
        || manifest.architecture != std::env::consts::ARCH
        || (!portable_source && !valid_npm_source)
        || manifest.executable != definition.executable
    {
        return false;
    }
    if version == definition.version {
        if let Some(artifact) = super::catalog::current_artifact(definition) {
            if manifest.operating_system != artifact.operating_system
                || manifest.architecture != artifact.architecture
                || manifest.source_url != artifact.url
                || manifest.sha256 != artifact.sha256
            {
                return false;
            }
        } else if !definition.npm_packages.is_empty()
            && (manifest.source_url != npm_source(definition)
                || manifest.sha256 != "npm-package-lock")
        {
            return false;
        }
    }

    let executable = directory.join(&manifest.executable);
    validated_executable(directory, &executable)
}

fn validated_executable(directory: &Path, executable: &Path) -> bool {
    let Ok(directory) = fs::canonicalize(directory) else {
        return false;
    };
    let Ok(executable) = fs::canonicalize(executable) else {
        return false;
    };
    executable.starts_with(&directory)
        && fs::metadata(executable).is_ok_and(|metadata| {
            metadata.file_type().is_file() && metadata.permissions().mode() & 0o111 != 0
        })
}

fn npm_source(definition: &LanguageServerDefinition) -> String {
    format!(
        "npm:{}",
        definition
            .npm_packages
            .iter()
            .map(|package| package.spec)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn verify_npm_integrities(
    directory: &Path,
    packages: &[NpmPackage],
) -> Result<(), LanguageServerError> {
    let lockfile =
        fs::read(directory.join("package-lock.json")).map_err(LanguageServerError::io)?;
    let lockfile: serde_json::Value = serde_json::from_slice(&lockfile)
        .map_err(|error| LanguageServerError::InvalidArtifact(error.to_string()))?;
    for package in packages {
        let name = package.spec[..package.spec.rfind('@').ok_or_else(|| {
            LanguageServerError::InvalidArtifact("invalid pinned npm package".to_owned())
        })?]
            .to_owned();
        let key = format!("node_modules/{name}");
        let integrity = lockfile
            .pointer("/packages")
            .and_then(serde_json::Value::as_object)
            .and_then(|packages| packages.get(&key))
            .and_then(|package| package.get("integrity"))
            .and_then(serde_json::Value::as_str);
        if integrity != Some(package.integrity) {
            return Err(LanguageServerError::ChecksumMismatch);
        }
    }
    Ok(())
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

fn validate_managed_directory(path: &Path) -> Result<(), LanguageServerError> {
    if is_managed_directory(path) {
        Ok(())
    } else {
        Err(LanguageServerError::InvalidManifest(format!(
            "managed directory is not a regular directory: {}",
            path.display()
        )))
    }
}

fn is_managed_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_dir())
}

fn remove_entry(path: &Path) -> Result<(), LanguageServerError> {
    let metadata = fs::symlink_metadata(path).map_err(LanguageServerError::io)?;
    if metadata.file_type().is_dir() {
        fs::remove_dir_all(path).map_err(LanguageServerError::io)
    } else {
        fs::remove_file(path).map_err(LanguageServerError::io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_must_be_single_normal_path_components() {
        assert!(is_safe_component("2026-07-06"));
        assert!(!is_safe_component(""));
        assert!(!is_safe_component("../outside"));
        assert!(!is_safe_component("nested/version"));
        assert!(!is_safe_component("/absolute"));
    }
}
