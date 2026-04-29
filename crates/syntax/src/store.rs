use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use file_editor::{Buffer, BufferEvent, BufferId};
use gpui::{App, AppContext, BorrowAppContext, Entity, Global};
use language::LanguageId;
use tree_sitter::{Parser, QueryCursor, StreamingIterator, Tree};

use crate::grammar::Grammar;
use crate::registry::SyntaxRegistry;
use crate::snapshot::{Injection, SyntaxSnapshot};

/// Workspace-global cache of [`SyntaxSnapshot`]s, keyed by [`BufferId`]. The
/// snapshot for a buffer is created on first request and subscribes to that
/// buffer's events, so subsequent edits or language changes flow into the
/// snapshot automatically — callers fetch it once and can ignore lifecycle.
#[derive(Default)]
pub struct SyntaxStore {
    snapshots: HashMap<BufferId, Entity<SyntaxSnapshot>>,
}

impl SyntaxStore {
    pub fn install(cx: &mut App) {
        cx.set_global(Self::default());
    }

    /// Return the snapshot for `buffer`, creating it (and kicking off the
    /// initial parse on a background thread) if it doesn't exist yet.
    pub fn for_buffer(buffer: &Entity<Buffer>, cx: &mut App) -> Entity<SyntaxSnapshot> {
        let id = buffer.read(cx).id();
        if let Some(existing) = cx
            .try_global::<Self>()
            .and_then(|s| s.snapshots.get(&id).cloned())
        {
            return existing;
        }
        let language = buffer.read(cx).language().cloned();
        let grammar = language.as_ref().and_then(|l| SyntaxRegistry::load(l, cx));
        let buffer_for_sub = buffer.clone();
        let language_for_snapshot = language.clone();
        let grammar_for_snapshot = grammar.clone();
        let snapshot = cx.new(move |cx_inner| {
            cx_inner
                .subscribe(
                    &buffer_for_sub,
                    move |this: &mut SyntaxSnapshot, buffer, event, cx| match event {
                        BufferEvent::Edited { edits } => {
                            this.apply_edits(edits);
                            spawn_parse(&buffer, this.grammar().cloned(), this.tree().cloned(), cx);
                        }
                        BufferEvent::LanguageChanged => {
                            let new_language = buffer.read(cx).language().cloned();
                            let new_grammar = new_language
                                .as_ref()
                                .and_then(|l| SyntaxRegistry::load(l, cx));
                            this.set_grammar(new_language, new_grammar.clone());
                            cx.notify();
                            spawn_parse(&buffer, new_grammar, None, cx);
                        }
                    },
                )
                .detach();
            SyntaxSnapshot::new(language_for_snapshot, grammar_for_snapshot)
        });
        cx.update_global::<Self, _>(|store, _| {
            store.snapshots.insert(id, snapshot.clone());
        });
        if grammar.is_some() {
            spawn_parse(buffer, grammar, None, cx);
        }
        snapshot
    }

    /// Drop the cached snapshot for `id`. Call when a buffer is permanently
    /// closed so its parse state isn't held forever.
    pub fn drop_buffer(id: BufferId, cx: &mut App) {
        if cx.try_global::<Self>().is_none() {
            return;
        }
        cx.update_global::<Self, _>(|store, _| {
            store.snapshots.remove(&id);
        });
    }
}

impl Global for SyntaxStore {}

/// Kick off a parse of `buffer` against its current grammar on a background
/// thread, then route the resulting `Tree` (plus any injections it
/// describes) back into the snapshot on the main thread. Cheap to call — if
/// there's no grammar or no snapshot, this turns into a few `Option`
/// lookups and exits.
fn spawn_parse(
    buffer: &Entity<Buffer>,
    grammar: Option<Arc<Grammar>>,
    old_tree: Option<Tree>,
    cx: &mut App,
) {
    let Some(grammar) = grammar else {
        return;
    };
    let id = buffer.read(cx).id();
    let Some(snapshot) = cx
        .try_global::<SyntaxStore>()
        .and_then(|s| s.snapshots.get(&id).cloned())
    else {
        return;
    };
    let content = buffer.read(cx).content().to_string();
    let content_for_injections = content.clone();
    let grammar_for_injections = grammar.clone();
    let snapshot_weak = snapshot.downgrade();
    cx.spawn(async move |cx| {
        // Heavy main parse runs on the background pool so the UI thread
        // never blocks on it. Injection resolution is done back on the main
        // thread because it needs `App` access to look up grammars from the
        // registry — but that work is small (HashMap lookup + parse a few
        // hundred bytes per injection) and not worth a second dispatch.
        let parsed = cx
            .background_executor()
            .spawn(async move { parse(&grammar, &content, old_tree.as_ref()) })
            .await;
        let Some(tree) = parsed else { return };
        let _ = cx.update(|app| {
            let injections =
                resolve_injections(&grammar_for_injections, &tree, &content_for_injections, app);
            let Some(snapshot) = snapshot_weak.upgrade() else {
                return;
            };
            snapshot.update(app, |s, cx| {
                s.set_tree_and_injections(tree, injections);
                cx.notify();
            });
        });
    })
    .detach();
}

fn parse(grammar: &Arc<Grammar>, content: &str, old_tree: Option<&Tree>) -> Option<Tree> {
    let mut parser = Parser::new();
    parser.set_language(&grammar.language).ok()?;
    parser.parse(content, old_tree)
}

/// Walk the main grammar's `injections_query` against `tree` and produce one
/// [`Injection`] per region that should be re-parsed with another grammar.
/// Static `(#set! injection.language "X")` directives win over dynamic
/// `@injection.language` captures (matches tree-sitter's documented
/// precedence). Injections whose language we don't ship a grammar for are
/// silently skipped.
fn resolve_injections(
    grammar: &Arc<Grammar>,
    tree: &Tree,
    content: &str,
    cx: &mut App,
) -> Vec<Injection> {
    let Some(query) = grammar.injections_query.as_ref() else {
        return Vec::new();
    };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), content.as_bytes());
    let names = query.capture_names();
    let mut out = Vec::new();
    while let Some(m) = matches.next() {
        let mut content_ranges: Vec<tree_sitter::Range> = Vec::new();
        let mut dynamic_language: Option<String> = None;
        for cap in m.captures {
            let name = names[cap.index as usize];
            match name {
                "injection.content" => {
                    let r = cap.node.range();
                    if r.start_byte < r.end_byte {
                        content_ranges.push(r);
                    }
                }
                "injection.language" => {
                    let bytes = cap.node.byte_range();
                    dynamic_language = Some(content[bytes].to_string());
                }
                _ => {}
            }
        }
        if content_ranges.is_empty() {
            continue;
        }
        let language_str = static_language(query, m.pattern_index).or(dynamic_language);
        let Some(language_str) = language_str else {
            continue;
        };
        let canonical = normalize_injection_language(&language_str);
        let language_id = LanguageId::new(canonical);
        let Some(injected_grammar) = SyntaxRegistry::load(&language_id, cx) else {
            continue;
        };
        let Some(injected_tree) = parse_included(&injected_grammar, content, &content_ranges)
        else {
            continue;
        };
        let span: Range<usize> =
            content_ranges.first().unwrap().start_byte..content_ranges.last().unwrap().end_byte;
        out.push(Injection {
            range: span,
            grammar: injected_grammar,
            tree: injected_tree,
        });
    }
    out
}

fn static_language(query: &tree_sitter::Query, pattern_index: usize) -> Option<String> {
    for prop in query.property_settings(pattern_index) {
        if &*prop.key == "injection.language" {
            return prop.value.as_ref().map(|v| v.to_string());
        }
    }
    None
}

fn parse_included(
    grammar: &Arc<Grammar>,
    content: &str,
    ranges: &[tree_sitter::Range],
) -> Option<Tree> {
    let mut parser = Parser::new();
    parser.set_language(&grammar.language).ok()?;
    parser.set_included_ranges(ranges).ok()?;
    parser.parse(content, None)
}

/// Map shorthand language identifiers used in markdown fenced-code-block info
/// strings (and similar dynamic injections) to the canonical ids we register
/// in [`SyntaxRegistry`]. Keeps the alias list short — only common cases —
/// because most fenced blocks already use the canonical id.
fn normalize_injection_language(s: &str) -> String {
    let lowered = s.trim().to_ascii_lowercase();
    let canonical: &str = match lowered.as_str() {
        "ts" => "typescript",
        "tsx" => "typescriptreact",
        "js" => "javascript",
        "jsx" => "javascriptreact",
        "py" => "python",
        "rs" => "rust",
        "sh" | "bash" | "zsh" | "shell" => "shellscript",
        "yml" => "yaml",
        "md" => "markdown",
        "c++" | "cxx" | "cc" => "cpp",
        _ => return lowered,
    };
    canonical.to_string()
}
