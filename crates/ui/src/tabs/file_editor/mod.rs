use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{
    AnchoredPositionMode, AnyElement, App, Bounds, Context, Corner, DragMoveEvent, Entity,
    FontStyle, FontWeight, HighlightStyle, InteractiveText, IntoElement,
    ListHorizontalSizingBehavior, MouseMoveEvent, Pixels, Point, SharedString, StyledText,
    TextLayout, Window, anchored, deferred, div, point, prelude::*, px, rems, uniform_list,
};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use file_editor::{
    Buffer, BufferStore, EditorHoverStatus, EditorView, EditorViewStore, soft_wrap_enabled,
    virtual_list,
};
use file_tree::ActiveFileTree;
use highlight::HighlightId;
use icons::{Icon, IconName};
use syntax::{SyntaxRegistry, SyntaxSnapshot, SyntaxStore};
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
const HOVER_DEBOUNCE: Duration = Duration::from_millis(500);
const HOVER_HIDE_DELAY: Duration = Duration::from_millis(180);

#[derive(Clone)]
struct LineHover {
    line_index: usize,
    buffer: Entity<Buffer>,
    view: Entity<EditorView>,
    root: Option<PathBuf>,
}

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
        let virtual_state = view.read(cx).virtual_scroll();
        // Snapshot per-line char counts so the height closure doesn't need
        // App context. ~one usize per logical line, doesn't change while
        // the buffer is read-only.
        let line_chars: Vec<usize> = {
            let buf = buffer.read(cx);
            (0..buf.line_count()).map(|i| buf.line_chars(i)).collect()
        };
        // Approximate em width for monospace as 0.6 × font_size. Off-by-10%
        // is fine for wrap-count estimation — VirtualList feeds this height
        // straight into the cumulative table without ever shaping text for
        // non-visible rows, so the scrollbar tracks our estimate exactly.
        let height_fn = move |index: usize, viewport_w: Pixels, rem_size: Pixels| -> Pixels {
            let font_size_px = rems(0.875).to_pixels(rem_size);
            let line_height_px = rems(ROW_HEIGHT_REM).to_pixels(rem_size);
            let em_width = font_size_px * 0.6;
            if index >= line_chars.len() {
                // Bottom spacer rows: fixed single-line height.
                return line_height_px;
            }
            let cpl = if viewport_w > px(0.0) && em_width > px(0.0) {
                ((viewport_w / em_width).floor() as usize).max(1)
            } else {
                80
            };
            let chars = line_chars[index];
            let wraps = ((chars.max(1) + cpl - 1) / cpl).max(1) as f32;
            line_height_px * wraps
        };

        let buffer_for_render = buffer.clone();
        let view_for_render = view.clone();
        let snapshot_for_render = snapshot.clone();
        let root_for_render = file_tree_root.clone();
        virtual_list(
            "file-editor-soft-wrap",
            virtual_state,
            row_count,
            height_fn,
            move |index, _window, cx| {
                if index >= line_count {
                    return render_spacer_row(px(0.0), *cx.theme()).into_any_element();
                }
                let theme = *cx.theme();
                let (line, spans) =
                    line_with_spans(&buffer_for_render, &snapshot_for_render, index, cx);
                // Soft wrap can't scroll horizontally, so the gutter is never
                // sticky — its offset is always 0.
                render_row(
                    index + 1,
                    line,
                    spans,
                    soft_wrap,
                    px(0.0),
                    Some(LineHover {
                        line_index: index,
                        buffer: buffer_for_render.clone(),
                        view: view_for_render.clone(),
                        root: root_for_render.clone(),
                    }),
                    &theme,
                    cx,
                )
                .into_any_element()
            },
        )
        .size_full()
        .into_any_element()
    } else {
        let scroll = view.read(cx).uniform_scroll();
        let buffer_for_render = buffer.clone();
        let view_for_render = view.clone();
        let snapshot_for_render = snapshot.clone();
        let root_for_render = file_tree_root.clone();
        uniform_list("file-editor-lines", row_count, move |range, window, cx| {
            let theme = *cx.theme();
            let view_ref = view_for_render.read(cx);
            let scroll_handle = view_ref.uniform_scroll();
            // Negate the list's current x scroll so the gutter overlay
            // shifts back to the viewport's left edge as content scrolls
            // past it horizontally — i.e. position: sticky on x only.
            let scroll_state = scroll_handle.0.borrow();
            let sticky_offset = -scroll_state.base_handle.offset().x;
            // gpui set this from the previous prepaint's measurement.
            // `contents.width` is `viewport.max(longest_item_width)`, so
            // it only matches the true longest width when the longest
            // line is wider than the viewport — which is the case we
            // care about (long pnpm-lock.yaml integrity hashes etc.).
            let prev_sizes = scroll_state.last_item_size;
            drop(scroll_state);
            let rem_size = window.rem_size();
            if let Some(sizes) = prev_sizes
                && sizes.contents.width > sizes.item.width
            {
                view_ref.set_cached_longest_width(rem_size, sizes.contents.width);
            }
            let cached_longest = view_ref.cached_longest_width(rem_size);
            // Heuristic: gpui's `measure_item` always calls us with a
            // single-element range starting at `longest_idx`. The visible
            // render uses a multi-element range. Treat single-row calls
            // for the longest line as measurement-only and serve a stub.
            let is_longest_measure = range.len() == 1 && range.start == longest_idx;

            range
                .map(|i| {
                    if is_longest_measure && let Some(width) = cached_longest {
                        return render_longest_stub(width, theme).into_any_element();
                    }
                    if i >= line_count {
                        return render_spacer_row(sticky_offset, theme).into_any_element();
                    }
                    let (line, spans) =
                        line_with_spans(&buffer_for_render, &snapshot_for_render, i, cx);
                    render_row(
                        i + 1,
                        line,
                        spans,
                        soft_wrap,
                        sticky_offset,
                        Some(LineHover {
                            line_index: i,
                            buffer: buffer_for_render.clone(),
                            view: view_for_render.clone(),
                            root: root_for_render.clone(),
                        }),
                        &theme,
                        cx,
                    )
                    .into_any_element()
                })
                .collect()
        })
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
    let scrollbar_overlay =
        scrollbar::render(current_metrics(&view, soft_wrap, cx), view_owner, cx);
    let hover_overlay = render_hover_overlay(&view, cx);

    let view_for_drag = view.clone();
    let view_for_mouse = view.clone();
    let editor_area = div()
        .relative()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .text_sm()
        .font_family(FONT_FAMILY)
        // gpui's StyledText reads `white_space` from the window's text-style
        // stack at request_layout time. With the default `Normal`, its layout
        // closure derives `wrap_width = available_width`, which changes on
        // every pane-resize frame and invalidates the per-line shape cache.
        // Pinning nowrap at the editor's outermost layer guarantees nowrap
        // is on the stack before the row elements push their refinements,
        // so resize-driven width changes don't re-shape every visible line.
        .when(!soft_wrap, |this| this.whitespace_nowrap())
        .child(body)
        .child(scrollbar_overlay)
        .child(hover_overlay)
        .on_mouse_move(move |event, window, cx| {
            update_hover_visibility(&view_for_mouse, event, window, cx);
        })
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
                        let Some(axis) = metrics.horizontal else {
                            return;
                        };
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
        EditorScrollMetrics::from_virtual(&v.virtual_scroll())
    } else {
        EditorScrollMetrics::from_uniform(&v.uniform_scroll())
    }
}

fn set_scroll_y(view: &Entity<EditorView>, soft_wrap: bool, scrolled: Pixels, cx: &App) {
    let v = view.read(cx);
    if soft_wrap {
        v.virtual_scroll().set_scroll_y(scrolled);
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
    let line_text: SharedString = buf.content()[line_range.clone()].to_string().into();
    let raw = snapshot
        .read(cx)
        .highlights(buf.content(), line_range.clone());
    let spans = clip_spans_to_line(&line_text, line_range.start, &raw);
    (line_text, spans)
}

/// Reduce `raw_spans` (in absolute buffer byte offsets, pre-sorted by
/// `(specificity, pattern_index)` ascending by [`SyntaxSnapshot::highlights`])
/// to a non-overlapping list of `(line-relative-range, id)` tuples, applying
/// last-wins per byte — so a more-dotted capture name (`@string.special.key`)
/// beats a less-dotted one (`@string`) regardless of pattern position, and at
/// equal specificity later patterns (e.g. the JSX overlay) override earlier
/// ones. Ranges are truncated to char boundaries so gpui doesn't panic laying
/// the runs out.
fn clip_spans_to_line(
    line: &str,
    line_byte_start: usize,
    raw_spans: &[syntax::HighlightSpan],
) -> Vec<(Range<usize>, HighlightId)> {
    let len = line.len();
    if len == 0 {
        return Vec::new();
    }
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

/// Width-and-height-only proxy for the longest line, served to gpui's
/// `measure_item` so it doesn't re-shape the real (potentially 200+ char)
/// line on every prepaint. The width is the previous frame's measured value
/// captured from `UniformListScrollHandle::last_item_size`; height matches
/// the fixed row height. No children are added — taffy returns the declared
/// `width`/`height` directly under MinContent measurement.
fn render_longest_stub(width: Pixels, _theme: Theme) -> impl IntoElement {
    div().w(width).h(rems(ROW_HEIGHT_REM))
}

fn render_row(
    line_number: usize,
    line: SharedString,
    spans: Vec<(Range<usize>, HighlightId)>,
    soft_wrap: bool,
    sticky_offset: Pixels,
    hover: Option<LineHover>,
    theme: &Theme,
    cx: &App,
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
                .child(render_line_text(line, spans, theme, hover, cx)),
        )
        .child(render_gutter(Some(line_number), sticky_offset, *theme))
}

/// Build the styled text element for a line, lifting the highlight spans into
/// gpui `HighlightStyle` runs (color + italic/bold modifiers from the theme).
/// Falls back to plain text when there are no spans (no grammar, parse not
/// finished, or this line has no captures).
fn render_line_text(
    line: SharedString,
    spans: Vec<(Range<usize>, HighlightId)>,
    theme: &Theme,
    hover: Option<LineHover>,
    cx: &App,
) -> AnyElement {
    let source_highlight = hover
        .as_ref()
        .and_then(|hover| hover_source_highlight_range(hover, cx));
    let highlights = line_highlights(line.len(), spans, &theme.syntax, source_highlight, *theme);
    let text = if highlights.is_empty() {
        StyledText::new(line)
    } else {
        StyledText::new(line).with_highlights(highlights)
    };

    let Some(hover) = hover else {
        return text.into_any_element();
    };
    let text_layout = text.layout().clone();
    let hover_for_move = hover.clone();
    let hover_for_prepaint = hover.clone();
    div()
        .child(
            InteractiveText::new(("file-editor-line", hover.line_index), text).on_hover(
                move |byte_index, _event, _window, cx| {
                    if let Some(byte_index) = byte_index {
                        begin_lsp_hover(&hover_for_move, byte_index, cx);
                    } else {
                        schedule_hover_hide(&hover_for_move.view, hover_for_move.line_index, cx);
                    }
                },
            ),
        )
        .on_children_prepainted(move |bounds, window, cx| {
            update_hover_source_bounds(&hover_for_prepaint, &text_layout, bounds, window, cx);
        })
        .id(("file-editor-line-hover", hover.line_index))
        .into_any_element()
}

fn begin_lsp_hover(hover: &LineHover, byte_index: usize, cx: &mut App) {
    let Some((byte_index, byte_range)) = hoverable_target(hover, byte_index, cx) else {
        hover
            .view
            .update(cx, |view, _| view.clear_hover_for_line(hover.line_index));
        cx.refresh_windows();
        return;
    };

    let Some(generation) = hover.view.update(cx, |view, _| {
        view.begin_hover(hover.line_index, byte_index, byte_range)
    }) else {
        return;
    };
    cx.refresh_windows();

    let hover = hover.clone();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(HOVER_DEBOUNCE).await;

        let request = cx
            .update(|cx| build_lsp_hover_request(&hover, generation, cx))
            .ok()
            .flatten();
        let Some(request) = request else {
            let _ = cx.update(|cx| {
                hover.view.update(cx, |view, _| {
                    view.finish_hover(generation, EditorHoverStatus::Empty)
                });
                cx.refresh_windows();
            });
            return;
        };

        let result = cx
            .background_executor()
            .spawn(async move { lsp::hover(request) })
            .await;
        let status = match result {
            Ok(Some(hover)) => EditorHoverStatus::Ready(hover.contents),
            Ok(None) => EditorHoverStatus::Empty,
            Err(err) => EditorHoverStatus::Error(err.to_string()),
        };

        let _ = cx.update(|cx| {
            hover
                .view
                .update(cx, |view, _| view.finish_hover(generation, status));
            cx.refresh_windows();
        });
    })
    .detach();
}

fn schedule_hover_hide(view: &Entity<EditorView>, line_index: usize, cx: &mut App) {
    let Some(hide_generation) =
        view.update(cx, |view, _| view.schedule_hover_hide_for_line(line_index))
    else {
        return;
    };

    let view = view.clone();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(HOVER_HIDE_DELAY).await;
        let _ = cx.update(|cx| {
            view.update(cx, |view, _| {
                view.clear_scheduled_hover(line_index, hide_generation)
            });
            cx.refresh_windows();
        });
    })
    .detach();
}

fn update_hover_visibility(
    view: &Entity<EditorView>,
    event: &MouseMoveEvent,
    window: &mut Window,
    cx: &mut App,
) {
    update_hover_visibility_at(view, event.position, window, cx);
}

fn update_hover_visibility_at(
    view: &Entity<EditorView>,
    position: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(active) = view.read(cx).hover().cloned() else {
        return;
    };
    if matches!(active.status, EditorHoverStatus::Empty) {
        return;
    }

    let Some(source_bounds) = active.source_bounds else {
        return;
    };
    let active_bounds = active
        .popup_bounds
        .map(|popup_bounds| source_bounds.union(&popup_bounds))
        .unwrap_or(source_bounds);
    let gap = rems(0.75).to_pixels(window.rem_size());
    if active_bounds.inset(-gap).contains(&position) {
        view.update(cx, |view, _| {
            view.cancel_hover_hide_for_line(active.line_index)
        });
    } else {
        schedule_hover_hide(view, active.line_index, cx);
    }
}

fn hoverable_target(
    hover: &LineHover,
    byte_index: usize,
    cx: &App,
) -> Option<(usize, Range<usize>)> {
    let buffer = hover.buffer.read(cx);
    let language = buffer.language()?.as_str();
    if !lsp::has_installed_server(language) {
        return None;
    }

    let line = buffer.line(hover.line_index)?;
    symbol_range_at(line, byte_index).map(|range| {
        let byte_index = clamp_to_char_boundary(line, byte_index.min(line.len()));
        let byte_index = if byte_index < range.start || byte_index >= range.end {
            range.start
        } else {
            byte_index
        };
        (byte_index, range)
    })
}

fn symbol_range_at(line: &str, byte_index: usize) -> Option<Range<usize>> {
    let byte_index = clamp_to_char_boundary(line, byte_index.min(line.len()));
    if let Some(range) = string_range_at(line, byte_index) {
        return Some(range);
    }

    let (char_start, ch) = line[byte_index..]
        .chars()
        .next()
        .map(|ch| (byte_index, ch))
        .or_else(|| {
            let (idx, ch) = line[..byte_index].char_indices().next_back()?;
            Some((idx, ch))
        })?;
    if ch.is_whitespace() {
        return None;
    }

    if !is_symbol_char(ch) {
        return None;
    }

    let mut start = char_start;
    for (idx, ch) in line[..char_start].char_indices().rev() {
        if !is_symbol_char(ch) {
            break;
        }
        start = idx;
    }

    let mut end = char_start + ch.len_utf8();
    let forward_base = end;
    for (offset, ch) in line[forward_base..].char_indices() {
        if !is_symbol_char(ch) {
            break;
        }
        end = forward_base + offset + ch.len_utf8();
    }

    Some(start..end)
}

fn string_range_at(line: &str, byte_index: usize) -> Option<Range<usize>> {
    let (char_start, _) = line[byte_index..]
        .chars()
        .next()
        .map(|ch| (byte_index, ch))
        .or_else(|| {
            let (idx, ch) = line[..byte_index].char_indices().next_back()?;
            Some((idx, ch))
        })?;

    let mut start = None;
    let mut in_string = false;
    for (idx, ch) in line.char_indices() {
        if ch != '"' || is_escaped_quote(line, idx) {
            continue;
        }

        if in_string {
            let end = idx + ch.len_utf8();
            if start? <= char_start && char_start < end {
                return Some(start?..end);
            }
            in_string = false;
            start = None;
        } else {
            in_string = true;
            start = Some(idx);
        }
    }

    None
}

fn is_escaped_quote(line: &str, quote_index: usize) -> bool {
    let mut backslashes = 0usize;
    for ch in line[..quote_index].chars().rev() {
        if ch != '\\' {
            break;
        }
        backslashes += 1;
    }
    backslashes % 2 == 1
}

fn is_symbol_char(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_alphanumeric()
}

fn build_lsp_hover_request(
    hover: &LineHover,
    generation: u64,
    cx: &App,
) -> Option<lsp::HoverRequest> {
    let active = hover.view.read(cx).hover()?.clone();
    if active.generation != generation {
        return None;
    }

    let buffer = hover.buffer.read(cx);
    let language_id = buffer.language()?.as_str().to_string();
    if !lsp::has_installed_server(&language_id) {
        return None;
    }

    let line = buffer.line(active.line_index)?;
    let byte_index = clamp_to_char_boundary(line, active.byte_index.min(line.len()));
    let character = utf16_units(&line[..byte_index]) as u32;
    let path = buffer.path().to_path_buf();
    let root = hover
        .root
        .clone()
        .or_else(|| path.parent().map(Path::to_path_buf))?;

    Some(lsp::HoverRequest {
        root,
        path,
        language_id,
        content: buffer.content().to_string(),
        position: lsp::Position {
            line: active.line_index as u32,
            character,
        },
    })
}

fn clamp_to_char_boundary(line: &str, mut byte_index: usize) -> usize {
    byte_index = byte_index.min(line.len());
    while byte_index > 0 && !line.is_char_boundary(byte_index) {
        byte_index -= 1;
    }
    byte_index
}

fn utf16_units(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

fn hover_source_highlight_range(hover: &LineHover, cx: &App) -> Option<Range<usize>> {
    let active = hover.view.read(cx).hover()?;
    if active.line_index == hover.line_index
        && active.source_highlight_visible
        && !matches!(active.status, EditorHoverStatus::Empty)
    {
        Some(active.byte_range.clone())
    } else {
        None
    }
}

fn line_highlights(
    line_len: usize,
    spans: Vec<(Range<usize>, HighlightId)>,
    syntax: &SyntaxStyles,
    source_highlight: Option<Range<usize>>,
    theme: Theme,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let syntax_highlights = spans
        .into_iter()
        .filter_map(|(range, id)| {
            clipped_range(range, line_len).map(|range| (range, syntax.style(id)))
        })
        .collect::<Vec<_>>();
    let source_highlight = source_highlight.and_then(|range| clipped_range(range, line_len));

    if syntax_highlights.is_empty() && source_highlight.is_none() {
        return Vec::new();
    }

    let mut boundaries = Vec::with_capacity(2 + syntax_highlights.len() * 2 + 2);
    boundaries.push(0);
    boundaries.push(line_len);
    for (range, _) in &syntax_highlights {
        boundaries.push(range.start);
        boundaries.push(range.end);
    }
    if let Some(range) = &source_highlight {
        boundaries.push(range.start);
        boundaries.push(range.end);
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let source_style = source_hover_highlight_style(theme);
    let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    for window in boundaries.windows(2) {
        let start = window[0];
        let end = window[1];
        if start == end {
            continue;
        }

        let mut style = HighlightStyle::default();
        for (range, syntax_style) in &syntax_highlights {
            if range.start <= start && end <= range.end {
                style = style.highlight(*syntax_style);
            }
        }
        if source_highlight
            .as_ref()
            .is_some_and(|range| range.start <= start && end <= range.end)
        {
            style = style.highlight(source_style);
        }
        if style == HighlightStyle::default() {
            continue;
        }

        if let Some((last_range, last_style)) = highlights.last_mut()
            && *last_style == style
            && last_range.end == start
        {
            last_range.end = end;
            continue;
        }
        highlights.push((start..end, style));
    }

    highlights
}

fn clipped_range(range: Range<usize>, line_len: usize) -> Option<Range<usize>> {
    let start = range.start.min(line_len);
    let end = range.end.min(line_len);
    (start < end).then_some(start..end)
}

fn source_hover_highlight_style(theme: Theme) -> HighlightStyle {
    HighlightStyle {
        background_color: Some(theme.bg_hover_strong.into()),
        ..Default::default()
    }
}

fn update_hover_source_bounds(
    hover: &LineHover,
    text_layout: &TextLayout,
    bounds: Vec<Bounds<Pixels>>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(source_bounds) = bounds.first().copied() else {
        return;
    };
    let Some(active) = hover.view.read(cx).hover().cloned() else {
        return;
    };
    if active.line_index != hover.line_index || matches!(active.status, EditorHoverStatus::Empty) {
        return;
    }
    let source_bounds = hover_source_bounds(hover, text_layout, source_bounds, &active, cx);
    hover.view.update(cx, |view, _| {
        view.set_hover_source_bounds(hover.line_index, active.byte_range, source_bounds)
    });
    update_hover_visibility_at(&hover.view, window.mouse_position(), window, cx);
}

fn hover_source_bounds(
    hover: &LineHover,
    text_layout: &TextLayout,
    source_bounds: Bounds<Pixels>,
    active: &file_editor::EditorHover,
    cx: &App,
) -> Bounds<Pixels> {
    let buffer = hover.buffer.read(cx);
    let Some(line) = buffer.line(active.line_index) else {
        return source_bounds;
    };
    let start = clamp_to_char_boundary(line, active.byte_range.start.min(line.len()));
    let end = clamp_to_char_boundary(line, active.byte_range.end.min(line.len())).max(start);
    let Some(start_position) = text_layout.position_for_index(start) else {
        return source_bounds;
    };

    let fallback_char_width = source_bounds.size.width / line.chars().count().max(1) as f32;
    let right = text_layout
        .position_for_index(end)
        .map(|position| position.x)
        .filter(|right| *right > start_position.x)
        .unwrap_or(start_position.x + fallback_char_width);
    let width = (right - start_position.x).max(fallback_char_width);
    Bounds::new(
        Point::new(start_position.x, start_position.y),
        gpui::size(width, text_layout.line_height()),
    )
}

fn render_hover_overlay(view: &Entity<EditorView>, cx: &mut App) -> AnyElement {
    let Some(active) = view.read(cx).hover().cloned() else {
        return div().into_any_element();
    };
    if !hover_status_has_popup(&active.status) {
        return div().into_any_element();
    }
    let Some(source_bounds) = active.source_bounds else {
        return div().into_any_element();
    };

    let anchor = point(source_bounds.left(), source_bounds.bottom());
    let overlay_view = view.clone();
    let bounds_view = view.clone();
    let line_index = active.line_index;

    deferred(
        anchored()
            .position(anchor)
            .position_mode(AnchoredPositionMode::Window)
            .anchor(Corner::TopLeft)
            .snap_to_window()
            .child(
                div()
                    .child(render_hover_popup(view, line_index, cx))
                    .on_children_prepainted(move |bounds, window, cx| {
                        if let Some(bounds) = bounds.first().copied() {
                            bounds_view.update(cx, |view, _| {
                                view.set_hover_popup_bounds(line_index, bounds)
                            });
                            update_hover_visibility_at(
                                &bounds_view,
                                window.mouse_position(),
                                window,
                                cx,
                            );
                        }
                    })
                    .on_mouse_move(move |event, window, cx| {
                        update_hover_visibility(&overlay_view, event, window, cx);
                    })
                    .id(("lsp-hover-overlay-hitbox", line_index)),
            ),
    )
    .with_priority(3)
    .into_any_element()
}

fn render_hover_popup(view: &Entity<EditorView>, line_index: usize, cx: &mut App) -> AnyElement {
    let theme = *cx.theme();
    let active_hover = view.read(cx).hover().cloned();
    let visible = active_hover
        .as_ref()
        .is_some_and(|hover| hover.line_index == line_index)
        && active_hover
            .as_ref()
            .is_some_and(|hover| hover_status_has_popup(&hover.status));
    let (text, muted) = match active_hover.map(|hover| hover.status) {
        Some(EditorHoverStatus::Loading) => ("Loading LSP hover...".to_string(), true),
        Some(EditorHoverStatus::Ready(text)) => (text, false),
        Some(EditorHoverStatus::Empty) => ("No hover information".to_string(), true),
        Some(EditorHoverStatus::Error(err)) => (format!("LSP hover failed: {err}"), true),
        None => (String::new(), true),
    };

    let content = render_markdown(&text, theme, muted, cx);
    div()
        .id(("lsp-hover-tooltip", view.entity_id()))
        .when(!visible, |this| this.hidden())
        .max_w(rems(42.0))
        .max_h(rems(28.0))
        .overflow_y_scroll()
        .block_mouse_except_scroll()
        .px(rems(0.75))
        .py(rems(0.625))
        .rounded(rems(0.375))
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.bg_elevated)
        .shadow_lg()
        .text_xs()
        .line_height(rems(1.25))
        .font_family(FONT_FAMILY)
        .flex()
        .flex_col()
        .gap(rems(0.5))
        .text_color(if muted {
            theme.text_muted
        } else {
            theme.text_emphasis
        })
        .children(content)
        .into_any_element()
}

fn hover_status_has_popup(status: &EditorHoverStatus) -> bool {
    matches!(
        status,
        EditorHoverStatus::Ready(_) | EditorHoverStatus::Error(_)
    )
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
struct MarkdownStyle {
    emphasis: bool,
    strong: bool,
    code: bool,
    link: bool,
}

#[derive(Clone, Copy)]
enum MarkdownStyleKind {
    Emphasis,
    Strong,
    Code,
    Link,
}

#[derive(Default)]
struct InlineMarkdown {
    text: String,
    ranges: Vec<(Range<usize>, MarkdownStyle)>,
    stack: Vec<MarkdownStyleKind>,
}

enum MarkdownBlock {
    Paragraph(InlineMarkdown),
    Heading(HeadingLevel, InlineMarkdown),
    ListItem(InlineMarkdown),
    CodeBlock {
        language: Option<String>,
        text: String,
    },
    Rule,
}

enum ActiveMarkdownBlock {
    Paragraph(InlineMarkdown),
    Heading(HeadingLevel, InlineMarkdown),
    ListItem(InlineMarkdown),
}

impl InlineMarkdown {
    fn push(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(text);
        let end = self.text.len();
        let style = self.current_style();
        if style != MarkdownStyle::default() {
            self.ranges.push((start..end, style));
        }
    }

    fn push_with(&mut self, text: &str, kind: MarkdownStyleKind) {
        self.stack.push(kind);
        self.push(text);
        self.stack.pop();
    }

    fn push_style(&mut self, kind: MarkdownStyleKind) {
        self.stack.push(kind);
    }

    fn pop_style(&mut self, kind: MarkdownStyleKind) {
        if let Some(index) = self
            .stack
            .iter()
            .rposition(|existing| std::mem::discriminant(existing) == std::mem::discriminant(&kind))
        {
            self.stack.remove(index);
        }
    }

    fn current_style(&self) -> MarkdownStyle {
        let mut style = MarkdownStyle::default();
        for kind in &self.stack {
            match kind {
                MarkdownStyleKind::Emphasis => style.emphasis = true,
                MarkdownStyleKind::Strong => style.strong = true,
                MarkdownStyleKind::Code => style.code = true,
                MarkdownStyleKind::Link => style.link = true,
            }
        }
        style
    }
}

impl ActiveMarkdownBlock {
    fn inline_mut(&mut self) -> &mut InlineMarkdown {
        match self {
            Self::Paragraph(inline) | Self::Heading(_, inline) | Self::ListItem(inline) => inline,
        }
    }

    fn finish(self) -> MarkdownBlock {
        match self {
            Self::Paragraph(inline) => MarkdownBlock::Paragraph(inline),
            Self::Heading(level, inline) => MarkdownBlock::Heading(level, inline),
            Self::ListItem(inline) => MarkdownBlock::ListItem(inline),
        }
    }
}

fn render_markdown(text: &str, theme: Theme, muted: bool, cx: &mut App) -> Vec<AnyElement> {
    let blocks = parse_markdown(text);
    if blocks.is_empty() {
        return vec![
            div()
                .child(SharedString::from(text.to_string()))
                .into_any_element(),
        ];
    }

    blocks
        .into_iter()
        .map(|block| render_markdown_block(block, theme, muted, cx))
        .collect()
}

fn parse_markdown(text: &str) -> Vec<MarkdownBlock> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);

    let mut blocks = Vec::new();
    let mut active: Option<ActiveMarkdownBlock> = None;
    let mut code_block: Option<(Option<String>, String)> = None;
    let mut list_depth = 0usize;

    for event in Parser::new_ext(text, options) {
        match event {
            Event::Start(Tag::Paragraph) if active.is_none() => {
                active = Some(ActiveMarkdownBlock::Paragraph(InlineMarkdown::default()));
            }
            Event::Start(Tag::Paragraph) => {}
            Event::Start(Tag::Heading { level, .. }) => {
                active = Some(ActiveMarkdownBlock::Heading(
                    level,
                    InlineMarkdown::default(),
                ));
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let language = match kind {
                    CodeBlockKind::Fenced(language) if !language.is_empty() => {
                        Some(language.to_string())
                    }
                    _ => None,
                };
                code_block = Some((language, String::new()));
            }
            Event::Start(Tag::List(_)) => {
                list_depth += 1;
            }
            Event::Start(Tag::Item) => {
                active = Some(ActiveMarkdownBlock::ListItem(InlineMarkdown::default()));
            }
            Event::Start(Tag::Emphasis) => {
                push_active_style(&mut active, MarkdownStyleKind::Emphasis)
            }
            Event::Start(Tag::Strong) => push_active_style(&mut active, MarkdownStyleKind::Strong),
            Event::Start(Tag::Link { .. }) => {
                push_active_style(&mut active, MarkdownStyleKind::Link)
            }
            Event::End(TagEnd::Paragraph) => {
                if !matches!(active, Some(ActiveMarkdownBlock::ListItem(_)))
                    && let Some(active) = active.take()
                {
                    blocks.push(active.finish());
                }
            }
            Event::End(TagEnd::Heading(_)) | Event::End(TagEnd::Item) => {
                if let Some(active) = active.take() {
                    blocks.push(active.finish());
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((language, text)) = code_block.take() {
                    blocks.push(MarkdownBlock::CodeBlock { language, text });
                }
            }
            Event::End(TagEnd::List(_)) => {
                list_depth = list_depth.saturating_sub(1);
            }
            Event::End(TagEnd::Emphasis) => {
                pop_active_style(&mut active, MarkdownStyleKind::Emphasis)
            }
            Event::End(TagEnd::Strong) => pop_active_style(&mut active, MarkdownStyleKind::Strong),
            Event::End(TagEnd::Link) => pop_active_style(&mut active, MarkdownStyleKind::Link),
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                if let Some((_, code)) = code_block.as_mut() {
                    code.push_str(&text);
                } else {
                    push_active_text(&mut active, &text);
                }
            }
            Event::Code(text) => {
                if let Some(active) = active.as_mut() {
                    active
                        .inline_mut()
                        .push_with(&text, MarkdownStyleKind::Code);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some((_, code)) = code_block.as_mut() {
                    code.push('\n');
                } else {
                    push_active_text(&mut active, "\n");
                }
            }
            Event::Rule => blocks.push(MarkdownBlock::Rule),
            Event::TaskListMarker(checked) => {
                push_active_text(&mut active, if checked { "[x] " } else { "[ ] " });
            }
            Event::FootnoteReference(reference) => {
                push_active_text(&mut active, &format!("[{reference}]"));
            }
            _ => {}
        }

        if list_depth == 0 && matches!(active, Some(ActiveMarkdownBlock::ListItem(_))) {
            if let Some(active) = active.take() {
                blocks.push(active.finish());
            }
        }
    }

    if let Some(active) = active.take() {
        blocks.push(active.finish());
    }
    if let Some((language, text)) = code_block.take() {
        blocks.push(MarkdownBlock::CodeBlock { language, text });
    }

    blocks
}

fn push_active_text(active: &mut Option<ActiveMarkdownBlock>, text: &str) {
    if active.is_none() {
        *active = Some(ActiveMarkdownBlock::Paragraph(InlineMarkdown::default()));
    }
    if let Some(active) = active.as_mut() {
        active.inline_mut().push(text);
    }
}

fn push_active_style(active: &mut Option<ActiveMarkdownBlock>, style: MarkdownStyleKind) {
    if let Some(active) = active.as_mut() {
        active.inline_mut().push_style(style);
    }
}

fn pop_active_style(active: &mut Option<ActiveMarkdownBlock>, style: MarkdownStyleKind) {
    if let Some(active) = active.as_mut() {
        active.inline_mut().pop_style(style);
    }
}

fn render_markdown_block(
    block: MarkdownBlock,
    theme: Theme,
    muted: bool,
    cx: &mut App,
) -> AnyElement {
    match block {
        MarkdownBlock::Paragraph(inline) => render_inline_markdown(inline, theme, muted)
            .mb(rems(0.125))
            .into_any_element(),
        MarkdownBlock::Heading(level, inline) => render_inline_markdown(inline, theme, muted)
            .text_size(match level {
                HeadingLevel::H1 | HeadingLevel::H2 => rems(0.95),
                _ => rems(0.875),
            })
            .font_weight(FontWeight::BOLD)
            .text_color(theme.text_emphasis)
            .into_any_element(),
        MarkdownBlock::ListItem(inline) => div()
            .flex()
            .flex_row()
            .gap(rems(0.375))
            .child(div().flex_none().text_color(theme.text_subtle).child("•"))
            .child(
                render_inline_markdown(inline, theme, muted)
                    .flex_1()
                    .min_w_0(),
            )
            .into_any_element(),
        MarkdownBlock::CodeBlock { language, text } => render_code_block(language, text, theme, cx),
        MarkdownBlock::Rule => div()
            .h(rems(0.0625))
            .w_full()
            .bg(theme.border_subtle)
            .into_any_element(),
    }
}

fn render_inline_markdown(inline: InlineMarkdown, theme: Theme, muted: bool) -> gpui::Div {
    let highlights = inline
        .ranges
        .into_iter()
        .map(|(range, style)| (range, markdown_highlight(style, theme)));
    div()
        .text_color(if muted {
            theme.text_muted
        } else {
            theme.text_emphasis
        })
        .child(StyledText::new(SharedString::from(inline.text)).with_highlights(highlights))
}

fn markdown_highlight(style: MarkdownStyle, theme: Theme) -> HighlightStyle {
    HighlightStyle {
        color: if style.code {
            Some(theme.syntax.markup_code.into())
        } else if style.link {
            Some(theme.syntax.markup_link.into())
        } else {
            None
        },
        font_weight: style.strong.then_some(FontWeight::BOLD),
        font_style: style.emphasis.then_some(FontStyle::Italic),
        background_color: style.code.then_some(theme.bg_hover.into()),
        ..Default::default()
    }
}

fn render_code_block(
    language: Option<String>,
    text: String,
    theme: Theme,
    cx: &mut App,
) -> AnyElement {
    let code = text.trim_end_matches('\n');
    let highlighted = language
        .as_deref()
        .and_then(code_block_language_id)
        .and_then(|language| SyntaxRegistry::load(&language, cx))
        .map(|grammar| syntax::highlight_content(&grammar, code));
    let mut block = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(rems(0.25))
        .rounded(rems(0.3125))
        .border_1()
        .border_color(theme.border_subtle)
        .bg(theme.bg_hover)
        .px(rems(0.625))
        .py(rems(0.5))
        .text_color(theme.text_emphasis);

    if let Some(language) = language.filter(|language| !language.is_empty()) {
        block = block.child(
            div()
                .text_color(theme.text_subtle)
                .font_weight(FontWeight::BOLD)
                .child(language),
        );
    }

    let mut line_start = 0usize;
    for line in code.lines() {
        let spans = highlighted
            .as_deref()
            .map(|raw| clip_spans_to_line(line, line_start, raw))
            .unwrap_or_default();
        block = block.child(render_code_line(line, spans, theme));
        line_start += line.len() + 1;
    }

    block.into_any_element()
}

fn code_block_language_id(language: &str) -> Option<language::LanguageId> {
    let raw = language
        .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == ';')
        .next()?
        .trim()
        .to_ascii_lowercase();
    if raw.is_empty() {
        return None;
    }

    let canonical = match raw.as_str() {
        "bash" | "sh" | "shell" | "zsh" => Some("shellscript"),
        "js" => Some("javascript"),
        "jsx" => Some("javascriptreact"),
        "ts" => Some("typescript"),
        "tsx" => Some("typescriptreact"),
        "py" => Some("python"),
        "rs" => Some("rust"),
        "yml" => Some("yaml"),
        "md" => Some("markdown"),
        "c++" | "cc" | "cxx" => Some("cpp"),
        _ => None,
    };
    if let Some(canonical) = canonical {
        return Some(language::LanguageId::from(canonical));
    }
    if language::info(&raw).is_some() {
        return Some(language::LanguageId::new(raw));
    }
    language::from_extension(&raw)
}

fn render_code_line(
    line: &str,
    spans: Vec<(Range<usize>, HighlightId)>,
    theme: Theme,
) -> AnyElement {
    let line = SharedString::from(line.to_string());
    let mut row = div().text_color(theme.syntax.markup_code);
    if spans.is_empty() {
        row = row.child(line);
    } else {
        let highlights = spans
            .into_iter()
            .map(|(range, id)| (range, theme.syntax.style(id)));
        row = row.child(StyledText::new(line).with_highlights(highlights));
    }
    row.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fenced_code_blocks() {
        let blocks = parse_markdown("```rust\nfn main() {}\n```");

        let MarkdownBlock::CodeBlock { language, text } = &blocks[0] else {
            panic!("expected code block");
        };
        assert_eq!(language.as_deref(), Some("rust"));
        assert_eq!(text, "fn main() {}\n");
    }

    #[test]
    fn parses_inline_code_and_emphasis() {
        let blocks = parse_markdown("Use `hover` for *details*.");

        let MarkdownBlock::Paragraph(inline) = &blocks[0] else {
            panic!("expected paragraph");
        };
        assert_eq!(inline.text, "Use hover for details.");
        assert!(inline.ranges.iter().any(|(_, style)| style.code));
        assert!(inline.ranges.iter().any(|(_, style)| style.emphasis));
    }

    #[test]
    fn normalizes_code_block_language_aliases() {
        assert_eq!(
            code_block_language_id("tsx").map(|id| id.to_string()),
            Some("typescriptreact".to_string())
        );
        assert_eq!(
            code_block_language_id("bash").map(|id| id.to_string()),
            Some("shellscript".to_string())
        );
        assert_eq!(
            code_block_language_id("rust ignore").map(|id| id.to_string()),
            Some("rust".to_string())
        );
    }

    #[test]
    fn symbol_range_covers_entire_identifier() {
        assert_eq!(symbol_range_at("let declaration_name = 1", 6), Some(4..20));
        assert_eq!(symbol_range_at("let declaration_name = 1", 19), Some(4..20));
    }

    #[test]
    fn symbol_range_ignores_whitespace() {
        assert_eq!(symbol_range_at("let value = 1", 3), None);
    }

    #[test]
    fn symbol_range_ignores_punctuation() {
        assert_eq!(symbol_range_at("call(value);", 4), None);
        assert_eq!(symbol_range_at("call(value);", 10), None);
        assert_eq!(symbol_range_at("foo.bar", 3), None);
        assert_eq!(symbol_range_at("a / b", 2), None);
    }

    #[test]
    fn symbol_range_covers_entire_double_quoted_string() {
        assert_eq!(
            symbol_range_at("let value = \"hello world\";", 13),
            Some(12..25)
        );
        assert_eq!(
            symbol_range_at("let value = \"hello world\";", 18),
            Some(12..25)
        );
        assert_eq!(
            symbol_range_at("let value = \"hello world\";", 24),
            Some(12..25)
        );
    }

    #[test]
    fn symbol_range_keeps_escaped_quotes_inside_string() {
        assert_eq!(
            symbol_range_at(r#"let value = "hello \"world\"";"#, 24),
            Some(12..29)
        );
        assert_eq!(
            symbol_range_at(r#"let value = "hello \"world\"";"#, 20),
            Some(12..29)
        );
    }

    #[test]
    fn line_highlights_combines_syntax_and_hover_source() {
        let theme = Theme::dark();
        let highlights = line_highlights(
            10,
            vec![(0..10, HighlightId::Variable)],
            &theme.syntax,
            Some(4..8),
            theme,
        );

        assert_eq!(highlights.len(), 3);
        assert_eq!(highlights[0].0, 0..4);
        assert_eq!(highlights[1].0, 4..8);
        assert_eq!(highlights[2].0, 8..10);
        assert_eq!(
            highlights[1].1.background_color,
            Some(theme.bg_hover_strong.into())
        );
        assert_eq!(highlights[1].1.color, highlights[0].1.color);
    }

    #[test]
    fn line_highlights_supports_hover_source_without_syntax() {
        let theme = Theme::dark();
        let highlights = line_highlights(10, Vec::new(), &theme.syntax, Some(2..5), theme);

        assert_eq!(highlights.len(), 1);
        assert_eq!(highlights[0].0, 2..5);
        assert_eq!(
            highlights[0].1.background_color,
            Some(theme.bg_hover_strong.into())
        );
    }

    #[test]
    fn hover_popup_only_renders_after_lsp_result() {
        assert!(!hover_status_has_popup(&EditorHoverStatus::Loading));
        assert!(!hover_status_has_popup(&EditorHoverStatus::Empty));
        assert!(hover_status_has_popup(&EditorHoverStatus::Ready(
            "details".to_string()
        )));
        assert!(hover_status_has_popup(&EditorHoverStatus::Error(
            "failed".to_string()
        )));
    }
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
