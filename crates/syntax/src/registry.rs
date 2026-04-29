use std::collections::HashMap;
use std::sync::Arc;

use gpui::{App, BorrowAppContext, Global};
use language::LanguageId;

use crate::grammar::Grammar;

/// Workspace-global cache of loaded grammars, keyed by [`LanguageId`].
/// Grammars are compiled lazily on first request and held forever, so each
/// language pays its compilation cost exactly once per session. Languages
/// that have no grammar entry get cached as `None` so we don't keep retrying
/// the build on every render.
#[derive(Default)]
pub struct SyntaxRegistry {
    grammars: HashMap<LanguageId, Option<Arc<Grammar>>>,
}

impl SyntaxRegistry {
    pub fn install(cx: &mut App) {
        cx.set_global(Self::default());
    }

    /// Return the grammar for `language`, building it on first request.
    /// `None` means we don't ship a grammar for that language — callers
    /// should fall back to plain text.
    pub fn load(language: &LanguageId, cx: &mut App) -> Option<Arc<Grammar>> {
        if let Some(cached) = cx
            .try_global::<Self>()
            .and_then(|s| s.grammars.get(language).cloned())
        {
            return cached;
        }
        let built = build_grammar(language).map(Arc::new);
        cx.update_global::<Self, _>(|store, _| {
            store.grammars.insert(language.clone(), built.clone());
        });
        built
    }
}

impl Global for SyntaxRegistry {}

fn build_grammar(language: &LanguageId) -> Option<Grammar> {
    match language.as_str() {
        "rust" => Grammar::new(
            tree_sitter_rust::LANGUAGE.into(),
            tree_sitter_rust::HIGHLIGHTS_QUERY,
        )
        .ok(),

        // ─── JS / TS family ─────────────────────────────────────────────
        // tree-sitter-javascript ships JS captures + a separate JSX overlay;
        // concatenating them gives JSX files the same JS captures plus the
        // tag/attribute-specific ones. We strip JS's blanket "any uppercase
        // identifier is `@constructor`" rule because it makes import lists
        // and `instanceof Foo` checks render with mixed colors based purely
        // on capitalization (see `strip_js_uppercase_constructor_rule`). For
        // JSX, the upstream overlay only tags lowercase element names; we
        // extend it via `JSX_UPPERCASE_TAG_QUERY` so React components share
        // the same tag color as HTML elements.
        "javascript" => Grammar::new(
            tree_sitter_javascript::LANGUAGE.into(),
            &strip_js_uppercase_constructor_rule(tree_sitter_javascript::HIGHLIGHT_QUERY),
        )
        .ok(),
        "javascriptreact" => Grammar::new(
            tree_sitter_javascript::LANGUAGE.into(),
            &format!(
                "{}\n{}\n{}",
                strip_js_uppercase_constructor_rule(tree_sitter_javascript::HIGHLIGHT_QUERY),
                tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
                JSX_UPPERCASE_TAG_QUERY,
            ),
        )
        .ok(),
        // tree-sitter-typescript's highlights.scm only contains type-specific
        // patterns; by convention it inherits the rest from tree-sitter-
        // javascript. We concatenate the two so identifiers, function calls,
        // booleans, etc. all light up the same way they do in JS files. TSX
        // additionally pulls in the JSX overlay for tag/attribute captures.
        "typescript" => Grammar::new(
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            &format!(
                "{}\n{}",
                strip_js_uppercase_constructor_rule(tree_sitter_javascript::HIGHLIGHT_QUERY),
                strip_ts_uppercase_type_rule(tree_sitter_typescript::HIGHLIGHTS_QUERY),
            ),
        )
        .ok(),
        "typescriptreact" => Grammar::new(
            tree_sitter_typescript::LANGUAGE_TSX.into(),
            &format!(
                "{}\n{}\n{}\n{}",
                strip_js_uppercase_constructor_rule(tree_sitter_javascript::HIGHLIGHT_QUERY),
                tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
                JSX_UPPERCASE_TAG_QUERY,
                strip_ts_uppercase_type_rule(tree_sitter_typescript::HIGHLIGHTS_QUERY),
            ),
        )
        .ok(),

        // ─── Systems ────────────────────────────────────────────────────
        "go" => Grammar::new(
            tree_sitter_go::LANGUAGE.into(),
            tree_sitter_go::HIGHLIGHTS_QUERY,
        )
        .ok(),
        "c" => Grammar::new(
            tree_sitter_c::LANGUAGE.into(),
            tree_sitter_c::HIGHLIGHT_QUERY,
        )
        .ok(),
        // Same inheritance trick as TypeScript: tree-sitter-cpp adds C++-only
        // patterns and expects the C base captures underneath.
        "cpp" => Grammar::new(
            tree_sitter_cpp::LANGUAGE.into(),
            &format!(
                "{}\n{}",
                tree_sitter_c::HIGHLIGHT_QUERY,
                tree_sitter_cpp::HIGHLIGHT_QUERY,
            ),
        )
        .ok(),

        // ─── Scripting ──────────────────────────────────────────────────
        "python" => Grammar::new(
            tree_sitter_python::LANGUAGE.into(),
            tree_sitter_python::HIGHLIGHTS_QUERY,
        )
        .ok(),
        "shellscript" => Grammar::new(
            tree_sitter_bash::LANGUAGE.into(),
            tree_sitter_bash::HIGHLIGHT_QUERY,
        )
        .ok(),

        // ─── Markup / styles ────────────────────────────────────────────
        "html" => Grammar::new(
            tree_sitter_html::LANGUAGE.into(),
            tree_sitter_html::HIGHLIGHTS_QUERY,
        )
        .and_then(|g| g.with_injections(tree_sitter_html::INJECTIONS_QUERY))
        .ok(),
        "css" => Grammar::new(
            tree_sitter_css::LANGUAGE.into(),
            tree_sitter_css::HIGHLIGHTS_QUERY,
        )
        .ok(),
        // SVG is XML — same grammar, different language id so the file shows
        // up as "SVG" in the UI. Other XML dialects (xsd/xsl/xslt/rss/atom/
        // plist) map directly onto the `xml` id in the language crate.
        "xml" | "svg" => Grammar::new(
            tree_sitter_xml::LANGUAGE_XML.into(),
            tree_sitter_xml::XML_HIGHLIGHT_QUERY,
        )
        .ok(),

        // ─── Data / config ──────────────────────────────────────────────
        // jsonc/json5 reuse the JSON grammar — close enough for highlighting
        // until/unless we ship dedicated grammars for the trailing-comma
        // dialects.
        "json" | "jsonc" | "json5" => Grammar::new(
            tree_sitter_json::LANGUAGE.into(),
            tree_sitter_json::HIGHLIGHTS_QUERY,
        )
        .ok(),
        "yaml" => Grammar::new(
            tree_sitter_yaml::LANGUAGE.into(),
            tree_sitter_yaml::HIGHLIGHTS_QUERY,
        )
        .ok(),
        "toml" => Grammar::new(
            tree_sitter_toml_ng::LANGUAGE.into(),
            tree_sitter_toml_ng::HIGHLIGHTS_QUERY,
        )
        .ok(),

        // ─── Docs ───────────────────────────────────────────────────────
        // Markdown ships two grammars: block-level structure and inline
        // formatting (bold/italic/links/etc). The block grammar's injection
        // query points `(inline)` regions at the inline grammar, plus
        // fenced code blocks at the language identified by the info string.
        "markdown" => Grammar::new(
            tree_sitter_md::LANGUAGE.into(),
            tree_sitter_md::HIGHLIGHT_QUERY_BLOCK,
        )
        .and_then(|g| g.with_injections(tree_sitter_md::INJECTION_QUERY_BLOCK))
        .ok(),
        // Internal language — never assigned by file extension; loaded
        // exclusively via the markdown block grammar's injection query.
        "markdown_inline" => Grammar::new(
            tree_sitter_md::INLINE_LANGUAGE.into(),
            tree_sitter_md::HIGHLIGHT_QUERY_INLINE,
        )
        .and_then(|g| g.with_injections(tree_sitter_md::INJECTION_QUERY_INLINE))
        .ok(),

        _ => None,
    }
}

/// Remove tree-sitter-typescript's blanket "any uppercase identifier is a
/// `@type`" pattern. Tree-sitter's later-pattern-wins precedence makes that
/// rule clobber `@function` for things like `function Layout(...)`, where
/// the JS query has already correctly classified `Layout` as a function via
/// the `function_declaration` shape. The dedicated `(type_identifier) @type`
/// and `(predefined_type) @type.builtin` patterns still fire — they're
/// position-aware and don't have this collision.
fn strip_ts_uppercase_type_rule(source: &str) -> String {
    const RULE: &str = "((identifier) @type\n (#match? @type \"^[A-Z]\"))";
    source.replace(RULE, "")
}

/// Remove tree-sitter-javascript's blanket "any uppercase identifier is a
/// `@constructor`" pattern. Without this, import lists render with mixed
/// colors — lowercase names get `@variable` (light blue) but uppercase names
/// get `@constructor` (teal), producing the inconsistent look users notice.
/// JSX components legitimately want the constructor color; we re-add that as
/// a JSX-scoped rule below ([`JSX_UPPERCASE_CONSTRUCTOR_QUERY`]).
fn strip_js_uppercase_constructor_rule(source: &str) -> String {
    const RULE: &str = "((identifier) @constructor\n (#match? @constructor \"^[A-Z]\"))";
    source.replace(RULE, "")
}

/// Tag uppercase JSX element names alongside the lowercase ones the upstream
/// overlay already covers. The upstream restricts `@tag` to `^[a-z]` so React
/// components like `<MyComponent />` would otherwise fall through to
/// `@variable`; this gives both HTML-style and component-style elements the
/// same tag color so a JSX block reads as a single visual unit.
const JSX_UPPERCASE_TAG_QUERY: &str = r#"
(jsx_opening_element (identifier) @tag (#match? @tag "^[A-Z]"))
(jsx_closing_element (identifier) @tag (#match? @tag "^[A-Z]"))
(jsx_self_closing_element (identifier) @tag (#match? @tag "^[A-Z]"))
"#;
