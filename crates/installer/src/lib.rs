use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use registry::{GithubAsset, InstallSource, LaunchSpec, RegistryEntry, Target};

pub struct InstalledTool {
    pub bin_path: PathBuf,
    pub launch: &'static LaunchSpec,
}

#[derive(Debug)]
pub enum InstallError {
    Io(io::Error),
    UnsupportedPlatform,
    PackageManagerMissing(&'static str),
    CommandFailed { tool: &'static str, message: String },
    BinaryMissing(PathBuf),
    SourceUnsupported(&'static str),
}

impl From<io::Error> for InstallError {
    fn from(e: io::Error) -> Self {
        InstallError::Io(e)
    }
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallError::Io(e) => write!(f, "io error: {e}"),
            InstallError::UnsupportedPlatform => write!(f, "unsupported platform"),
            InstallError::PackageManagerMissing(name) => {
                write!(f, "missing package manager: {name}")
            }
            InstallError::CommandFailed { tool, message } => {
                write!(f, "{tool} failed: {message}")
            }
            InstallError::BinaryMissing(path) => {
                write!(f, "expected binary at {} after install", path.display())
            }
            InstallError::SourceUnsupported(name) => {
                write!(f, "install source not yet supported: {name}")
            }
        }
    }
}

impl std::error::Error for InstallError {}

pub fn cache_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".kosmos").join("tools")
}

pub fn tool_dir(entry: &RegistryEntry) -> PathBuf {
    cache_dir().join(entry.id)
}

pub fn bin_path(entry: &RegistryEntry) -> Option<PathBuf> {
    let dir = tool_dir(entry);
    match &entry.install {
        InstallSource::Npm { bin, .. } => Some(dir.join("node_modules").join(".bin").join(bin)),
        InstallSource::Pip { bin, .. } => Some(dir.join("venv").join("bin").join(bin)),
        InstallSource::Cargo { bin, .. } => Some(dir.join("bin").join(bin)),
        InstallSource::Go { bin, .. } => Some(dir.join("bin").join(bin)),
        InstallSource::GithubRelease { assets, .. } => {
            let target = current_target()?;
            assets
                .iter()
                .find(|a| a.target == target)
                .map(|a| dir.join(a.bin))
        }
    }
}

pub fn current_target() -> Option<Target> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some(Target::LinuxX64Gnu),
        ("linux", "aarch64") => Some(Target::LinuxArm64Gnu),
        ("macos", "x86_64") => Some(Target::DarwinX64),
        ("macos", "aarch64") => Some(Target::DarwinArm64),
        ("windows", "x86_64") => Some(Target::WinX64),
        ("windows", "aarch64") => Some(Target::WinArm64),
        _ => None,
    }
}

pub fn is_installed(entry: &RegistryEntry) -> bool {
    bin_path(entry).map(|p| p.exists()).unwrap_or(false)
}

pub fn ensure(entry: &'static RegistryEntry) -> Result<InstalledTool, InstallError> {
    let dir = tool_dir(entry);
    match &entry.install {
        InstallSource::Npm {
            package,
            bin,
            extra_packages,
        } => ensure_npm(entry, &dir, package, bin, extra_packages),
        InstallSource::Pip {
            package,
            bin,
            extra_packages,
        } => ensure_pip(entry, &dir, package, bin, extra_packages),
        InstallSource::Cargo { crate_name, bin } => ensure_cargo(entry, &dir, crate_name, bin),
        InstallSource::Go { module, bin } => ensure_go(entry, &dir, module, bin),
        InstallSource::GithubRelease { repo, assets } => {
            ensure_github_release(entry, &dir, repo, assets)
        }
    }
}

fn run(cmd: &mut Command, tool: &'static str) -> Result<(), InstallError> {
    let output = cmd.output().map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            InstallError::PackageManagerMissing(tool)
        } else {
            InstallError::Io(e)
        }
    })?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(InstallError::CommandFailed {
            tool,
            message: if message.is_empty() {
                format!("exit status {}", output.status)
            } else {
                message
            },
        });
    }
    Ok(())
}

fn finish(
    bin_path: PathBuf,
    entry: &'static RegistryEntry,
) -> Result<InstalledTool, InstallError> {
    if !bin_path.exists() {
        return Err(InstallError::BinaryMissing(bin_path));
    }
    Ok(InstalledTool {
        bin_path,
        launch: &entry.launch,
    })
}

fn ensure_npm(
    entry: &'static RegistryEntry,
    dir: &Path,
    package: &str,
    bin: &str,
    extra_packages: &[&str],
) -> Result<InstalledTool, InstallError> {
    let bin_path = dir.join("node_modules").join(".bin").join(bin);
    if bin_path.exists() {
        return finish(bin_path, entry);
    }
    fs::create_dir_all(dir)?;
    let mut cmd = Command::new("npm");
    cmd.arg("install")
        .arg("--prefix")
        .arg(dir)
        .arg("--silent")
        .arg("--no-progress")
        .arg("--no-fund")
        .arg("--no-audit")
        .arg(package)
        .args(extra_packages.iter().copied());
    run(&mut cmd, "npm")?;
    finish(bin_path, entry)
}

fn ensure_cargo(
    entry: &'static RegistryEntry,
    dir: &Path,
    crate_name: &str,
    bin: &str,
) -> Result<InstalledTool, InstallError> {
    let bin_path = dir.join("bin").join(bin);
    if bin_path.exists() {
        return finish(bin_path, entry);
    }
    fs::create_dir_all(dir)?;
    let mut cmd = Command::new("cargo");
    cmd.arg("install")
        .arg("--root")
        .arg(dir)
        .arg("--quiet")
        .arg(crate_name);
    run(&mut cmd, "cargo")?;
    finish(bin_path, entry)
}

fn ensure_go(
    entry: &'static RegistryEntry,
    dir: &Path,
    module: &str,
    bin: &str,
) -> Result<InstalledTool, InstallError> {
    let bin_dir = dir.join("bin");
    let bin_path = bin_dir.join(bin);
    if bin_path.exists() {
        return finish(bin_path, entry);
    }
    fs::create_dir_all(&bin_dir)?;
    let mut cmd = Command::new("go");
    cmd.arg("install")
        .arg(format!("{module}@latest"))
        .env("GOBIN", &bin_dir);
    run(&mut cmd, "go")?;
    finish(bin_path, entry)
}

fn ensure_github_release(
    entry: &'static RegistryEntry,
    dir: &Path,
    repo: &str,
    assets: &[GithubAsset],
) -> Result<InstalledTool, InstallError> {
    let target = current_target().ok_or(InstallError::UnsupportedPlatform)?;
    let asset = assets
        .iter()
        .find(|a| a.target == target)
        .ok_or(InstallError::UnsupportedPlatform)?;

    let bin_path = dir.join(asset.bin);
    if bin_path.exists() {
        return finish(bin_path, entry);
    }
    fs::create_dir_all(dir)?;

    let url = format!(
        "https://github.com/{repo}/releases/latest/download/{file}",
        file = asset.file
    );
    let response = ureq::get(&url)
        .call()
        .map_err(|e| InstallError::CommandFailed {
            tool: "github",
            message: format!("download failed: {e}"),
        })?;
    let mut body = response.into_reader();

    if asset.file.ends_with(".gz") {
        let mut decoder = flate2::read::GzDecoder::new(&mut body);
        let mut out = fs::File::create(&bin_path)?;
        io::copy(&mut decoder, &mut out)?;
    } else if asset.file.ends_with(".zip") {
        let mut tmp = tempfile_in(dir, asset.file)?;
        io::copy(&mut body, &mut tmp)?;
        tmp.seek(io::SeekFrom::Start(0))?;
        let mut archive = zip::ZipArchive::new(tmp).map_err(|e| InstallError::CommandFailed {
            tool: "zip",
            message: e.to_string(),
        })?;
        let mut entry_file =
            archive
                .by_name(asset.bin)
                .map_err(|e| InstallError::CommandFailed {
                    tool: "zip",
                    message: format!("missing {} in archive: {e}", asset.bin),
                })?;
        let mut out = fs::File::create(&bin_path)?;
        io::copy(&mut entry_file, &mut out)?;
    } else {
        let mut out = fs::File::create(&bin_path)?;
        io::copy(&mut body, &mut out)?;
    }

    set_executable(&bin_path)?;
    finish(bin_path, entry)
}

#[cfg(unix)]
fn set_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(perms.mode() | 0o111);
    fs::set_permissions(path, perms)
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> io::Result<()> {
    Ok(())
}

use std::io::Seek;

fn tempfile_in(dir: &Path, file_name: &str) -> io::Result<fs::File> {
    let path = dir.join(format!(".download-{file_name}"));
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
}

fn ensure_pip(
    entry: &'static RegistryEntry,
    dir: &Path,
    package: &str,
    bin: &str,
    extra_packages: &[&str],
) -> Result<InstalledTool, InstallError> {
    let venv_dir = dir.join("venv");
    let venv_bin = venv_dir.join("bin");
    let bin_path = venv_bin.join(bin);
    if bin_path.exists() {
        return finish(bin_path, entry);
    }
    fs::create_dir_all(dir)?;

    if !venv_dir.exists() {
        let mut venv_cmd = Command::new("python3");
        venv_cmd.arg("-m").arg("venv").arg(&venv_dir);
        run(&mut venv_cmd, "python3")?;
    }

    let mut install_cmd = Command::new(venv_bin.join("pip"));
    install_cmd
        .arg("install")
        .arg("--quiet")
        .arg("--disable-pip-version-check")
        .arg(package)
        .args(extra_packages.iter().copied());
    run(&mut install_cmd, "pip")?;

    finish(bin_path, entry)
}
