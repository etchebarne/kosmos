use std::ops::Range;
use std::path::Path;

use gpui::{
    AnyElement, App, Context, DragMoveEvent, Entity, HighlightStyle, IntoElement,
    ListHorizontalSizingBehavior, Pixels, Point, SharedString, StyledText, div, list, prelude::*,
    px, rems, uniform_list,
};

use file_editor::{Buffer, BufferStore, EditorView, EditorViewStore, soft_wrap_enabled};
use file_tree::ActiveFileTree;
use highlight::HighlightId;
use icons::{Icon, IconName};
use syntax::{SyntaxSnapshot, SyntaxStore};
use tabs::{Tab, registry};
use theme::{ActiveTheme, SyntaxStyles, Theme};

use crate::components::scrollbar::{self, EditorScrollMetrics, ScrollbarDrag};

const GUTTER_WIDTH_REM: f32 = 3.5;
const GUTTER_PADDING_REM: f32 = 0.5;
const BODY_PADDING_LEFT_REM: f32 = 0.75;
const FONT_FAMILY: &str = "DejaVu Sans Mono";
/// Fixed row height. Pinning this lets `uniform_list::measure_item` return a
/// stable row height regardless of how it lays out our flex_row at
/// MinContent — otherwise the reported content size jitters between renders.
const ROW_HEIGHT_REM: f32 = 1.4;

pub fn render<T: 'static>(tab: &Tab, cx: &mut Context<T>) -> AnyElement {
    let Some(path) = tab.path.clone() else {
        return missing_path(cx);
    };
    let theme = *cx.theme();
    let file_tree_root = cx
        .file_tree()
        .cloned()
        .and_then(|tree| tree.read(cx).root().map(Path::to_path_buf));
    let breadcrumb = render_breadcrumb(&path, file_tree_root.as_deref(), theme);
    let buffer = BufferStore::open(path, cx);
    let view = EditorViewStore::for_tab(tab.id, &buffer, cx);
    let snapshot = SyntaxStore::for_buffer(&buffer, cx);
    observe_snapshot(&view, &snapshot, cx);
    let soft_wrap = soft_wrap_enabled(cx);
    let (line_count, row_count, longest_idx) = {
        let buf = buffer.read(cx);
        (buf.line_count(), buf.row_count(), buf.longest_line_index())
    };

    let body: AnyElement = if soft_wrap {
        let list_state = view.update(cx, |v, _| v.list_state_for(true));
        let buffer_for_render = buffer.clone();
        let snapshot_for_render = snapshot.clone();
        list(list_state, move |index, _window, cx| {
            if index >= line_count {
                return render_spacer_row(px(0.0), *cx.theme()).into_any_element();
            }
            let theme = *cx.theme();
            let (line, spans) = line_with_spans(&buffer_for_render, &snapshot_for_render, index, cx);
            // Soft wrap can't scroll horizontally, so the gutter is never
            // sticky — its offset is always 0.
            render_row(index + 1, line, spans, soft_wrap, px(0.0), &theme).into_any_element()
        })
        .size_full()
        .into_any_element()
    } else {
        let scroll = view.read(cx).uniform_scroll();
        let buffer_for_render = buffer.clone();
        let view_for_render = view.clone();
        let snapshot_for_render = snapshot.clone();
        uniform_list(
            "file-editor-lines",
            row_count,
            move |range, _window, cx| {
                let theme = *cx.theme();
                // Negate the list's current x scroll so the gutter overlay
                // shifts back to the viewport's left edge as content scrolls
                // past it horizontally — i.e. position: sticky on x only.
                let sticky_offset = -view_for_render
                    .read(cx)
                    .uniform_scroll()
                    .0
                    .borrow()
                    .base_handle
                    .offset()
                    .x;
                range
                    .map(|i| {
                        if i >= line_count {
                            return render_spacer_row(sticky_offset, theme)
                                .into_any_element();
                        }
                        let (line, spans) =
                            line_with_spans(&buffer_for_render, &snapshot_for_render, i, cx);
                        render_row(i + 1, line, spans, soft_wrap, sticky_offset, &theme)
                            .into_any_element()
                    })
                    .collect()
            },
        )
        .size_full()
        .track_scroll(scroll)
        // Let the longest line drive the horizontal extent so shift+wheel
        // scrolls past the widest content, not just past line 0's width.
        .with_width_from_item(Some(longest_idx))
        .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
        .into_any_element()
    };

    let view_owner = view.entity_id();
    // Sibling overlay (not a uniform_list decoration): decorations are
    // positioned at the scrolled origin, so their visible area shrinks as
    // the user scrolls down. A sibling absolute child of the editor's
    // outer wrapper stays fixed to the viewport.
    let scrollbar_overlay = scrollbar::render(current_metrics(&view, soft_wrap, cx), view_owner, cx);

    let view_for_drag = view.clone();
    let editor_area = div()
        .relative()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .text_sm()
        .font_family(FONT_FAMILY)
        .child(body)
        .child(scrollbar_overlay)
        .on_drag_move(cx.listener(
            move |_, event: &DragMoveEvent<ScrollbarDrag>, _window, cx| {
                let drag = *event.drag(cx);
                // gpui fires on_drag_move on every listener of this drag
                // type, so each side-by-side editor would otherwise scroll
                // when any of them is dragged. Ignore drags that didn't
                // start in this editor's own scrollbar.
                if drag.owner() != view_owner {
                    return;
                }
                let metrics = current_metrics(&view_for_drag, soft_wrap, cx);
                match drag {
                    ScrollbarDrag::Vertical(_) => {
                        let Some(axis) = metrics.vertical else { return };
                        let mouse_y = event.event.position.y - event.bounds.top();
                        let new_scroll = axis.scroll_for_mouse_position(mouse_y);
                        set_scroll_y(&view_for_drag, soft_wrap, new_scroll, cx);
                    }
                    ScrollbarDrag::Horizontal(_) => {
                        let Some(axis) = metrics.horizontal else { return };
                        let mouse_x = event.event.position.x - event.bounds.left();
                        let new_scroll = axis.scroll_for_mouse_position(mouse_x);
                        set_scroll_x(&view_for_drag, new_scroll, cx);
                    }
                }
                cx.notify();
            },
        ));

    div()
        .size_full()
        .min_h_0()
        .min_w_0()
        .flex()
        .flex_col()
        .bg(theme.bg_surface)
        .text_color(theme.text)
        .child(breadcrumb)
        .child(editor_area)
        .into_any_element()
}

fn current_metrics(view: &Entity<EditorView>, soft_wrap: bool, cx: &App) -> EditorScrollMetrics {
    let v = view.read(cx);
    if soft_wrap {
        EditorScrollMetrics::from_list(&v.list_state_snapshot())
    } else {
        EditorScrollMetrics::from_uniform(&v.uniform_scroll())
    }
}

fn set_scroll_y(view: &Entity<EditorView>, soft_wrap: bool, scrolled: Pixels, cx: &App) {
    let v = view.read(cx);
    if soft_wrap {
        v.list_state_snapshot()
            .set_offset_from_scrollbar(Point::new(px(0.0), -scrolled));
    } else {
        let handle = v.uniform_scroll();
        let state = handle.0.borrow();
        let current = state.base_handle.offset();
        state
            .base_handle
            .set_offset(Point::new(current.x, -scrolled));
    }
}

fn set_scroll_x(view: &Entity<EditorView>, scrolled: Pixels, cx: &App) {
    let v = view.read(cx);
    let handle = v.uniform_scroll();
    let state = handle.0.borrow();
    let current = state.base_handle.offset();
    state
        .base_handle
        .set_offset(Point::new(-scrolled, current.y));
}

fn render_breadcrumb(path: &Path, root: Option<&Path>, theme: Theme) -> AnyElement {
    let segments = breadcrumb_segments(path, root);
    if segments.is_empty() {
        return div().flex_none().into_any_element();
    }
    let last_idx = segments.len() - 1;
    let file_icon = file_icon_for_path(path);

    let mut row = div()
        .flex()
        .flex_none()
        .flex_row()
        .items_center()
        .w_full()
        .min_w_0()
        .px(rems(0.75))
        .py(rems(0.375))
        .gap(rems(0.25))
        .text_xs()
        .text_color(theme.text_subtle)
        .overflow_hidden()
        .whitespace_nowrap();

    for (i, seg) in segments.into_iter().enumerate() {
        if i > 0 {
            row = row.child(
                Icon::new(IconName::ChevronRight)
                    .size(12.0)
                    .color(theme.text_subtle),
            );
        }
        if i == last_idx {
            row = row.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(rems(0.25))
                    .child(Icon::new(file_icon).size(14.0).color(theme.text_muted))
                    .child(div().text_color(theme.text_muted).child(seg)),
            );
        } else {
            row = row.child(div().child(seg));
        }
    }

    row.into_any_element()
}

fn breadcrumb_segments(path: &Path, root: Option<&Path>) -> Vec<SharedString> {
    if let Some(root) = root
        && let Ok(relative) = path.strip_prefix(root)
    {
        return relative
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(s) => {
                    s.to_str().map(|s| SharedString::from(s.to_string()))
                }
                _ => None,
            })
            .collect();
    }
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|s| vec![SharedString::from(s.to_string())])
        .unwrap_or_default()
}

fn file_icon_for_path(path: &Path) -> IconName {
    if let Some(name) = path.file_name().and_then(|n| n.to_str())
        && let Some(icon) = IconName::for_file_name(name)
    {
        return icon;
    }
    language::from_path(path)
        .and_then(|id| IconName::for_language(id.as_str()))
        .unwrap_or(IconName::File)
}

/// Return the text of `line_index` in `buffer` along with the highlight
/// spans for that line, translated to be relative to the line's start so
/// they can be passed straight into [`StyledText::with_highlights`]. Spans
/// that overlap or share bytes are reduced to a non-overlapping last-wins
/// sequence — tree-sitter queries can emit nested or repeated captures over
/// the same range, but gpui's run builder requires a clean sequence.
fn line_with_spans(
    buffer: &Entity<Buffer>,
    snapshot: &Entity<SyntaxSnapshot>,
    line_index: usize,
    cx: &App,
) -> (SharedString, Vec<(Range<usize>, HighlightId)>) {
    let buf = buffer.read(cx);
    let Some(line_range) = buf.line_range(line_index) else {
        return (SharedString::default(), Vec::new());
    };
    let line_text: SharedString = buf.content()[line_range.clone()]
        .to_string()
        .into();
    let raw = snapshot
        .read(cx)
        .highlights(buf.content(), line_range.clone());
    let spans = clip_spans_to_line(&line_text, line_range.start, raw);
    (line_text, spans)
}

/// Reduce `raw_spans` (in absolute buffer byte offsets, possibly overlapping
/// or nested as tree-sitter emits) to a non-overlapping list of
/// `(line-relative-range, id)` tuples. Spans are sorted by
/// `(specificity, pattern_index)` ascending and applied last-wins per byte —
/// so a more-dotted capture name (`@string.special.key`) beats a less-dotted
/// one (`@string`) regardless of pattern position, and at equal specificity
/// later patterns (e.g. the JSX overlay) override earlier ones. Ranges are
/// truncated to char boundaries so gpui doesn't panic laying the runs out.
fn clip_spans_to_line(
    line: &str,
    line_byte_start: usize,
    mut raw_spans: Vec<syntax::HighlightSpan>,
) -> Vec<(Range<usize>, HighlightId)> {
    let len = line.len();
    if len == 0 {
        return Vec::new();
    }
    raw_spans.sort_by_key(|s| (s.specificity, s.pattern_index));
    let mut bytes_id: Vec<Option<HighlightId>> = vec![None; len];
    for span in raw_spans {
        let start = span.range.start.saturating_sub(line_byte_start);
        let end = span.range.end.saturating_sub(line_byte_start);
        let start = start.min(len);
        let end = end.min(len);
        for slot in &mut bytes_id[start..end] {
            *slot = Some(span.id);
        }
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < len {
        if !line.is_char_boundary(i) {
            i += 1;
            continue;
        }
        let id = bytes_id[i];
        let mut j = i + 1;
        while j < len && bytes_id[j] == id {
            j += 1;
        }
        while j < len && !line.is_char_boundary(j) {
            j += 1;
        }
        if let Some(id) = id {
            out.push((i..j, id));
        }
        i = j;
    }
    out
}

/// Wire `view` to re-render whenever `snapshot` notifies (e.g. when the
/// initial parse completes). Idempotent across renders by stashing the
/// observed entity id on the editor view — without that gate we'd attach a
/// new observer every frame.
fn observe_snapshot<T: 'static>(
    view: &Entity<EditorView>,
    snapshot: &Entity<SyntaxSnapshot>,
    cx: &mut Context<T>,
) {
    let snapshot_id = snapshot.entity_id();
    if view.read(cx).observed_external() == Some(snapshot_id) {
        return;
    }
    view.update(cx, |v, _| v.set_observed_external(snapshot_id));
    cx.observe(snapshot, |_, _, cx| cx.notify()).detach();
}

fn render_spacer_row(sticky_offset: Pixels, theme: Theme) -> AnyElement {
    div()
        .relative()
        .w_full()
        .h(rems(ROW_HEIGHT_REM))
        .child(render_gutter(None, sticky_offset, theme))
        .into_any_element()
}

fn render_row(
    line_number: usize,
    line: SharedString,
    spans: Vec<(Range<usize>, HighlightId)>,
    soft_wrap: bool,
    sticky_offset: Pixels,
    theme: &Theme,
) -> impl IntoElement {
    div()
        .relative()
        .w_full()
        // Soft-wrap mode lets rows grow vertically to fit wrapped lines, so
        // we only fix the row height for the non-wrap path.
        .when(!soft_wrap, |this| this.h(rems(ROW_HEIGHT_REM)))
        .line_height(rems(ROW_HEIGHT_REM))
        .child(
            // Reserve left space for the gutter overlay so the line text
            // never starts underneath it.
            div()
                .w_full()
                .pl(rems(GUTTER_WIDTH_REM + BODY_PADDING_LEFT_REM))
                .when(!soft_wrap, |this| this.whitespace_nowrap())
                .child(render_line_text(line, spans, &theme.syntax)),
        )
        .child(render_gutter(Some(line_number), sticky_offset, *theme))
}

/// Build the styled text element for a line, lifting the highlight spans into
/// gpui [`HighlightStyle`] runs. Falls back to plain text when there are no
/// spans (no grammar, parse not finished, or this line has no captures).
fn render_line_text(
    line: SharedString,
    spans: Vec<(Range<usize>, HighlightId)>,
    syntax: &SyntaxStyles,
) -> AnyElement {
    if spans.is_empty() {
        return div().child(line).into_any_element();
    }
    let highlights = spans.into_iter().map(|(range, id)| {
        let color = syntax.color(id);
        (
            range,
            HighlightStyle {
                color: Some(color.into()),
                ..Default::default()
            },
        )
    });
    StyledText::new(line)
        .with_highlights(highlights)
        .into_any_element()
}

fn render_gutter(
    line_number: Option<usize>,
    sticky_offset: Pixels,
    theme: Theme,
) -> impl IntoElement {
    let label: SharedString = match line_number {
        Some(n) => format!("{n}").into(),
        None => SharedString::default(),
    };
    div()
        .absolute()
        .top(rems(0.0))
        .left(sticky_offset)
        .h(rems(ROW_HEIGHT_REM))
        .w(rems(GUTTER_WIDTH_REM))
        .pr(rems(GUTTER_PADDING_REM))
        .text_right()
        .text_color(theme.text_subtle)
        .bg(theme.bg_surface)
        .child(label)
}

fn missing_path<T: 'static>(cx: &mut Context<T>) -> AnyElement {
    let theme = *cx.theme();
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .text_color(theme.text_subtle)
        .child(
            Icon::new(registry::FILE_EDITOR.icon)
                .size(32.0)
                .color(theme.text_muted),
        )
        .child(div().text_sm().child("No file"))
        .into_any_element()
}
