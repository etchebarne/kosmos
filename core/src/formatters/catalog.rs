#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FormatterDefinition {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) languages: &'static [&'static str],
    pub(crate) language_ids: &'static [&'static str],
    pub(crate) version: &'static str,
    pub(crate) npm_package: &'static str,
    pub(crate) npm_integrity: &'static str,
    pub(crate) executable: &'static str,
}

const CATALOG: &[FormatterDefinition] = &[FormatterDefinition {
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
    version: "3.9.4",
    npm_package: "prettier@3.9.4",
    npm_integrity: "sha512-yWG/o/4oJfo036EKAfK6ACAoDOfHeRHx4tuxkfBZiauURiaSmYwlpOr5LQqKtIkRD2z1PLteme2WoxEnj4tHTg==",
    executable: "node_modules/.bin/prettier",
}];

pub fn formatter_catalog() -> &'static [FormatterDefinition] {
    CATALOG
}

pub(crate) fn formatter_definition(id: &str) -> Option<&'static FormatterDefinition> {
    CATALOG.iter().find(|definition| definition.id == id)
}

pub(crate) fn formatters_for_language(
    language_id: &str,
) -> impl Iterator<Item = &'static FormatterDefinition> {
    CATALOG
        .iter()
        .filter(move |definition| definition.language_ids.contains(&language_id))
}

impl FormatterDefinition {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_pinned_and_language_order_is_deterministic() {
        assert_eq!(formatter_catalog().len(), 1);
        let prettier = &formatter_catalog()[0];
        assert_eq!(prettier.id(), "prettier");
        assert_eq!(prettier.version(), "3.9.4");
        assert!(prettier.npm_package.ends_with("@3.9.4"));
        assert!(prettier.npm_integrity.starts_with("sha512-"));
        assert_eq!(
            formatters_for_language("typescript")
                .map(FormatterDefinition::id)
                .collect::<Vec<_>>(),
            vec!["prettier"]
        );
    }
}
