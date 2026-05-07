use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::sync::Arc;

use file_editor::TextEdit;
use highlight::HighlightId;
use language::LanguageId;
use tree_sitter::{
    InputEdit, Node, Parser, Point as TsPoint, Query, QueryCursor, StreamingIterator, Tree,
};

use crate::grammar::Grammar;
use crate::highlight::HighlightSpan;

/// Hard ceiling on the per-snapshot highlight cache. Sized to cover ~20+
/// viewports of scroll history before any eviction happens. Memory cost per
/// entry is small (a `Vec<HighlightSpan>` for one line range), so we'd rather
/// keep more than re-walk the parse tree.
const HIGHLIGHT_CACHE_LIMIT: usize = 2048;

/// One injected sub-tree: a region of the buffer that should be parsed by a
/// different grammar than the main one (e.g. `<script>` content as
/// JavaScript, fenced markdown code blocks as their stated language). Built
/// once per parse cycle by [`crate::SyntaxStore`] and queried by
/// [`SyntaxSnapshot::highlights`] alongside the main tree.
pub(crate) struct Injection {
    /// Byte range in the original buffer that this injection covers. Used to
    /// quickly skip injections that don't intersect the renderer's request.
    pub(crate) range: Range<usize>,
    pub(crate) grammar: Arc<Grammar>,
    pub(crate) tree: Tree,
}

/// Per-buffer parse state. One entity exists per open buffer (held by
/// [`crate::SyntaxStore`]) and is updated in place as the buffer emits
/// `Edited` and `LanguageChanged` events. The snapshot does not own the
/// buffer's text — callers pass `content` into [`Self::highlights`] — so it
/// stays small and safe to clone the tree handle independently if needed.
///
/// `cache` memoizes the result of [`Self::highlights`] keyed by byte range,
/// since the renderer typically asks for the same line ranges on every
/// frame while the parse tree is unchanged. The cache is cleared whenever
/// the parse state changes (new tree, new grammar, edited).
pub struct SyntaxSnapshot {
    language: Option<LanguageId>,
    grammar: Option<Arc<Grammar>>,
    tree: Option<Tree>,
    injections: Vec<Injection>,
    cache: RefCell<HighlightCache>,
}

/// Bounded FIFO cache for [`SyntaxSnapshot::highlights`]. Eviction drops the
/// oldest insertion only — replacing the previous all-or-nothing
/// `HashMap::clear` once the limit is hit, which used to flush the entire
/// hot set on every scroll past the boundary.
///
/// The renderer asks for the same line ranges every frame at a given scroll
/// position, so FIFO behaves like LRU here: entries naturally retire as the
/// user scrolls into new ranges.
#[derive(Default)]
struct HighlightCache {
    entries: HashMap<(usize, usize), Arc<[HighlightSpan]>>,
    order: VecDeque<(usize, usize)>,
}

impl HighlightCache {
    fn get(&self, key: &(usize, usize)) -> Option<Arc<[HighlightSpan]>> {
        self.entries.get(key).cloned()
    }

    fn insert(&mut self, key: (usize, usize), value: Arc<[HighlightSpan]>) {
        if self.entries.insert(key, value).is_some() {
            return;
        }
        self.order.push_back(key);
        while self.entries.len() > HIGHLIGHT_CACHE_LIMIT {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            } else {
                break;
            }
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }
}

impl SyntaxSnapshot {
    pub fn new(language: Option<LanguageId>, grammar: Option<Arc<Grammar>>) -> Self {
        Self {
            language,
            grammar,
            tree: None,
            injections: Vec::new(),
            cache: RefCell::new(HighlightCache::default()),
        }
    }

    pub fn language(&self) -> Option<&LanguageId> {
        self.language.as_ref()
    }

    pub fn grammar(&self) -> Option<&Arc<Grammar>> {
        self.grammar.as_ref()
    }

    pub fn tree(&self) -> Option<&Tree> {
        self.tree.as_ref()
    }

    /// Replace the parse tree and the set of injected sub-trees. Called by
    /// the store whenever a parse cycle finishes, so the two always update
    /// atomically — the renderer never sees a fresh main tree paired with
    /// stale injection trees.
    pub(crate) fn set_tree_and_injections(&mut self, tree: Tree, injections: Vec<Injection>) {
        self.tree = Some(tree);
        self.injections = injections;
        self.cache.borrow_mut().clear();
    }

    /// Replace the active grammar (typically because the buffer's language
    /// changed) and discard the cached parse state. The store is responsible
    /// for kicking off a fresh parse against the new grammar afterwards.
    pub fn set_grammar(&mut self, language: Option<LanguageId>, grammar: Option<Arc<Grammar>>) {
        self.language = language;
        self.grammar = grammar;
        self.tree = None;
        self.injections.clear();
        self.cache.borrow_mut().clear();
    }

    /// Apply the buffer's edit deltas to the existing tree so subsequent
    /// reparses can start from a structurally-aware starting point. The
    /// reparse itself happens in [`crate::SyntaxStore`] off the main thread.
    pub fn apply_edits(&mut self, edits: &[TextEdit]) {
        let Some(tree) = self.tree.as_mut() else {
            return;
        };
        for edit in edits {
            tree.edit(&to_input_edit(edit));
        }
        // Injection trees track ranges into the original buffer; on edits
        // they're easier to drop and rebuild on the next parse than to keep
        // in sync.
        self.injections.clear();
        self.cache.borrow_mut().clear();
    }

    /// Highlight spans intersecting `byte_range`. Returns spans from the main
    /// parse tree merged with spans from any injection that overlaps the
    /// range — so e.g. a single line that's inside a `<script>` tag gets
    /// both HTML's punctuation captures and JavaScript's keyword/identifier
    /// captures, with overlap resolution sorting them out per byte.
    ///
    /// Memoized per byte range while the parse state is unchanged. A typical
    /// editor frame asks for this once per visible line; without the cache
    /// each idle re-render walks the tree N times for nothing.
    /// Returns spans pre-sorted by `(specificity, pattern_index)` so callers
    /// can apply them last-wins without re-sorting on every frame. Result is
    /// cheap-to-clone (`Arc<[..]>`); cache hits are an atomic refcount bump
    /// rather than a Vec copy.
    pub fn highlights(&self, content: &str, byte_range: Range<usize>) -> Arc<[HighlightSpan]> {
        let key = (byte_range.start, byte_range.end);
        if let Some(cached) = self.cache.borrow().get(&key) {
            return cached;
        }
        let spans = self.compute_highlights(content, byte_range);
        self.cache.borrow_mut().insert(key, spans.clone());
        spans
    }

    fn compute_highlights(&self, content: &str, byte_range: Range<usize>) -> Arc<[HighlightSpan]> {
        let mut spans = Vec::new();
        if let (Some(grammar), Some(tree)) = (self.grammar.as_ref(), self.tree.as_ref()) {
            collect_highlights(
                &grammar.highlights_query,
                tree.root_node(),
                content,
                byte_range.clone(),
                &mut spans,
            );
        }
        for inj in &self.injections {
            if inj.range.end <= byte_range.start || inj.range.start >= byte_range.end {
                continue;
            }
            let sub_range =
                byte_range.start.max(inj.range.start)..byte_range.end.min(inj.range.end);
            if sub_range.start >= sub_range.end {
                continue;
            }
            collect_highlights(
                &inj.grammar.highlights_query,
                inj.tree.root_node(),
                content,
                sub_range,
                &mut spans,
            );
        }
        // Pre-sort here so renderers can do last-wins per byte without
        // re-sorting on every cache hit.
        spans.sort_by_key(|s| (s.specificity, s.pattern_index));
        spans.into()
    }
}

fn collect_highlights(
    query: &Query,
    root: Node,
    content: &str,
    byte_range: Range<usize>,
    out: &mut Vec<HighlightSpan>,
) {
    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(byte_range);
    let names = query.capture_names();
    let mut matches = cursor.matches(query, root, content.as_bytes());
    while let Some(m) = matches.next() {
        let pattern_index = m.pattern_index;
        for capture in m.captures {
            let name = names[capture.index as usize];
            let Some(id) = capture_name_to_id(name) else {
                continue;
            };
            out.push(HighlightSpan {
                range: capture.node.byte_range(),
                id,
                pattern_index,
                specificity: name.matches('.').count() as u8,
            });
        }
    }
}

/// Parse a standalone snippet and return its syntax highlight spans. Used for
/// small transient documents such as LSP hover fenced code blocks where wiring
/// a full buffer-backed [`SyntaxSnapshot`] would be unnecessary overhead.
pub fn highlight_content(grammar: &Grammar, content: &str) -> Vec<HighlightSpan> {
    let mut parser = Parser::new();
    if parser.set_language(&grammar.language).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(content, None) else {
        return Vec::new();
    };

    let mut spans = Vec::new();
    collect_highlights(
        &grammar.highlights_query,
        tree.root_node(),
        content,
        0..content.len(),
        &mut spans,
    );
    spans.sort_by_key(|s| (s.specificity, s.pattern_index));
    spans
}

fn to_input_edit(edit: &TextEdit) -> InputEdit {
    InputEdit {
        start_byte: edit.start_byte,
        old_end_byte: edit.old_end_byte,
        new_end_byte: edit.new_end_byte,
        start_position: TsPoint::new(edit.start_point.row, edit.start_point.column),
        old_end_position: TsPoint::new(edit.old_end_point.row, edit.old_end_point.column),
        new_end_position: TsPoint::new(edit.new_end_point.row, edit.new_end_point.column),
    }
}

/// Map a tree-sitter capture name (e.g. `"function.method"`) to our shared
/// [`HighlightId`] vocabulary. Unknown names get dropped silently — adding a
/// new variant means extending both this match and the theme palette.
fn capture_name_to_id(name: &str) -> Option<HighlightId> {
    match name {
        "attribute" => Some(HighlightId::Attribute),
        "boolean" => Some(HighlightId::Boolean),
        "comment" | "comment.documentation" => Some(HighlightId::Comment),
        "constant" | "constant.builtin" => Some(HighlightId::Constant),
        "constructor" => Some(HighlightId::Constructor),
        "escape" | "string.escape" => Some(HighlightId::Escape),
        "function" | "function.builtin" | "function.special" | "function.call" | "method"
        | "method.call" => Some(HighlightId::Function),
        "function.macro" => Some(HighlightId::FunctionMacro),
        "function.method" | "function.method.builtin" => Some(HighlightId::Method),
        // CSS at-rule keywords (`@import`, `@media`, …) and the misc.
        // `keyword.*` / synonym captures used by Lua / Zig / Kotlin / Svelte
        // — all collapse to plain Keyword from the user's point of view.
        "keyword"
        | "keyword.function"
        | "keyword.operator"
        | "keyword.return"
        | "keyword.conditional"
        | "keyword.coroutine"
        | "keyword.exception"
        | "keyword.import"
        | "keyword.modifier"
        | "keyword.repeat"
        | "keyword.type"
        | "conditional"
        | "exception"
        | "include"
        | "repeat"
        | "import"
        | "media"
        | "keyframes"
        | "supports"
        | "charset" => Some(HighlightId::Keyword),
        "label" => Some(HighlightId::Label),
        // `module` / `module.builtin` (Zig / PHP) sit in our Namespace slot.
        "namespace" | "module" | "module.builtin" => Some(HighlightId::Namespace),
        "number" | "number.float" | "float" => Some(HighlightId::Number),
        "operator" => Some(HighlightId::Operator),
        // `field` (Lua) and `variable.member` (Zig) are the same concept as
        // our Property highlight — record/struct field access.
        "property" | "field" | "variable.member" => Some(HighlightId::Property),
        "punctuation"
        | "punctuation.bracket"
        | "punctuation.delimiter"
        | "punctuation.special"
        | "delimiter"
        | "tag.delimiter" => Some(HighlightId::Punctuation),
        "string" | "string.special" | "string.regex" | "string.special.regex"
        // Character literals (Zig / Kotlin) render at the same color as
        // strings — they're effectively single-char string literals.
        | "character" => Some(HighlightId::String),
        // JSON keys (and YAML/TOML scalars) — render with the property color
        // so they read as identifiers rather than as string literals.
        "string.special.key" => Some(HighlightId::Property),
        "tag" | "tag.error" => Some(HighlightId::Tag),
        "type" => Some(HighlightId::Type),
        "type.builtin" => Some(HighlightId::TypeBuiltin),
        "variable" | "variable.builtin" => Some(HighlightId::Variable),
        "variable.parameter" | "parameter" => Some(HighlightId::Parameter),
        // Markdown markup classes. Italic / bold styling will require setting
        // `font_style` / `font_weight` on the `HighlightStyle` — for now we
        // only color them, which is enough to make markdown legible. Both
        // the legacy `@text.*` and modern `@markup.*` capture conventions
        // are accepted so grammars on either standard light up the same way.
        "text.title" | "markup.heading" => Some(HighlightId::MarkupHeading),
        "text.literal" | "markup.raw" => Some(HighlightId::MarkupCode),
        "text.reference" | "text.uri" | "markup.link" => Some(HighlightId::MarkupLink),
        "text.emphasis" | "markup.italic" => Some(HighlightId::MarkupEmphasis),
        "text.strong" | "markup.bold" => Some(HighlightId::MarkupStrong),
        _ => None,
    }
}
