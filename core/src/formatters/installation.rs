use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::FormatterError;
use super::catalog::{
    ArtifactFormat, FormatterArtifact, FormatterDefinition, FormatterSource, current_artifact,
};
use super::process::{ProcessError, ProcessLimits, run_process, stderr_message};

const INSTALL_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_DOWNLOAD_BYTES: u64 = 64 * 1024 * 1024;
const MAX_EXECUTABLE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_INSTALL_STDERR_BYTES: usize = 64 * 1024;
const MANIFEST_SCHEMA_VERSION: u32 = 1;
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);
static NPM_RUNTIME_AVAILABLE: OnceLock<bool> = OnceLock::new();
static NODE_VERSION: OnceLock<Option<(u32, u32)>> = OnceLock::new();

#[derive(Clone, Debug)]
pub struct FormatterPaths {
    data_directory: PathBuf,
    cache_directory: PathBuf,
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
    pub fn new(data_directory: impl Into<PathBuf>, cache_directory: impl Into<PathBuf>) -> Self {
        Self {
            data_directory: data_directory.into(),
            cache_directory: cache_directory.into(),
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
        fs::create_dir_all(&self.cache_directory).map_err(FormatterError::io)?;
        validate_directory(&self.data_directory)?;
        validate_directory(&self.cache_directory)
    }
}

pub(super) fn installation_supported(definition: &FormatterDefinition) -> bool {
    match definition.source {
        FormatterSource::Npm { .. } => npm_runtime_available(),
        FormatterSource::Portable(_) => current_artifact(definition).is_some(),
    }
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
    if !installation_supported(definition) {
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

    let suffix = unique_suffix();
    let temporary = formatter_directory.join(format!(".install-{suffix}"));
    let download = paths
        .cache_directory
        .join(format!("{}-{suffix}.download", definition.id));
    let result = match definition.source {
        FormatterSource::Npm { .. } => install_npm(&temporary, &final_directory, definition),
        FormatterSource::Portable(_) => install_portable(
            &download,
            &temporary,
            &final_directory,
            definition,
            current_artifact(definition).expect("supported portable artifact exists"),
        ),
    };
    let _ = fs::remove_file(&download);
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
    if let Ok(downloads) = fs::read_dir(&paths.cache_directory) {
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
}

fn install_npm(
    temporary: &Path,
    final_directory: &Path,
    definition: &FormatterDefinition,
) -> Result<(), FormatterError> {
    let FormatterSource::Npm { package, .. } = definition.source else {
        return Err(FormatterError::InvalidInstallation(
            "formatter does not have an npm source".to_owned(),
        ));
    };
    fs::create_dir(temporary).map_err(FormatterError::io)?;
    let mut command = Command::new("npm");
    command.arg("install").arg("--prefix").arg(temporary).args([
        "--ignore-scripts",
        "--no-audit",
        "--no-fund",
        "--save-exact",
        "--loglevel=error",
        package,
    ]);
    run_install_command(&mut command, INSTALL_TIMEOUT)?;
    verify_integrity(temporary, definition)?;
    validate_installed_executable(temporary, definition)?;
    write_manifest(temporary, definition, None)?;
    fs::rename(temporary, final_directory).map_err(FormatterError::io)
}

fn install_portable(
    download_path: &Path,
    temporary: &Path,
    final_directory: &Path,
    definition: &FormatterDefinition,
    artifact: &FormatterArtifact,
) -> Result<(), FormatterError> {
    download_artifact(artifact, download_path)?;
    fs::create_dir(temporary).map_err(FormatterError::io)?;
    let executable = temporary.join(definition.executable);
    match artifact.format {
        ArtifactFormat::Raw => copy_bounded(download_path, &executable)?,
        ArtifactFormat::TarGzip => extract_tar_gzip(download_path, &executable, artifact)?,
    }
    let mut permissions = fs::metadata(&executable)
        .map_err(FormatterError::io)?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&executable, permissions).map_err(FormatterError::io)?;
    validate_installed_executable(temporary, definition)?;
    write_manifest(temporary, definition, Some(artifact))?;
    fs::rename(temporary, final_directory).map_err(FormatterError::io)
}

fn run_install_command(command: &mut Command, timeout: Duration) -> Result<(), FormatterError> {
    let output = run_process(
        command,
        None,
        ProcessLimits {
            timeout,
            stdout_bytes: 16 * 1024,
            stderr_bytes: MAX_INSTALL_STDERR_BYTES,
        },
    )
    .map_err(|error| match error {
        ProcessError::Start(error) => {
            FormatterError::Install(format!("npm could not start: {error}"))
        }
        ProcessError::Timeout => FormatterError::Install("npm installation timed out".to_owned()),
        error => FormatterError::Install(format!("npm process failed: {error:?}")),
    })?;
    if output.status.success() {
        return Ok(());
    }
    let details = stderr_message(&output);
    let details = if details.is_empty() {
        format!("npm exited with {}", output.status)
    } else {
        format!("npm exited with {}: {details}", output.status)
    };
    Err(FormatterError::Install(details))
}

fn download_artifact(
    artifact: &FormatterArtifact,
    destination: &Path,
) -> Result<(), FormatterError> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .build();
    let agent: ureq::Agent = config.into();
    let mut response = agent
        .get(artifact.url)
        .call()
        .map_err(|error| FormatterError::Install(format!("download failed: {error}")))?;
    if response
        .headers()
        .get("content-length")
        .and_then(|length| length.to_str().ok())
        .and_then(|length| length.parse::<u64>().ok())
        .is_some_and(|length| length > MAX_DOWNLOAD_BYTES)
    {
        return Err(FormatterError::Install(
            "download exceeded the size limit".to_owned(),
        ));
    }

    let mut file = File::create(destination).map_err(FormatterError::io)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut downloaded = 0_u64;
    let mut body = response.body_mut().as_reader();
    loop {
        let count = body.read(&mut buffer).map_err(FormatterError::io)?;
        if count == 0 {
            break;
        }
        downloaded = downloaded
            .checked_add(count as u64)
            .ok_or_else(|| FormatterError::Install("download is too large".to_owned()))?;
        if downloaded > MAX_DOWNLOAD_BYTES {
            return Err(FormatterError::Install(
                "download exceeded the size limit".to_owned(),
            ));
        }
        hasher.update(&buffer[..count]);
        file.write_all(&buffer[..count])
            .map_err(FormatterError::io)?;
    }
    file.sync_all().map_err(FormatterError::io)?;
    let actual = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if actual == artifact.sha256 {
        Ok(())
    } else {
        Err(FormatterError::ChecksumMismatch)
    }
}

fn copy_bounded(source: &Path, destination: &Path) -> Result<(), FormatterError> {
    let source = File::open(source).map_err(FormatterError::io)?;
    copy_reader_bounded(source, destination)
}

fn extract_tar_gzip(
    source: &Path,
    destination: &Path,
    artifact: &FormatterArtifact,
) -> Result<(), FormatterError> {
    let source = File::open(source).map_err(FormatterError::io)?;
    let decoder = GzDecoder::new(source);
    let mut archive = tar::Archive::new(decoder);
    let entries = archive
        .entries()
        .map_err(|error| FormatterError::InvalidInstallation(error.to_string()))?;
    for entry in entries {
        let mut entry =
            entry.map_err(|error| FormatterError::InvalidInstallation(error.to_string()))?;
        let path = entry
            .path()
            .map_err(|error| FormatterError::InvalidInstallation(error.to_string()))?;
        if path == Path::new(artifact.executable_path) {
            if !entry.header().entry_type().is_file() {
                return Err(FormatterError::InvalidInstallation(
                    "artifact executable is not a regular file".to_owned(),
                ));
            }
            return copy_reader_bounded(&mut entry, destination);
        }
    }
    Err(FormatterError::InvalidInstallation(format!(
        "artifact did not contain `{}`",
        artifact.executable_path
    )))
}

fn copy_reader_bounded(mut source: impl Read, destination: &Path) -> Result<(), FormatterError> {
    let mut destination = File::create(destination).map_err(FormatterError::io)?;
    let copied = std::io::copy(
        &mut source.by_ref().take(MAX_EXECUTABLE_BYTES + 1),
        &mut destination,
    )
    .map_err(FormatterError::io)?;
    if copied > MAX_EXECUTABLE_BYTES {
        return Err(FormatterError::InvalidInstallation(
            "artifact executable exceeded the size limit".to_owned(),
        ));
    }
    destination.sync_all().map_err(FormatterError::io)
}

fn verify_integrity(
    directory: &Path,
    definition: &FormatterDefinition,
) -> Result<(), FormatterError> {
    let FormatterSource::Npm { package, integrity } = definition.source else {
        return Err(FormatterError::InvalidInstallation(
            "formatter does not have an npm source".to_owned(),
        ));
    };
    let lockfile = fs::read(directory.join("package-lock.json")).map_err(FormatterError::io)?;
    let lockfile: serde_json::Value = serde_json::from_slice(&lockfile)
        .map_err(|error| FormatterError::InvalidInstallation(error.to_string()))?;
    let package_name = package
        .rsplit_once('@')
        .map(|(name, _)| name)
        .ok_or_else(|| FormatterError::InvalidInstallation("package is not pinned".to_owned()))?;
    let key = format!("node_modules/{package_name}");
    let installed_integrity = lockfile
        .pointer("/packages")
        .and_then(serde_json::Value::as_object)
        .and_then(|packages| packages.get(&key))
        .and_then(|package| package.get("integrity"))
        .and_then(serde_json::Value::as_str);
    if installed_integrity == Some(integrity) {
        Ok(())
    } else {
        Err(FormatterError::ChecksumMismatch)
    }
}

fn write_manifest(
    directory: &Path,
    definition: &FormatterDefinition,
    artifact: Option<&FormatterArtifact>,
) -> Result<(), FormatterError> {
    let (source, integrity) = match definition.source {
        FormatterSource::Npm { package, integrity } => {
            (format!("npm:{package}"), integrity.to_owned())
        }
        FormatterSource::Portable(_) => {
            let artifact = artifact.ok_or_else(|| {
                FormatterError::InvalidInstallation("portable artifact is missing".to_owned())
            })?;
            (artifact.url.to_owned(), artifact.sha256.to_owned())
        }
    };
    let manifest = InstallationManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        formatter_id: definition.id.to_owned(),
        version: definition.version.to_owned(),
        operating_system: std::env::consts::OS.to_owned(),
        architecture: std::env::consts::ARCH.to_owned(),
        source,
        integrity,
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
        || !valid_manifest_source(&manifest, definition.source)
    {
        return false;
    }
    if version == definition.version {
        let expected = match definition.source {
            FormatterSource::Npm { package, integrity } => {
                Some((format!("npm:{package}"), integrity))
            }
            FormatterSource::Portable(_) => current_artifact(definition)
                .map(|artifact| (artifact.url.to_owned(), artifact.sha256)),
        };
        if !expected.is_some_and(|(source, integrity)| {
            manifest.source == source && manifest.integrity == integrity
        }) {
            return false;
        }
    }
    validated_executable(directory, &directory.join(&manifest.executable))
}

fn valid_manifest_source(manifest: &InstallationManifest, source: FormatterSource) -> bool {
    match source {
        FormatterSource::Npm { .. } => {
            manifest.source.starts_with("npm:") && manifest.integrity.starts_with("sha512-")
        }
        FormatterSource::Portable(_) => {
            manifest.source.starts_with("https://")
                && manifest.integrity.len() == 64
                && manifest
                    .integrity
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
        }
    }
}

fn validate_installed_executable(
    directory: &Path,
    definition: &FormatterDefinition,
) -> Result<(), FormatterError> {
    if validated_executable(directory, &directory.join(definition.executable)) {
        Ok(())
    } else {
        Err(FormatterError::InvalidInstallation(format!(
            "package did not provide `{}`",
            definition.executable
        )))
    }
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

fn npm_runtime_available() -> bool {
    *NPM_RUNTIME_AVAILABLE.get_or_init(|| {
        command_succeeds("npm", "--version")
            && node_version().is_some_and(|version| version >= (22, 6))
    })
}

fn command_succeeds(command: &str, argument: &str) -> bool {
    bounded_command_output(command, argument).is_some_and(|output| output.status.success())
}

pub(super) fn node_version() -> Option<(u32, u32)> {
    *NODE_VERSION.get_or_init(|| {
        bounded_command_output("node", "--version")
            .filter(|output| output.status.success() && !output.stdout_truncated)
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .and_then(|version| {
                let mut parts = version.trim().trim_start_matches('v').split('.');
                Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
            })
    })
}

fn bounded_command_output(
    executable: &str,
    argument: &str,
) -> Option<super::process::ProcessOutput> {
    let mut command = Command::new(executable);
    command.arg(argument);
    run_process(
        &mut command,
        None,
        ProcessLimits {
            timeout: Duration::from_secs(2),
            stdout_bytes: 64 * 1024,
            stderr_bytes: 16 * 1024,
        },
    )
    .ok()
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
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn versions_must_be_single_normal_components() {
        assert!(is_safe_component("3.9.5"));
        assert!(!is_safe_component("../outside"));
        assert!(!is_safe_component("nested/version"));
    }

    #[test]
    fn portable_catalog_is_supported_only_on_cataloged_platforms() {
        let ruff = super::super::catalog::formatter_definition("ruff").unwrap();
        assert_eq!(
            installation_supported(ruff),
            matches!(
                (std::env::consts::OS, std::env::consts::ARCH),
                ("linux", "x86_64" | "aarch64")
            )
        );
    }

    #[test]
    fn install_errors_retain_bounded_stderr() {
        let directory = test_directory("install-error");
        let script = directory.join("npm");
        fs::write(
            &script,
            "#!/bin/sh\nprintf 'actionable install failure' >&2\nexit 7\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();

        let error = run_install_command(&mut Command::new(script), Duration::from_secs(1))
            .expect_err("command should fail");
        assert!(error.to_string().contains("actionable install failure"));
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn install_timeout_kills_descendant_processes() {
        let directory = test_directory("install-timeout");
        let marker = directory.join("descendant-survived");
        let script = directory.join("npm");
        fs::write(
            &script,
            "#!/bin/sh\nmarker=$1\n(sleep 0.2; printf survived > \"$marker\") &\nsleep 5\n",
        )
        .unwrap();
        let mut command = Command::new("/bin/sh");
        command.arg(&script).arg(&marker);

        let error = run_install_command(&mut command, Duration::from_millis(50))
            .expect_err("command should time out");
        assert!(error.to_string().contains("timed out"));
        std::thread::sleep(Duration::from_millis(350));
        assert!(!marker.exists());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn extracts_only_the_cataloged_executable_from_tar_gzip() {
        let directory = test_directory("tar-extract");
        let archive_path = directory.join("formatter.tar.gz");
        let archive = File::create(&archive_path).unwrap();
        let encoder = flate2::write::GzEncoder::new(archive, flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        let contents = b"formatter executable";
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        archive
            .append_data(&mut header, "release/formatter", contents.as_slice())
            .unwrap();
        archive.into_inner().unwrap().finish().unwrap();
        let destination = directory.join("formatter");
        let artifact = FormatterArtifact {
            operating_system: "linux",
            architecture: "x86_64",
            url: "https://example.invalid/formatter.tar.gz",
            sha256: "0000000000000000000000000000000000000000000000000000000000000000",
            format: ArtifactFormat::TarGzip,
            executable_path: "release/formatter",
        };

        extract_tar_gzip(&archive_path, &destination, &artifact).unwrap();

        assert_eq!(fs::read(&destination).unwrap(), contents);
        let _ = fs::remove_dir_all(directory);
    }

    fn test_directory(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "kosmos-formatter-install-{name}-{}-{}",
            std::process::id(),
            NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        path
    }
}
