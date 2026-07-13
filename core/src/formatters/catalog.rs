use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FormatterDefinition {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) languages: &'static [&'static str],
    pub(crate) language_ids: &'static [&'static str],
    pub(crate) extensions: &'static [&'static str],
    pub(crate) filenames: &'static [&'static str],
    pub(crate) version: &'static str,
    pub(crate) source: FormatterSource,
    pub(crate) executable: &'static str,
    pub(crate) invocation: FormatterInvocation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FormatterSource {
    Npm {
        package: &'static str,
        integrity: &'static str,
    },
    Portable(&'static [FormatterArtifact]),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FormatterArtifact {
    pub(crate) operating_system: &'static str,
    pub(crate) architecture: &'static str,
    pub(crate) url: &'static str,
    pub(crate) sha256: &'static str,
    pub(crate) format: ArtifactFormat,
    pub(crate) executable_path: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ArtifactFormat {
    Raw,
    TarGzip,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FormatterInvocation {
    Prettier,
    Ruff,
    Shfmt,
}

const PRETTIER_EXTENSIONS: &[&str] = &[
    ".component.html",
    ".css",
    ".wxss",
    ".js.flow",
    ".graphql",
    ".gql",
    ".graphqls",
    ".handlebars",
    ".hbs",
    ".html",
    ".hta",
    ".htm",
    ".html.hl",
    ".inc",
    ".xht",
    ".xhtml",
    ".js",
    "._js",
    ".bones",
    ".cjs",
    ".es",
    ".es6",
    ".gs",
    ".jake",
    ".javascript",
    ".jsb",
    ".jscad",
    ".jsfl",
    ".jslib",
    ".jsm",
    ".jspre",
    ".jss",
    ".mjs",
    ".njs",
    ".pac",
    ".sjs",
    ".ssjs",
    ".xsjs",
    ".xsjslib",
    ".start.frag",
    ".end.frag",
    ".wxs",
    ".json",
    ".4DForm",
    ".4DProject",
    ".avsc",
    ".geojson",
    ".gltf",
    ".har",
    ".ice",
    ".JSON-tmLanguage",
    ".json.example",
    ".mcmeta",
    ".sarif",
    ".slnlaunch",
    ".tact",
    ".tfstate",
    ".tfstate.backup",
    ".topojson",
    ".webapp",
    ".webmanifest",
    ".yy",
    ".yyp",
    ".jsonc",
    ".code-snippets",
    ".code-workspace",
    ".sublime-build",
    ".sublime-color-scheme",
    ".sublime-commands",
    ".sublime-completions",
    ".sublime-keymap",
    ".sublime-macro",
    ".sublime-menu",
    ".sublime-mousemap",
    ".sublime-project",
    ".sublime-settings",
    ".sublime-theme",
    ".sublime-workspace",
    ".sublime_metrics",
    ".sublime_session",
    ".importmap",
    ".json5",
    ".jsx",
    ".less",
    ".md",
    ".livemd",
    ".markdown",
    ".mdown",
    ".mdwn",
    ".mkd",
    ".mkdn",
    ".mkdown",
    ".ronn",
    ".scd",
    ".workbook",
    ".mdx",
    ".mjml",
    ".pcss",
    ".postcss",
    ".scss",
    ".tsx",
    ".ts",
    ".cts",
    ".mts",
    ".vue",
    ".yml",
    ".mir",
    ".reek",
    ".rviz",
    ".sublime-syntax",
    ".syntax",
    ".yaml",
    ".yaml-tmlanguage",
    ".yaml.sed",
    ".yml.mysql",
];

const PRETTIER_FILENAMES: &[&str] = &[
    "Jakefile",
    "start.frag",
    "end.frag",
    ".all-contributorsrc",
    ".arcconfig",
    ".auto-changelog",
    ".c8rc",
    ".htmlhintrc",
    ".imgbotconfig",
    ".nycrc",
    ".tern-config",
    ".tern-project",
    ".watchmanconfig",
    ".babelrc",
    ".jscsrc",
    ".jshintrc",
    ".jslintrc",
    ".swcrc",
    "package.json",
    "package-lock.json",
    "composer.json",
    "contents.lr",
    "README",
    ".clang-format",
    ".clang-tidy",
    ".clangd",
    ".gemrc",
    "CITATION.cff",
    "glide.lock",
    "pixi.lock",
    ".prettierrc",
    ".stylelintrc",
    ".lintstagedrc",
];

const RUFF_ARTIFACTS: &[FormatterArtifact] = &[
    FormatterArtifact {
        operating_system: "linux",
        architecture: "x86_64",
        url: "https://github.com/astral-sh/ruff/releases/download/0.15.21/ruff-x86_64-unknown-linux-gnu.tar.gz",
        sha256: "7ddba1886f39ba918587f9ca37de9651008726834811c19ee83991705bd3e56b",
        format: ArtifactFormat::TarGzip,
        executable_path: "ruff-x86_64-unknown-linux-gnu/ruff",
    },
    FormatterArtifact {
        operating_system: "linux",
        architecture: "aarch64",
        url: "https://github.com/astral-sh/ruff/releases/download/0.15.21/ruff-aarch64-unknown-linux-gnu.tar.gz",
        sha256: "9846136be7fe5b70351d5bde22fd21d4b3ab55b07c9793fdf190040b296ee9a3",
        format: ArtifactFormat::TarGzip,
        executable_path: "ruff-aarch64-unknown-linux-gnu/ruff",
    },
];

const SHFMT_ARTIFACTS: &[FormatterArtifact] = &[
    FormatterArtifact {
        operating_system: "linux",
        architecture: "x86_64",
        url: "https://github.com/mvdan/sh/releases/download/v3.13.1/shfmt_v3.13.1_linux_amd64",
        sha256: "fb096c5d1ac6beabbdbaa2874d025badb03ee07929f0c9ff67563ce8c75398b1",
        format: ArtifactFormat::Raw,
        executable_path: "shfmt_v3.13.1_linux_amd64",
    },
    FormatterArtifact {
        operating_system: "linux",
        architecture: "aarch64",
        url: "https://github.com/mvdan/sh/releases/download/v3.13.1/shfmt_v3.13.1_linux_arm64",
        sha256: "32d92acaa5cd8abb29fc49dac123dc412442d5713967819d8af2c29f1b3857c7",
        format: ArtifactFormat::Raw,
        executable_path: "shfmt_v3.13.1_linux_arm64",
    },
];

const CATALOG: &[FormatterDefinition] = &[
    FormatterDefinition {
        id: "prettier",
        name: "Prettier",
        description: "Opinionated formatting for web, config, and documentation files.",
        languages: &[
            "TypeScript",
            "TSX",
            "JavaScript",
            "JSX",
            "Flow",
            "JSON",
            "JSON5",
            "JSONC",
            "CSS",
            "PostCSS",
            "SCSS",
            "Less",
            "HTML",
            "Angular",
            "Vue",
            "Lightning Web Components",
            "Handlebars",
            "Glimmer",
            "MJML",
            "YAML",
            "Markdown",
            "MDX",
            "GraphQL",
        ],
        language_ids: &[
            "typescript",
            "typescriptreact",
            "javascript",
            "javascriptreact",
            "flow",
            "json",
            "json5",
            "jsonc",
            "css",
            "postcss",
            "scss",
            "less",
            "html",
            "angular",
            "vue",
            "lwc",
            "handlebars",
            "glimmer",
            "mjml",
            "yaml",
            "markdown",
            "mdx",
            "graphql",
        ],
        extensions: PRETTIER_EXTENSIONS,
        filenames: PRETTIER_FILENAMES,
        version: "3.9.4",
        source: FormatterSource::Npm {
            package: "prettier@3.9.4",
            integrity: "sha512-yWG/o/4oJfo036EKAfK6ACAoDOfHeRHx4tuxkfBZiauURiaSmYwlpOr5LQqKtIkRD2z1PLteme2WoxEnj4tHTg==",
        },
        executable: "node_modules/.bin/prettier",
        invocation: FormatterInvocation::Prettier,
    },
    FormatterDefinition {
        id: "ruff",
        name: "Ruff",
        description: "Fast, project-aware formatting for Python files.",
        languages: &["Python"],
        language_ids: &["python"],
        extensions: &[".py", ".pyi", ".pyw"],
        filenames: &[],
        version: "0.15.21",
        source: FormatterSource::Portable(RUFF_ARTIFACTS),
        executable: "ruff",
        invocation: FormatterInvocation::Ruff,
    },
    FormatterDefinition {
        id: "shfmt",
        name: "shfmt",
        description: "Formatting for POSIX shell, Bash, mksh, and Zsh scripts.",
        languages: &["Shell", "Bash", "Zsh", "mksh"],
        language_ids: &["shell"],
        extensions: &[".sh", ".bash", ".bats", ".zsh", ".mksh"],
        filenames: &[
            ".bashrc",
            ".bash_profile",
            ".bash_login",
            ".bash_logout",
            ".zshrc",
        ],
        version: "3.13.1",
        source: FormatterSource::Portable(SHFMT_ARTIFACTS),
        executable: "shfmt",
        invocation: FormatterInvocation::Shfmt,
    },
];

pub fn formatter_catalog() -> &'static [FormatterDefinition] {
    CATALOG
}

pub(crate) fn formatter_definition(id: &str) -> Option<&'static FormatterDefinition> {
    CATALOG.iter().find(|definition| definition.id == id)
}

pub(crate) fn current_artifact(
    definition: &FormatterDefinition,
) -> Option<&'static FormatterArtifact> {
    let FormatterSource::Portable(artifacts) = definition.source else {
        return None;
    };
    artifacts.iter().find(|artifact| {
        artifact.operating_system == std::env::consts::OS
            && artifact.architecture == std::env::consts::ARCH
    })
}

pub(crate) fn formatter_applies(
    definition: &FormatterDefinition,
    language_id: &str,
    relative_path: &Path,
) -> bool {
    definition.language_ids.contains(&language_id) || definition.matches_path(relative_path)
}

impl FormatterDefinition {
    fn matches_path(&self, relative_path: &Path) -> bool {
        let Some(filename) = relative_path.file_name().and_then(|name| name.to_str()) else {
            return false;
        };
        self.filenames.contains(&filename)
            || self
                .extensions
                .iter()
                .any(|extension| filename.ends_with(extension))
    }

    pub fn id(&self) -> &str {
        self.id
    }

    pub fn name(&self) -> &str {
        self.name
    }

    pub fn description(&self) -> &str {
        self.description
    }

    pub fn languages(&self) -> &[&str] {
        self.languages
    }

    pub fn extensions(&self) -> &[&str] {
        self.extensions
    }

    pub fn filenames(&self) -> &[&str] {
        self.filenames
    }

    pub fn version(&self) -> &str {
        self.version
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn catalog_is_pinned_and_order_is_deterministic() {
        assert_eq!(
            formatter_catalog()
                .iter()
                .map(FormatterDefinition::id)
                .collect::<Vec<_>>(),
            vec!["prettier", "ruff", "shfmt"]
        );
        for definition in formatter_catalog() {
            assert!(!definition.version().is_empty());
            match definition.source {
                FormatterSource::Npm { package, integrity } => {
                    assert!(package.ends_with(definition.version));
                    assert!(integrity.starts_with("sha512-"));
                }
                FormatterSource::Portable(artifacts) => assert!(artifacts.iter().all(|artifact| {
                    artifact.url.starts_with("https://")
                        && artifact.sha256.len() == 64
                        && artifact.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
                })),
            }
        }
    }

    #[test]
    fn all_prettier_builtin_extensions_route_without_a_language_match() {
        let prettier = formatter_definition("prettier").unwrap();
        assert_eq!(
            PRETTIER_EXTENSIONS
                .iter()
                .copied()
                .collect::<HashSet<_>>()
                .len(),
            PRETTIER_EXTENSIONS.len()
        );
        for extension in PRETTIER_EXTENSIONS {
            let path = format!("nested/example{extension}");
            assert!(
                formatter_applies(prettier, "plaintext", Path::new(&path)),
                "{path} did not route to Prettier"
            );
        }
    }

    #[test]
    fn applicability_uses_language_ids_extensions_and_exact_filenames() {
        let prettier = formatter_definition("prettier").unwrap();
        assert!(formatter_applies(
            prettier,
            "typescript",
            Path::new("source.unknown")
        ));
        assert!(formatter_applies(
            prettier,
            "plaintext",
            Path::new("config/package.json")
        ));
        assert!(formatter_applies(
            prettier,
            "plaintext",
            Path::new("types/index.d.ts")
        ));
        assert!(!formatter_applies(
            prettier,
            "plaintext",
            Path::new("config/PACKAGE.JSON")
        ));
        assert!(!formatter_applies(
            prettier,
            "plaintext",
            Path::new("notes.txt")
        ));
    }
}
