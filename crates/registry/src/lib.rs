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

static ENTRIES: &[RegistryEntry] = &[
    // ─── LSPs (npm) ───────────────────────────────────────────────────────
    RegistryEntry {
        id: "typescript-language-server",
        kinds: &[ToolKind::Lsp],
        languages: &["typescript", "typescriptreact", "javascript", "javascriptreact"],
        install: InstallSource::Npm {
            package: "typescript-language-server",
            bin: "typescript-language-server",
            extra_packages: &["typescript"],
        },
        launch: LaunchSpec {
            args: &["--stdio"],
            env: &[],
        },
    },
    RegistryEntry {
        id: "tailwindcss-language-server",
        kinds: &[ToolKind::Lsp],
        languages: &[
            "html", "css", "javascript", "javascriptreact",
            "typescript", "typescriptreact", "vue", "svelte", "astro",
        ],
        install: InstallSource::Npm {
            package: "@tailwindcss/language-server",
            bin: "tailwindcss-language-server",
            extra_packages: &[],
        },
        launch: LaunchSpec {
            args: &["--stdio"],
            env: &[],
        },
    },
    RegistryEntry {
        id: "vue-language-server",
        kinds: &[ToolKind::Lsp],
        languages: &["vue"],
        install: InstallSource::Npm {
            package: "@vue/language-server",
            bin: "vue-language-server",
            extra_packages: &["typescript"],
        },
        launch: LaunchSpec {
            args: &["--stdio"],
            env: &[],
        },
    },
    RegistryEntry {
        id: "svelte-language-server",
        kinds: &[ToolKind::Lsp],
        languages: &["svelte"],
        install: InstallSource::Npm {
            package: "svelte-language-server",
            bin: "svelteserver",
            extra_packages: &[],
        },
        launch: LaunchSpec {
            args: &["--stdio"],
            env: &[],
        },
    },
    RegistryEntry {
        id: "astro-language-server",
        kinds: &[ToolKind::Lsp],
        languages: &["astro"],
        install: InstallSource::Npm {
            package: "@astrojs/language-server",
            bin: "astro-ls",
            extra_packages: &["typescript"],
        },
        launch: LaunchSpec {
            args: &["--stdio"],
            env: &[],
        },
    },
    RegistryEntry {
        id: "yaml-language-server",
        kinds: &[ToolKind::Lsp],
        languages: &["yaml"],
        install: InstallSource::Npm {
            package: "yaml-language-server",
            bin: "yaml-language-server",
            extra_packages: &[],
        },
        launch: LaunchSpec {
            args: &["--stdio"],
            env: &[],
        },
    },
    RegistryEntry {
        id: "bash-language-server",
        kinds: &[ToolKind::Lsp],
        languages: &["shellscript"],
        install: InstallSource::Npm {
            package: "bash-language-server",
            bin: "bash-language-server",
            extra_packages: &[],
        },
        launch: LaunchSpec {
            args: &["start"],
            env: &[],
        },
    },
    RegistryEntry {
        id: "dockerfile-language-server",
        kinds: &[ToolKind::Lsp],
        languages: &["dockerfile"],
        install: InstallSource::Npm {
            package: "dockerfile-language-server-nodejs",
            bin: "docker-langserver",
            extra_packages: &[],
        },
        launch: LaunchSpec {
            args: &["--stdio"],
            env: &[],
        },
    },
    RegistryEntry {
        id: "pyright",
        kinds: &[ToolKind::Lsp],
        languages: &["python"],
        install: InstallSource::Npm {
            package: "pyright",
            bin: "pyright-langserver",
            extra_packages: &[],
        },
        launch: LaunchSpec {
            args: &["--stdio"],
            env: &[],
        },
    },
    RegistryEntry {
        id: "emmet-ls",
        kinds: &[ToolKind::Lsp],
        languages: &["html", "css", "scss", "less"],
        install: InstallSource::Npm {
            package: "emmet-ls",
            bin: "emmet-ls",
            extra_packages: &[],
        },
        launch: LaunchSpec {
            args: &["--stdio"],
            env: &[],
        },
    },

    // ─── LSPs (pip) ───────────────────────────────────────────────────────
    RegistryEntry {
        id: "python-lsp-server",
        kinds: &[ToolKind::Lsp],
        languages: &["python"],
        install: InstallSource::Pip {
            package: "python-lsp-server",
            bin: "pylsp",
            extra_packages: &[],
        },
        launch: LaunchSpec {
            args: &[],
            env: &[],
        },
    },

    // ─── LSPs (cargo) ─────────────────────────────────────────────────────
    RegistryEntry {
        id: "taplo",
        kinds: &[ToolKind::Lsp, ToolKind::Formatter],
        languages: &["toml"],
        install: InstallSource::Cargo {
            crate_name: "taplo-cli",
            bin: "taplo",
        },
        launch: LaunchSpec {
            args: &["lsp", "stdio"],
            env: &[],
        },
    },

    // ─── LSPs (go) ────────────────────────────────────────────────────────
    RegistryEntry {
        id: "gopls",
        kinds: &[ToolKind::Lsp],
        languages: &["go"],
        install: InstallSource::Go {
            module: "golang.org/x/tools/gopls",
            bin: "gopls",
        },
        launch: LaunchSpec {
            args: &[],
            env: &[],
        },
    },

    // ─── LSPs (github release) ────────────────────────────────────────────
    RegistryEntry {
        id: "rust-analyzer",
        kinds: &[ToolKind::Lsp],
        languages: &["rust"],
        install: InstallSource::GithubRelease {
            repo: "rust-lang/rust-analyzer",
            assets: &[
                GithubAsset {
                    target: Target::LinuxX64Gnu,
                    file: "rust-analyzer-x86_64-unknown-linux-gnu.gz",
                    bin: "rust-analyzer-x86_64-unknown-linux-gnu",
                },
                GithubAsset {
                    target: Target::LinuxArm64Gnu,
                    file: "rust-analyzer-aarch64-unknown-linux-gnu.gz",
                    bin: "rust-analyzer-aarch64-unknown-linux-gnu",
                },
                GithubAsset {
                    target: Target::LinuxX64Musl,
                    file: "rust-analyzer-x86_64-unknown-linux-musl.gz",
                    bin: "rust-analyzer-x86_64-unknown-linux-musl",
                },
                GithubAsset {
                    target: Target::DarwinX64,
                    file: "rust-analyzer-x86_64-apple-darwin.gz",
                    bin: "rust-analyzer-x86_64-apple-darwin",
                },
                GithubAsset {
                    target: Target::DarwinArm64,
                    file: "rust-analyzer-aarch64-apple-darwin.gz",
                    bin: "rust-analyzer-aarch64-apple-darwin",
                },
                GithubAsset {
                    target: Target::WinX64,
                    file: "rust-analyzer-x86_64-pc-windows-msvc.zip",
                    bin: "rust-analyzer.exe",
                },
                GithubAsset {
                    target: Target::WinArm64,
                    file: "rust-analyzer-aarch64-pc-windows-msvc.zip",
                    bin: "rust-analyzer.exe",
                },
            ],
        },
        launch: LaunchSpec {
            args: &[],
            env: &[],
        },
    },

    // ─── Formatters (npm) ─────────────────────────────────────────────────
    RegistryEntry {
        id: "prettier",
        kinds: &[ToolKind::Formatter],
        languages: &[
            "typescript", "typescriptreact", "javascript", "javascriptreact",
            "vue", "svelte", "astro",
            "html", "css", "scss", "less",
            "json", "jsonc", "yaml",
            "markdown", "mdx", "graphql",
        ],
        install: InstallSource::Npm {
            package: "prettier",
            bin: "prettier",
            extra_packages: &[],
        },
        launch: LaunchSpec {
            args: &[],
            env: &[],
        },
    },
    RegistryEntry {
        id: "biome",
        kinds: &[ToolKind::Formatter, ToolKind::Linter],
        languages: &[
            "typescript", "typescriptreact", "javascript", "javascriptreact",
            "json", "jsonc", "css",
        ],
        install: InstallSource::Npm {
            package: "@biomejs/biome",
            bin: "biome",
            extra_packages: &[],
        },
        launch: LaunchSpec {
            args: &[],
            env: &[],
        },
    },

    // ─── Formatters (pip) ─────────────────────────────────────────────────
    RegistryEntry {
        id: "black",
        kinds: &[ToolKind::Formatter],
        languages: &["python"],
        install: InstallSource::Pip {
            package: "black",
            bin: "black",
            extra_packages: &[],
        },
        launch: LaunchSpec {
            args: &[],
            env: &[],
        },
    },
    RegistryEntry {
        id: "autopep8",
        kinds: &[ToolKind::Formatter],
        languages: &["python"],
        install: InstallSource::Pip {
            package: "autopep8",
            bin: "autopep8",
            extra_packages: &[],
        },
        launch: LaunchSpec {
            args: &[],
            env: &[],
        },
    },
    RegistryEntry {
        id: "isort",
        kinds: &[ToolKind::Formatter],
        languages: &["python"],
        install: InstallSource::Pip {
            package: "isort",
            bin: "isort",
            extra_packages: &[],
        },
        launch: LaunchSpec {
            args: &[],
            env: &[],
        },
    },
    RegistryEntry {
        id: "ruff",
        kinds: &[ToolKind::Formatter, ToolKind::Linter],
        languages: &["python"],
        install: InstallSource::Pip {
            package: "ruff",
            bin: "ruff",
            extra_packages: &[],
        },
        launch: LaunchSpec {
            args: &[],
            env: &[],
        },
    },

    // ─── Formatters (cargo) ───────────────────────────────────────────────
    RegistryEntry {
        id: "stylua",
        kinds: &[ToolKind::Formatter],
        languages: &["lua"],
        install: InstallSource::Cargo {
            crate_name: "stylua",
            bin: "stylua",
        },
        launch: LaunchSpec {
            args: &[],
            env: &[],
        },
    },

    // ─── Linters (npm) ────────────────────────────────────────────────────
    RegistryEntry {
        id: "eslint",
        kinds: &[ToolKind::Linter],
        languages: &[
            "typescript", "typescriptreact", "javascript", "javascriptreact",
            "vue", "svelte", "astro",
        ],
        install: InstallSource::Npm {
            package: "eslint",
            bin: "eslint",
            extra_packages: &[],
        },
        launch: LaunchSpec {
            args: &[],
            env: &[],
        },
    },
    RegistryEntry {
        id: "markdownlint",
        kinds: &[ToolKind::Linter],
        languages: &["markdown", "mdx"],
        install: InstallSource::Npm {
            package: "markdownlint-cli",
            bin: "markdownlint",
            extra_packages: &[],
        },
        launch: LaunchSpec {
            args: &[],
            env: &[],
        },
    },
    RegistryEntry {
        id: "stylelint",
        kinds: &[ToolKind::Linter],
        languages: &["css", "scss", "less"],
        install: InstallSource::Npm {
            package: "stylelint",
            bin: "stylelint",
            extra_packages: &[],
        },
        launch: LaunchSpec {
            args: &[],
            env: &[],
        },
    },

    // ─── Linters (pip) ────────────────────────────────────────────────────
    RegistryEntry {
        id: "pylint",
        kinds: &[ToolKind::Linter],
        languages: &["python"],
        install: InstallSource::Pip {
            package: "pylint",
            bin: "pylint",
            extra_packages: &[],
        },
        launch: LaunchSpec {
            args: &[],
            env: &[],
        },
    },
    RegistryEntry {
        id: "mypy",
        kinds: &[ToolKind::Linter],
        languages: &["python"],
        install: InstallSource::Pip {
            package: "mypy",
            bin: "mypy",
            extra_packages: &[],
        },
        launch: LaunchSpec {
            args: &[],
            env: &[],
        },
    },
    RegistryEntry {
        id: "yamllint",
        kinds: &[ToolKind::Linter],
        languages: &["yaml"],
        install: InstallSource::Pip {
            package: "yamllint",
            bin: "yamllint",
            extra_packages: &[],
        },
        launch: LaunchSpec {
            args: &[],
            env: &[],
        },
    },
];

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
    ENTRIES.iter().filter(move |e| {
        e.kinds.contains(&kind) && e.languages.iter().any(|l| *l == language)
    })
}
