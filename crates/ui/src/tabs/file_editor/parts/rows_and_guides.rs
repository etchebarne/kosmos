struct EditorRow {
    line_number: usize,
    line: SharedString,
    spans: Vec<(Range<usize>, HighlightId)>,
    soft_wrap: bool,
    sticky_offset: Pixels,
    foldable: bool,
    folded: bool,
    show_fold_arrow: bool,
    hovered_fold_line: Option<usize>,
    edit_state: EditLineState,
    hover: Option<LineHover>,
}

struct LineText {
    line: SharedString,
    spans: Vec<(Range<usize>, HighlightId)>,
    soft_wrap: bool,
    edit_state: EditLineState,
    hover: Option<LineHover>,
}

struct GutterRow {
    row_index: usize,
    line_number: Option<usize>,
    sticky_offset: Pixels,
    foldable: bool,
    folded: bool,
    show_fold_arrow: bool,
    hovered_fold_line: Option<usize>,
}

fn render_row(
    row: EditorRow,
    view: &Entity<EditorView>,
    theme: &Theme,
    cx: &App,
) -> impl IntoElement {
    let selection_background = if row.soft_wrap {
        None
    } else {
        render_selection_background(row.line.as_ref(), row.edit_state.selection.as_ref(), *theme)
    };
    div()
        .relative()
        .w_full()
        // Soft-wrap mode lets rows grow vertically to fit wrapped lines, so
        // we only fix the row height for the non-wrap path.
        .when(!row.soft_wrap, |this| this.h(rems(ROW_HEIGHT_REM)))
        .line_height(rems(ROW_HEIGHT_REM))
        .child(
            // Reserve left space for the gutter overlay so the line text
            // never starts underneath it.
            div()
                .w_full()
                .min_w_0()
                .pl(rems(GUTTER_TOTAL_WIDTH_REM + BODY_PADDING_LEFT_REM))
                .child(
                    div()
                        .relative()
                        .w_full()
                        .min_w_0()
                        .when(!row.soft_wrap, |this| this.whitespace_nowrap())
                        .when_some(selection_background, |this, selection| {
                            this.child(selection)
                        })
                        .child(render_line_text(
                            LineText {
                                line: row.line,
                                spans: row.spans,
                                soft_wrap: row.soft_wrap,
                                edit_state: row.edit_state,
                                hover: row.hover,
                            },
                            view,
                            theme,
                            cx,
                        )),
                ),
        )
        .child(render_gutter(
            GutterRow {
                row_index: row.line_number - 1,
                line_number: Some(row.line_number),
                sticky_offset: row.sticky_offset,
                foldable: row.foldable,
                folded: row.folded,
                show_fold_arrow: row.show_fold_arrow,
                hovered_fold_line: row.hovered_fold_line,
            },
            view,
            *theme,
        ))
}

fn render_selection_background(
    line: &str,
    selection: Option<&LineSelection>,
    theme: Theme,
) -> Option<AnyElement> {
    let selection = selection?;
    let (start_column, end_column) = selection_visual_columns(line, selection);
    let width_columns = end_column.saturating_sub(start_column).max(1);
    let left_rem = start_column as f32 * MONOSPACE_CHAR_WIDTH_REM;
    let width_rem = width_columns as f32 * MONOSPACE_CHAR_WIDTH_REM;
    Some(
        div()
            .absolute()
            .top_0()
            .left(rems(left_rem))
            .h(rems(ROW_HEIGHT_REM))
            .w(rems(width_rem))
            .bg(gpui::Hsla::from(theme.accent).opacity(0.35))
            .into_any_element(),
    )
}

fn selection_visual_columns(line: &str, selection: &LineSelection) -> (usize, usize) {
    let start = visual_column_for_byte(line, selection.range.start.min(line.len()));
    let mut end = visual_column_for_byte(line, selection.range.end.min(line.len()));
    if selection.includes_line_break {
        end += 1;
    }
    (start, end)
}

fn visual_column_for_byte(line: &str, byte_offset: usize) -> usize {
    let byte_offset = byte_offset.min(line.len());
    let mut column = 0usize;
    for (index, ch) in line.char_indices() {
        if index >= byte_offset {
            break;
        }
        column += if ch == '\t' {
            TAB_SIZE_COLUMNS - (column % TAB_SIZE_COLUMNS)
        } else {
            1
        };
    }
    column
}

/// Build the styled text element for a line, lifting the highlight spans into
/// gpui `HighlightStyle` runs (color + italic/bold modifiers from the theme).
/// Falls back to plain text when there are no spans (no grammar, parse not
/// finished, or this line has no captures).
fn render_line_text(
    line_text: LineText,
    view: &Entity<EditorView>,
    theme: &Theme,
    cx: &App,
) -> AnyElement {
    let (display_byte_offset, indent_columns) = if line_text.soft_wrap {
        leading_indentation(line_text.line.as_ref())
    } else {
        (0, 0)
    };
    let original_line = line_text.line.clone();
    let display_line = if display_byte_offset == 0 {
        line_text.line
    } else {
        SharedString::from(original_line[display_byte_offset..].to_string())
    };
    let display_len = display_line.len();
    let spans = shift_spans_for_display(line_text.spans, display_byte_offset, display_len);
    let cursor = line_text.edit_state.cursor.and_then(|cursor| {
        if cursor < display_byte_offset || cursor > display_byte_offset + display_len {
            None
        } else {
            Some(cursor - display_byte_offset)
        }
    });
    let source_highlight = line_text
        .hover
        .as_ref()
        .and_then(|hover| hover_source_highlight_range(hover, cx))
        .and_then(|range| shift_range_for_display(range, display_byte_offset, display_len));
    let selection_highlight = if line_text.soft_wrap {
        line_text
            .edit_state
            .selection
            .as_ref()
            .and_then(|selection| {
                shift_range_for_display(selection.range.clone(), display_byte_offset, display_len)
            })
    } else {
        None
    };
    let highlights = line_highlights(
        display_line.len(),
        spans,
        &theme.syntax,
        source_highlight,
        selection_highlight,
        *theme,
    );
    let text = if highlights.is_empty() {
        StyledText::new(display_line)
    } else {
        StyledText::new(display_line).with_highlights(highlights)
    };
    let text_layout = text.layout().clone();
    let focus_handle = view.read(cx).focus_handle();
    let selection = if line_text.soft_wrap {
        line_text.edit_state.selection.clone().map(|selection| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .child(SoftWrapSelectionElement {
                    line: original_line.clone(),
                    text_layout: text_layout.clone(),
                    display_byte_offset,
                    selection,
                    color: gpui::Hsla::from(theme.accent).opacity(0.35),
                })
                .into_any_element()
        })
    } else {
        None
    };
    let cursor = cursor.map(|cursor| {
        div()
            .absolute()
            .top_0()
            .left_0()
            .child(CursorElement {
                text_layout: text_layout.clone(),
                cursor,
                color: theme.text_emphasis,
                focus_handle: focus_handle.clone(),
            })
            .into_any_element()
    });
    let indent_padding = rems(indent_columns as f32 * MONOSPACE_CHAR_WIDTH_REM);

    let Some(hover) = line_text.hover else {
        return div()
            .relative()
            .w_full()
            .min_w_0()
            .when(line_text.soft_wrap && indent_columns > 0, |this| {
                this.pl(indent_padding)
            })
            .when_some(selection, |this, selection| this.child(selection))
            .child(text)
            .when_some(cursor, |this, cursor| this.child(cursor))
            .into_any_element();
    };
    let hover_for_move = hover.clone();
    let hover_for_prepaint = hover.clone();
    let line_layout_for_prepaint = text_layout.clone();
    let text = InteractiveText::new(("file-editor-line", hover.line_index), text)
        .on_hover(move |byte_index, _event, _window, cx| {
            if let Some(byte_index) = byte_index {
                begin_lsp_hover(&hover_for_move, display_byte_offset + byte_index, cx);
            } else {
                schedule_hover_hide(&hover_for_move.view, hover_for_move.line_index, cx);
            }
        })
        .into_any_element();

    div()
        .relative()
        .w_full()
        .min_w_0()
        .when(line_text.soft_wrap && indent_columns > 0, |this| {
            this.pl(indent_padding)
        })
        .when_some(selection, |this, selection| this.child(selection))
        .child(text)
        .when_some(cursor, |this, cursor| this.child(cursor))
        .on_children_prepainted(move |bounds, window, cx| {
            hover_for_prepaint.view.update(cx, |view, _| {
                view.set_line_input_layout(EditorLineInputLayout {
                    line_index: hover_for_prepaint.line_index,
                    display_byte_offset,
                    text_layout: line_layout_for_prepaint.clone(),
                });
            });
            update_hover_source_bounds(
                &hover_for_prepaint,
                &text_layout,
                display_byte_offset,
                bounds,
                window,
                cx,
            );
        })
        .id(("file-editor-line-hover", hover.line_index))
        .into_any_element()
}

fn soft_wrap_selection_extra_bounds(
    line: &str,
    text_layout: &TextLayout,
    display_byte_offset: usize,
    selection: &LineSelection,
    window: &Window,
) -> Vec<Bounds<Pixels>> {
    let line_len = line.len();
    let selection_start = selection.range.start.min(line_len);
    let selection_end = selection.range.end.min(line_len);
    let display_byte_offset = display_byte_offset.min(line_len);
    let mut bounds = Vec::new();

    push_soft_wrap_indent_selection_bound(
        &mut bounds,
        line,
        text_layout,
        display_byte_offset,
        selection_start,
        selection_end,
        window,
    );

    if selection.includes_line_break {
        let display_len = line_len.saturating_sub(display_byte_offset);
        push_soft_wrap_line_break_selection_bound(&mut bounds, text_layout, display_len, window);
    }

    bounds
}

fn push_soft_wrap_indent_selection_bound(
    bounds: &mut Vec<Bounds<Pixels>>,
    line: &str,
    text_layout: &TextLayout,
    display_byte_offset: usize,
    selection_start: usize,
    selection_end: usize,
    window: &Window,
) {
    let indent_selection_start = selection_start.min(display_byte_offset);
    let indent_selection_end = selection_end.min(display_byte_offset);
    if indent_selection_start >= indent_selection_end {
        return;
    }

    let char_width = monospace_char_width(window.rem_size());
    let indent_columns = visual_column_for_byte(line, display_byte_offset);
    let text_bounds = text_layout.bounds();
    let indent_left = text_bounds.left() - char_width * indent_columns;
    let left = indent_left + char_width * visual_column_for_byte(line, indent_selection_start);
    let right = indent_left + char_width * visual_column_for_byte(line, indent_selection_end);
    push_selection_bound(
        bounds,
        left,
        text_bounds.top(),
        right,
        text_bounds.top() + text_layout.line_height(),
    );
}

fn push_soft_wrap_line_break_selection_bound(
    bounds: &mut Vec<Bounds<Pixels>>,
    text_layout: &TextLayout,
    display_len: usize,
    window: &Window,
) {
    let Some(position) = text_layout.position_for_index(display_len) else {
        return;
    };
    let char_width = monospace_char_width(window.rem_size());
    push_selection_bound(
        bounds,
        position.x,
        position.y,
        position.x + char_width,
        position.y + text_layout.line_height(),
    );
}

fn push_selection_bound(
    bounds: &mut Vec<Bounds<Pixels>>,
    left: Pixels,
    top: Pixels,
    right: Pixels,
    bottom: Pixels,
) {
    if right <= left || bottom <= top {
        return;
    }
    bounds.push(Bounds::from_corners(point(left, top), point(right, bottom)));
}

fn render_indent_guides_overlay(
    view: &Entity<EditorView>,
    soft_wrap: bool,
    row_count: usize,
    indent_guides: Vec<Vec<usize>>,
    cx: &App,
) -> AnyElement {
    let theme = *cx.theme();
    if soft_wrap {
        let state = view.read(cx).virtual_scroll();
        render_indent_guides_canvas(theme, move |bounds, window| {
            let rows = state
                .visible_rows()
                .into_iter()
                .map(|(index, top, bottom)| VisibleIndentRow { index, top, bottom })
                .collect::<Vec<_>>();
            continuous_indent_guide_bounds(bounds, rows, &indent_guides, Pixels::ZERO, window)
        })
    } else {
        let scroll = view.read(cx).uniform_scroll();
        render_indent_guides_canvas(theme, move |bounds, window| {
            let offset = scroll.0.borrow().base_handle.offset();
            let row_height = rems(ROW_HEIGHT_REM).to_pixels(window.rem_size());
            let rows =
                uniform_visible_indent_rows(row_count, row_height, -offset.y, bounds.size.height);
            continuous_indent_guide_bounds(bounds, rows, &indent_guides, offset.x, window)
        })
    }
}

fn render_indent_guides_canvas(
    theme: Theme,
    compute: impl Fn(Bounds<Pixels>, &mut Window) -> Vec<Bounds<Pixels>> + 'static,
) -> AnyElement {
    canvas(
        move |bounds, window, _cx| compute(bounds, window),
        move |_bounds, guide_bounds, window, _cx| {
            let color = gpui::Hsla::from(theme.text).opacity(0.1);
            for bounds in guide_bounds {
                window.paint_quad(fill(bounds, color));
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .right_0()
    .bottom_0()
    .into_any_element()
}

fn uniform_visible_indent_rows(
    row_count: usize,
    row_height: Pixels,
    scroll_y: Pixels,
    viewport_height: Pixels,
) -> Vec<VisibleIndentRow> {
    if row_count == 0 || row_height <= Pixels::ZERO || viewport_height <= Pixels::ZERO {
        return Vec::new();
    }

    let first = ((scroll_y / row_height).floor() as usize).min(row_count);
    let last = (((scroll_y + viewport_height) / row_height).ceil() as usize).min(row_count);
    (first..last)
        .map(|index| {
            let top = row_height * index - scroll_y;
            VisibleIndentRow {
                index,
                top,
                bottom: top + row_height,
            }
        })
        .collect()
}

fn continuous_indent_guide_bounds(
    bounds: Bounds<Pixels>,
    rows: Vec<VisibleIndentRow>,
    indent_guides: &[Vec<usize>],
    scroll_x: Pixels,
    window: &mut Window,
) -> Vec<Bounds<Pixels>> {
    let Some(max_column) = rows
        .iter()
        .filter_map(|row| indent_guides.get(row.index))
        .flat_map(|guides| guides.iter().copied())
        .max()
    else {
        return Vec::new();
    };
    let Some(x_offsets) = indent_guide_x_offsets(max_column, window) else {
        return Vec::new();
    };

    let guide_width = rems(INDENT_GUIDE_WIDTH_REM)
        .to_pixels(window.rem_size())
        .ceil();
    let text_left =
        rems(GUTTER_TOTAL_WIDTH_REM + BODY_PADDING_LEFT_REM).to_pixels(window.rem_size());
    let guide_context = IndentGuideContext {
        bounds,
        scroll_x,
        text_left,
        x_offsets: &x_offsets,
        guide_width,
    };
    let mut active: Vec<ActiveIndentGuideRun> = Vec::new();
    let mut guide_bounds = Vec::new();
    let mut last_row_bottom = Pixels::ZERO;

    for row in rows {
        last_row_bottom = row.bottom;
        let row_guides = indent_guides
            .get(row.index)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let mut index = 0;
        while index < active.len() {
            if row_guides.binary_search(&active[index].column).is_ok() {
                index += 1;
            } else {
                push_indent_guide_bound(
                    &mut guide_bounds,
                    &guide_context,
                    active[index],
                    row.top,
                );
                active.remove(index);
            }
        }

        for &column in row_guides {
            if active.iter().all(|run| run.column != column) {
                active.push(ActiveIndentGuideRun {
                    column,
                    top: row.top,
                });
            }
        }
    }

    for run in active {
        push_indent_guide_bound(&mut guide_bounds, &guide_context, run, last_row_bottom);
    }

    guide_bounds
}

struct IndentGuideContext<'a> {
    bounds: Bounds<Pixels>,
    scroll_x: Pixels,
    text_left: Pixels,
    x_offsets: &'a [Pixels],
    guide_width: Pixels,
}

fn push_indent_guide_bound(
    out: &mut Vec<Bounds<Pixels>>,
    context: &IndentGuideContext<'_>,
    run: ActiveIndentGuideRun,
    bottom: Pixels,
) {
    if bottom <= run.top {
        return;
    }
    let Some(column_x) = context.x_offsets.get(run.column).copied() else {
        return;
    };
    let left = (context.bounds.left() + context.scroll_x + context.text_left + column_x
        - context.guide_width / 2.0)
        .round();
    let top = (context.bounds.top() + run.top).round();
    let bottom = (context.bounds.top() + bottom).round();
    if bottom <= top {
        return;
    }

    out.push(Bounds::new(
        point(left, top),
        gpui::size(context.guide_width, bottom - top),
    ));
}

fn indent_guide_x_offsets(max_column: usize, window: &mut Window) -> Option<Vec<Pixels>> {
    let text_style = window.text_style();
    let font_size = text_style.font_size.to_pixels(window.rem_size());
    let line_height = window.line_height();
    let text = SharedString::from(" ".repeat(max_column + 1));
    let run = TextRun {
        len: text.len(),
        font: text_style.font(),
        color: text_style.color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let Ok(lines) = window
        .text_system()
        .shape_text(text, font_size, &[run], None, None)
    else {
        return None;
    };
    let line = lines.first()?;

    Some(
        (0..=max_column)
            .map(|column| {
                line.position_for_index(column, line_height)
                    .map(|position| position.x)
                    .unwrap_or(Pixels::ZERO)
            })
            .collect(),
    )
}
