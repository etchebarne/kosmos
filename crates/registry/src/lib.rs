mod entries;

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub enum ToolKind {
    Lsp,
    Formatter,
    Linter,
}

#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub enum Target {
    LinuxX64Gnu,
    LinuxArm64Gnu,
    LinuxX64Musl,
    LinuxArm64Musl,
    DarwinX64,
    DarwinArm64,
    WinX64,
    WinArm64,
}

#[derive(Clone, Debug)]
pub struct GithubAsset {
    pub target: Target,
    pub file: &'static str,
    pub bin: &'static str,
}

#[derive(Clone, Debug)]
pub enum InstallSource {
    Npm {
        package: &'static str,
        bin: &'static str,
        extra_packages: &'static [&'static str],
    },
    Pip {
        package: &'static str,
        bin: &'static str,
        extra_packages: &'static [&'static str],
    },
    Cargo {
        crate_name: &'static str,
        bin: &'static str,
    },
    Go {
        module: &'static str,
        bin: &'static str,
    },
    GithubRelease {
        repo: &'static str,
        assets: &'static [GithubAsset],
    },
}

#[derive(Clone, Debug)]
pub struct LaunchSpec {
    pub args: &'static [&'static str],
    pub env: &'static [(&'static str, &'static str)],
}

#[derive(Clone, Debug)]
pub struct RegistryEntry {
    pub id: &'static str,
    pub kinds: &'static [ToolKind],
    pub languages: &'static [&'static str],
    pub install: InstallSource,
    pub launch: LaunchSpec,
}

use entries::ENTRIES;

pub fn all() -> &'static [RegistryEntry] {
    ENTRIES
}

pub fn get(id: &str) -> Option<&'static RegistryEntry> {
    ENTRIES.iter().find(|e| e.id == id)
}

pub fn by_kind(kind: ToolKind) -> impl Iterator<Item = &'static RegistryEntry> {
    ENTRIES.iter().filter(move |e| e.kinds.contains(&kind))
}

pub fn for_language<'a>(
    language: &'a str,
    kind: ToolKind,
) -> impl Iterator<Item = &'static RegistryEntry> + 'a {
    ENTRIES
        .iter()
        .filter(move |e| e.kinds.contains(&kind) && e.languages.contains(&language))
}
