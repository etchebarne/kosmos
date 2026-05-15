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

fn edit_state_for_line(
    buffer: &Entity<Buffer>,
    view: &Entity<EditorView>,
    line_index: usize,
    cx: &App,
) -> EditLineState {
    let buf = buffer.read(cx);
    let Some(line_range) = buf.line_range(line_index) else {
        return EditLineState::default();
    };
    let view = view.read(cx);
    let selection = view.selected_range();
    let cursor = view.cursor_offset();
    let selection = if selection.is_empty() {
        None
    } else {
        let start = selection.start.max(line_range.start);
        let end = selection.end.min(line_range.end);
        let includes_line_break = line_range.end < buf.content().len()
            && selection.start <= line_range.end
            && selection.end > line_range.end;
        (start < end || includes_line_break).then_some(LineSelection {
            range: start - line_range.start..end - line_range.start,
            includes_line_break,
        })
    };
    let cursor = (line_range.start <= cursor && cursor <= line_range.end)
        .then_some(cursor - line_range.start);
    EditLineState { selection, cursor }
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

fn render_spacer_row(
    row_index: usize,
    sticky_offset: Pixels,
    view: &Entity<EditorView>,
    theme: Theme,
) -> AnyElement {
    div()
        .relative()
        .w_full()
        .h(rems(ROW_HEIGHT_REM))
        .child(render_gutter(
            row_index,
            None,
            sticky_offset,
            false,
            false,
            false,
            None,
            view,
            theme,
        ))
        .into_any_element()
}

fn soft_wrap_row_height(
    metrics: SoftWrapLineMetrics,
    viewport_w: Pixels,
    rem_size: Pixels,
) -> Pixels {
    let line_height = rems(ROW_HEIGHT_REM).to_pixels(rem_size);
    let text_width = text_area_width(viewport_w, metrics.indent_columns, rem_size);
    let char_width = monospace_char_width(rem_size);
    let chars_per_line = if text_width > px(0.0) && char_width > px(0.0) {
        ((text_width / char_width).floor() as usize).max(1)
    } else {
        80
    };
    let wraps =
        ((metrics.content_chars.max(1) + chars_per_line - 1) / chars_per_line).max(1) as f32;
    line_height * wraps
}

fn text_area_width(viewport_w: Pixels, indent_columns: usize, rem_size: Pixels) -> Pixels {
    let left_padding = rems(GUTTER_TOTAL_WIDTH_REM + BODY_PADDING_LEFT_REM).to_pixels(rem_size);
    (viewport_w - left_padding - indent_width(indent_columns, rem_size)).max(px(0.0))
}

fn indent_width(columns: usize, rem_size: Pixels) -> Pixels {
    monospace_char_width(rem_size) * columns
}

fn monospace_char_width(rem_size: Pixels) -> Pixels {
    rems(MONOSPACE_CHAR_WIDTH_REM).to_pixels(rem_size)
}

fn soft_wrap_line_metrics(line: &str) -> SoftWrapLineMetrics {
    let (byte_len, indent_columns) = leading_indentation(line);
    SoftWrapLineMetrics {
        content_chars: line[byte_len..].chars().count(),
        indent_columns,
    }
}

fn leading_indentation(line: &str) -> (usize, usize) {
    let mut byte_len = 0usize;
    let mut columns = 0usize;
    for ch in line.chars() {
        match ch {
            ' ' => {
                byte_len += ch.len_utf8();
                columns += 1;
            }
            '\t' => {
                byte_len += ch.len_utf8();
                columns += TAB_SIZE_COLUMNS - (columns % TAB_SIZE_COLUMNS);
            }
            _ => break,
        }
    }
    (byte_len, columns)
}

fn shift_spans_for_display(
    spans: Vec<(Range<usize>, HighlightId)>,
    display_byte_offset: usize,
    display_len: usize,
) -> Vec<(Range<usize>, HighlightId)> {
    spans
        .into_iter()
        .filter_map(|(range, id)| {
            shift_range_for_display(range, display_byte_offset, display_len)
                .map(|range| (range, id))
        })
        .collect()
}

fn shift_range_for_display(
    range: Range<usize>,
    display_byte_offset: usize,
    display_len: usize,
) -> Option<Range<usize>> {
    if range.end <= display_byte_offset {
        return None;
    }
    let start = range
        .start
        .saturating_sub(display_byte_offset)
        .min(display_len);
    let end = range
        .end
        .saturating_sub(display_byte_offset)
        .min(display_len);
    (start < end).then_some(start..end)
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

