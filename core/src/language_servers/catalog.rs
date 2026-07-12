#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LanguageServerDefinition {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) languages: &'static [&'static str],
    pub(crate) language_ids: &'static [&'static str],
    pub(crate) root_markers: &'static [&'static str],
    pub(crate) version: &'static str,
    pub(crate) artifacts: &'static [LanguageServerArtifact],
    pub(crate) npm_packages: &'static [NpmPackage],
    pub(crate) executable: &'static str,
    pub(crate) launch_args: &'static [&'static str],
    pub(crate) features: &'static [LanguageToolFeatureDefinition],
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum LanguageToolFeature {
    Completion,
    Hover,
    SignatureHelp,
    Navigation,
    References,
    Symbols,
    Diagnostics,
    Colors,
    Formatting,
    Rename,
    CodeActions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LanguageToolFeatureDefinition {
    language_id: &'static str,
    features: &'static [LanguageToolFeature],
}

const TYPESCRIPT_FEATURES: &[LanguageToolFeature] = &[
    LanguageToolFeature::Completion,
    LanguageToolFeature::Hover,
    LanguageToolFeature::SignatureHelp,
    LanguageToolFeature::Navigation,
    LanguageToolFeature::References,
    LanguageToolFeature::Symbols,
    LanguageToolFeature::Diagnostics,
    LanguageToolFeature::Rename,
    LanguageToolFeature::CodeActions,
];
const JSON_FEATURES: &[LanguageToolFeature] = &[
    LanguageToolFeature::Completion,
    LanguageToolFeature::Hover,
    LanguageToolFeature::Symbols,
    LanguageToolFeature::Diagnostics,
    LanguageToolFeature::Colors,
    LanguageToolFeature::Formatting,
];
const CSS_FEATURES: &[LanguageToolFeature] = &[
    LanguageToolFeature::Completion,
    LanguageToolFeature::Hover,
    LanguageToolFeature::Navigation,
    LanguageToolFeature::References,
    LanguageToolFeature::Symbols,
    LanguageToolFeature::Diagnostics,
    LanguageToolFeature::Colors,
    LanguageToolFeature::Formatting,
    LanguageToolFeature::Rename,
];
const HTML_FEATURES: &[LanguageToolFeature] = &[
    LanguageToolFeature::Completion,
    LanguageToolFeature::Hover,
    LanguageToolFeature::Symbols,
    LanguageToolFeature::Diagnostics,
    LanguageToolFeature::Colors,
    LanguageToolFeature::Formatting,
    LanguageToolFeature::Rename,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LanguageServerArtifact {
    pub(crate) operating_system: &'static str,
    pub(crate) architecture: &'static str,
    pub(crate) url: &'static str,
    pub(crate) sha256: &'static str,
    pub(crate) compression: ArtifactCompression,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NpmPackage {
    pub(crate) spec: &'static str,
    pub(crate) integrity: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ArtifactCompression {
    Gzip,
}

const RUST_ANALYZER_ARTIFACTS: &[LanguageServerArtifact] = &[
    LanguageServerArtifact {
        operating_system: "linux",
        architecture: "x86_64",
        url: "https://github.com/rust-lang/rust-analyzer/releases/download/2026-07-06/rust-analyzer-x86_64-unknown-linux-gnu.gz",
        sha256: "2fb596e12676e512de5dbf1c322dd591127ee089a1cca47995605593f2fc8850",
        compression: ArtifactCompression::Gzip,
    },
    LanguageServerArtifact {
        operating_system: "linux",
        architecture: "aarch64",
        url: "https://github.com/rust-lang/rust-analyzer/releases/download/2026-07-06/rust-analyzer-aarch64-unknown-linux-gnu.gz",
        sha256: "7e2627d96c6f1614115d212b61fd5f8dc9279853054b800f2b023c883e3ae056",
        compression: ArtifactCompression::Gzip,
    },
];

const CATALOG: &[LanguageServerDefinition] = &[
    LanguageServerDefinition {
        id: "rust-analyzer",
        name: "Rust Analyzer",
        description: "Language support for Rust projects.",
        languages: &["Rust"],
        language_ids: &["rust"],
        root_markers: &["Cargo.toml"],
        version: "2026-07-06",
        artifacts: RUST_ANALYZER_ARTIFACTS,
        npm_packages: &[],
        executable: "rust-analyzer",
        launch_args: &[],
        features: &[],
    },
    LanguageServerDefinition {
        id: "typescript-language-server",
        name: "TypeScript Language Server",
        description: "Language support for TypeScript and JavaScript projects.",
        languages: &["TypeScript", "JavaScript"],
        language_ids: &["typescript", "javascript"],
        root_markers: &[
            "tsconfig.json",
            "jsconfig.json",
            "package.json",
            "pnpm-lock.yaml",
            "yarn.lock",
            "package-lock.json",
        ],
        version: "5.3.0",
        artifacts: &[],
        npm_packages: &[
            NpmPackage {
                spec: "typescript-language-server@5.3.0",
                integrity: "sha512-5puofxZHgFdAYtfNpmwCAvgtaYgg8wrUnH30m7Ze3QuguId5RNRadKASpOpyDxTyUdAF51FjhTdjntLw/EuWcQ==",
            },
            NpmPackage {
                spec: "typescript@6.0.3",
                integrity: "sha512-y2TvuxSZPDyQakkFRPZHKFm+KKVqIisdg9/CZwm9ftvKXLP8NRWj38/ODjNbr43SsoXqNuAisEf1GdCxqWcdBw==",
            },
        ],
        executable: "node_modules/.bin/typescript-language-server",
        launch_args: &["--stdio"],
        features: &[
            LanguageToolFeatureDefinition {
                language_id: "typescript",
                features: TYPESCRIPT_FEATURES,
            },
            LanguageToolFeatureDefinition {
                language_id: "javascript",
                features: TYPESCRIPT_FEATURES,
            },
        ],
    },
    LanguageServerDefinition {
        id: "pyright",
        name: "Pyright",
        description: "Type checking and language support for Python projects.",
        languages: &["Python"],
        language_ids: &["python"],
        root_markers: &[
            "pyrightconfig.json",
            "pyproject.toml",
            "setup.py",
            "setup.cfg",
            "requirements.txt",
        ],
        version: "1.1.411",
        artifacts: &[],
        npm_packages: &[NpmPackage {
            spec: "pyright@1.1.411",
            integrity: "sha512-03S/vmS5lF1S/tVbKc2WNXCMq8JWCwta/qIYjj1jvqbQhoy+N3NgBzHTSmUlbYD6DJwqQ5XHf108QujoqeURvw==",
        }],
        executable: "node_modules/.bin/pyright-langserver",
        launch_args: &["--stdio"],
        features: &[],
    },
    LanguageServerDefinition {
        id: "html-language-server",
        name: "HTML Language Server",
        description: "Language support for HTML documents.",
        languages: &["HTML"],
        language_ids: &["html"],
        root_markers: &["package.json"],
        version: "4.10.0",
        artifacts: &[],
        npm_packages: &[NpmPackage {
            spec: "vscode-langservers-extracted@4.10.0",
            integrity: "sha512-EFf9uQI4dAKbzMQFjDvVm1xJq1DXAQvBEuEfPGrK/xzfsL5xWTfIuRr90NgfmqwO+IEt6vLZm9EOj6R66xIifg==",
        }],
        executable: "node_modules/.bin/vscode-html-language-server",
        launch_args: &["--stdio"],
        features: &[LanguageToolFeatureDefinition {
            language_id: "html",
            features: HTML_FEATURES,
        }],
    },
    LanguageServerDefinition {
        id: "css-language-server",
        name: "CSS Language Server",
        description: "Language support for CSS, SCSS, and Less stylesheets.",
        languages: &["CSS", "SCSS", "Less"],
        language_ids: &["css", "scss", "less"],
        root_markers: &["package.json"],
        version: "4.10.0",
        artifacts: &[],
        npm_packages: &[NpmPackage {
            spec: "vscode-langservers-extracted@4.10.0",
            integrity: "sha512-EFf9uQI4dAKbzMQFjDvVm1xJq1DXAQvBEuEfPGrK/xzfsL5xWTfIuRr90NgfmqwO+IEt6vLZm9EOj6R66xIifg==",
        }],
        executable: "node_modules/.bin/vscode-css-language-server",
        launch_args: &["--stdio"],
        features: &[
            LanguageToolFeatureDefinition {
                language_id: "css",
                features: CSS_FEATURES,
            },
            LanguageToolFeatureDefinition {
                language_id: "scss",
                features: CSS_FEATURES,
            },
            LanguageToolFeatureDefinition {
                language_id: "less",
                features: CSS_FEATURES,
            },
        ],
    },
    LanguageServerDefinition {
        id: "json-language-server",
        name: "JSON Language Server",
        description: "Schema-aware language support for JSON documents.",
        languages: &["JSON"],
        language_ids: &["json"],
        root_markers: &["package.json"],
        version: "4.10.0",
        artifacts: &[],
        npm_packages: &[NpmPackage {
            spec: "vscode-langservers-extracted@4.10.0",
            integrity: "sha512-EFf9uQI4dAKbzMQFjDvVm1xJq1DXAQvBEuEfPGrK/xzfsL5xWTfIuRr90NgfmqwO+IEt6vLZm9EOj6R66xIifg==",
        }],
        executable: "node_modules/.bin/vscode-json-language-server",
        launch_args: &["--stdio"],
        features: &[LanguageToolFeatureDefinition {
            language_id: "json",
            features: JSON_FEATURES,
        }],
    },
    LanguageServerDefinition {
        id: "yaml-language-server",
        name: "YAML Language Server",
        description: "Schema-aware language support for YAML documents.",
        languages: &["YAML"],
        language_ids: &["yaml"],
        root_markers: &[],
        version: "1.24.0",
        artifacts: &[],
        npm_packages: &[NpmPackage {
            spec: "yaml-language-server@1.24.0",
            integrity: "sha512-+HGcwu4M7IC+UDhDZScTZR8qsl2MMj/X1E5e83QcWzWn2pctj0fv8HHdrHHcbc1KB3CuRPJ4gc1Nm36D0iCu0g==",
        }],
        executable: "node_modules/.bin/yaml-language-server",
        launch_args: &["--stdio"],
        features: &[],
    },
    LanguageServerDefinition {
        id: "bash-language-server",
        name: "Bash Language Server",
        description: "Language support for Bash and shell scripts.",
        languages: &["Bash", "Shell"],
        language_ids: &["shell"],
        root_markers: &[],
        version: "5.6.0",
        artifacts: &[],
        npm_packages: &[NpmPackage {
            spec: "bash-language-server@5.6.0",
            integrity: "sha512-DCuV+/BZAAozsp5blvi6jDnU/ZDaTpJpWM0zqwGjnirfqv7iBsMK32xOze/jipxU0PUZ6CBUKgRUMKI7Kk70Lg==",
        }],
        executable: "node_modules/.bin/bash-language-server",
        launch_args: &["start"],
        features: &[],
    },
    LanguageServerDefinition {
        id: "tailwindcss-language-server",
        name: "Tailwind CSS Language Server",
        description: "Tailwind CSS class validation and project-aware language support.",
        languages: &["Tailwind CSS"],
        language_ids: &[
            "html",
            "css",
            "scss",
            "less",
            "typescript",
            "javascript",
            "markdown",
            "mdx",
        ],
        root_markers: &[
            "tailwind.config.js",
            "tailwind.config.cjs",
            "tailwind.config.mjs",
            "tailwind.config.ts",
            "tailwind.config.cts",
            "tailwind.config.mts",
            "package.json",
        ],
        version: "0.14.29",
        artifacts: &[],
        npm_packages: &[NpmPackage {
            spec: "@tailwindcss/language-server@0.14.29",
            integrity: "sha512-aZ3/XyTNmsoIyhs09Fghlw6D6y7o70aIxHmQEYPFiJPe/1k3HqtxXqhn7g7a5UpA1yeGOyKK9HRNJ8ghZqIclg==",
        }],
        executable: "node_modules/.bin/tailwindcss-language-server",
        launch_args: &["--stdio"],
        features: &[],
    },
];

pub fn language_server_catalog() -> &'static [LanguageServerDefinition] {
    CATALOG
}

pub(crate) fn language_server_definition(id: &str) -> Option<&'static LanguageServerDefinition> {
    CATALOG.iter().find(|definition| definition.id == id)
}

pub(crate) fn language_servers_for_language(
    language_id: &str,
) -> impl Iterator<Item = &'static LanguageServerDefinition> {
    CATALOG
        .iter()
        .filter(move |definition| definition.language_ids.contains(&language_id))
}

pub(crate) fn current_artifact(
    definition: &LanguageServerDefinition,
) -> Option<&LanguageServerArtifact> {
    let operating_system = std::env::consts::OS;
    let architecture = std::env::consts::ARCH;

    definition.artifacts.iter().find(|artifact| {
        artifact.operating_system == operating_system && artifact.architecture == architecture
    })
}

impl LanguageServerDefinition {
    pub(crate) fn features_for_language(
        &self,
        language_id: &str,
    ) -> &'static [LanguageToolFeature] {
        self.features
            .iter()
            .find(|definition| definition.language_id == language_id)
            .map_or(&[], |definition| definition.features)
    }
    pub(crate) fn protocol_language_id<'a>(
        &self,
        language_id: &'a str,
        relative_path: &str,
    ) -> &'a str {
        if language_id == "typescript" && relative_path.ends_with(".tsx") {
            return "typescriptreact";
        }
        if language_id == "javascript" && relative_path.ends_with(".jsx") {
            return "javascriptreact";
        }
        language_id
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

    pub fn version(&self) -> &str {
        self.version
    }

    pub fn is_supported(&self) -> bool {
        current_artifact(self).is_some() || !self.npm_packages.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn catalog_ids_are_unique_and_stable() {
        let ids = language_server_catalog()
            .iter()
            .map(LanguageServerDefinition::id)
            .collect::<Vec<_>>();
        let unique_ids = ids.iter().copied().collect::<HashSet<_>>();

        assert_eq!(
            ids,
            vec![
                "rust-analyzer",
                "typescript-language-server",
                "pyright",
                "html-language-server",
                "css-language-server",
                "json-language-server",
                "yaml-language-server",
                "bash-language-server",
                "tailwindcss-language-server",
            ]
        );
        assert_eq!(ids.len(), unique_ids.len());
    }

    #[test]
    fn catalog_artifacts_are_pinned_and_have_sha256_digests() {
        for definition in language_server_catalog() {
            assert!(!definition.version().is_empty());
            assert!(!definition.artifacts.is_empty() || !definition.npm_packages.is_empty());
            assert!(definition.artifacts.iter().all(|artifact| {
                artifact.url.starts_with("https://")
                    && artifact.sha256.len() == 64
                    && artifact.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            }));
            assert!(definition.npm_packages.iter().all(|package| {
                package
                    .spec
                    .rsplit_once('@')
                    .is_some_and(|(_, version)| !version.is_empty())
            }));
            assert!(
                definition
                    .npm_packages
                    .iter()
                    .all(|package| package.integrity.starts_with("sha512-"))
            );
        }
    }

    #[test]
    fn overlapping_languages_return_every_matching_server() {
        let ids = language_servers_for_language("typescript")
            .map(LanguageServerDefinition::id)
            .collect::<Vec<_>>();

        assert_eq!(
            ids,
            vec!["typescript-language-server", "tailwindcss-language-server"]
        );
    }

    #[test]
    fn resolved_tooling_catalog_features_preserve_every_existing_editor_mapping() {
        let features = |server, language| {
            language_server_definition(server)
                .unwrap()
                .features_for_language(language)
                .to_vec()
        };

        assert_eq!(
            features("typescript-language-server", "typescript"),
            TYPESCRIPT_FEATURES
        );
        assert_eq!(
            features("typescript-language-server", "javascript"),
            TYPESCRIPT_FEATURES
        );
        assert_eq!(features("json-language-server", "json"), JSON_FEATURES);
        assert_eq!(features("css-language-server", "css"), CSS_FEATURES);
        assert_eq!(features("css-language-server", "scss"), CSS_FEATURES);
        assert_eq!(features("css-language-server", "less"), CSS_FEATURES);
        assert_eq!(features("html-language-server", "html"), HTML_FEATURES);
        assert!(features("tailwindcss-language-server", "typescript").is_empty());
    }
}
