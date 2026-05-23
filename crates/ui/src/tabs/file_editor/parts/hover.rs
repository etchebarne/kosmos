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

        let request = cx.update(|cx| build_lsp_hover_request(&hover, generation, cx));
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
    selection: Option<Range<usize>>,
    theme: Theme,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let syntax_highlights = spans
        .into_iter()
        .filter_map(|(range, id)| {
            clipped_range(range, line_len).map(|range| (range, syntax.style(id)))
        })
        .collect::<Vec<_>>();
    let source_highlight = source_highlight.and_then(|range| clipped_range(range, line_len));
    let selection = selection.and_then(|range| clipped_range(range, line_len));

    if syntax_highlights.is_empty() && source_highlight.is_none() && selection.is_none() {
        return Vec::new();
    }

    let mut boundaries = Vec::with_capacity(2 + syntax_highlights.len() * 2 + 4);
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
    if let Some(range) = &selection {
        boundaries.push(range.start);
        boundaries.push(range.end);
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let source_style = source_hover_highlight_style(theme);
    let selection_style = selection_highlight_style(theme);
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
        if selection
            .as_ref()
            .is_some_and(|range| range.start <= start && end <= range.end)
        {
            style = style.highlight(selection_style);
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

fn selection_highlight_style(theme: Theme) -> HighlightStyle {
    HighlightStyle {
        background_color: Some(gpui::Hsla::from(theme.accent).opacity(0.35)),
        ..Default::default()
    }
}
