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
    view.update(cx, |view, cx| view.set_buffer(buffer.clone(), cx));
    let snapshot = SyntaxStore::for_buffer(&buffer, cx);
    observe_snapshot(&view, &snapshot, cx);
    let soft_wrap = soft_wrap_enabled(cx);
    let indents = {
        let buf = buffer.read(cx);
        indents_for_buffer(buf)
    };
    let indent_guides = indent_guides_for_indents(&indents);
    let foldable_lines = foldable_lines_for_indents(&indents);
    let (show_fold_arrows, hovered_fold_line, folded_lines) = {
        let view = view.read(cx);
        (
            view.gutter_hovered(),
            view.hovered_fold_line(),
            view.folded_lines().clone(),
        )
    };
    let visible_lines = visible_lines_for_indents(&indents, &foldable_lines, &folded_lines);
    let has_folded_lines = !folded_lines.is_empty();
    let visible_indent_guides = visible_lines
        .iter()
        .map(|&line| indent_guides.get(line).cloned().unwrap_or_default())
        .collect::<Vec<_>>();
    let row_count = visible_lines.len() + BOTTOM_SPACER_LINES;
    let longest_idx = {
        let buf = buffer.read(cx);
        longest_visible_row_index(buf, &visible_lines)
    };
    let visible_for_mouse = visible_lines.clone();
    let foldable_for_mouse = foldable_lines.clone();

    let body: AnyElement = if soft_wrap {
        let virtual_state = view.read(cx).virtual_scroll();
        // Snapshot per-line char counts so the height closure doesn't need
        // App context. ~one usize per logical line, doesn't change while
        // the buffer is read-only.
        let line_metrics: Vec<SoftWrapLineMetrics> = {
            let buf = buffer.read(cx);
            (0..buf.line_count())
                .map(|i| buf.line(i).map(soft_wrap_line_metrics).unwrap_or_default())
                .collect()
        };
        // Approximate em width for monospace as 0.6 × font_size. Off-by-10%
        // is fine for wrap-count estimation — VirtualList feeds this height
        // straight into the cumulative table without ever shaping text for
        // non-visible rows, so the scrollbar tracks our estimate exactly.
        let visible_for_height = visible_lines.clone();
        let height_fn = move |index: usize, viewport_w: Pixels, rem_size: Pixels| -> Pixels {
            let Some(&line_index) = visible_for_height.get(index) else {
                // Bottom spacer rows: fixed single-line height.
                return rems(ROW_HEIGHT_REM).to_pixels(rem_size);
            };
            soft_wrap_row_height(line_metrics[line_index], viewport_w, rem_size)
        };

        let buffer_for_render = buffer.clone();
        let view_for_render = view.clone();
        let snapshot_for_render = snapshot.clone();
        let root_for_render = file_tree_root;
        let foldable_for_render = foldable_lines;
        let folded_for_render = folded_lines;
        let visible_for_render = visible_lines;
        virtual_list(
            "file-editor-soft-wrap",
            virtual_state,
            row_count,
            height_fn,
            move |index, _window, cx| {
                let Some(&line_index) = visible_for_render.get(index) else {
                    return render_spacer_row(index, px(0.0), &view_for_render, *cx.theme())
                        .into_any_element();
                };
                let theme = *cx.theme();
                // Soft wrap can't scroll horizontally, so the gutter is never
                // sticky — its offset is always 0.
                render_editor_line_row(
                    &EditorLineRowContext {
                        buffer: &buffer_for_render,
                        view: &view_for_render,
                        snapshot: &snapshot_for_render,
                        root: &root_for_render,
                        foldable_lines: &foldable_for_render,
                        folded_lines: &folded_for_render,
                        soft_wrap,
                        show_fold_arrows,
                        hovered_fold_line,
                    },
                    line_index,
                    px(0.0),
                    &theme,
                    cx,
                )
            },
        )
        .size_full()
        .into_any_element()
    } else {
        let scroll = view.read(cx).uniform_scroll();
        let buffer_for_render = buffer.clone();
        let view_for_render = view.clone();
        let snapshot_for_render = snapshot.clone();
        let root_for_render = file_tree_root;
        let foldable_for_render = foldable_lines;
        let folded_for_render = folded_lines;
        let visible_for_render = visible_lines;
        let has_folded_for_render = has_folded_lines;
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
                    if !has_folded_for_render
                        && is_longest_measure
                        && let Some(width) = cached_longest
                    {
                        return render_longest_stub(width, theme).into_any_element();
                    }
                    let Some(&line_index) = visible_for_render.get(i) else {
                        return render_spacer_row(i, sticky_offset, &view_for_render, theme)
                            .into_any_element();
                    };
                    render_editor_line_row(
                        &EditorLineRowContext {
                            buffer: &buffer_for_render,
                            view: &view_for_render,
                            snapshot: &snapshot_for_render,
                            root: &root_for_render,
                            foldable_lines: &foldable_for_render,
                            folded_lines: &folded_for_render,
                            soft_wrap,
                            show_fold_arrows,
                            hovered_fold_line,
                        },
                        line_index,
                        sticky_offset,
                        &theme,
                        cx,
                    )
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
    let indent_guides_overlay =
        render_indent_guides_overlay(&view, soft_wrap, row_count, visible_indent_guides, cx);

    let view_for_drag = view.clone();
    let view_for_mouse = view.clone();
    let view_for_bounds = view.clone();
    let view_for_leave = view.clone();
    let view_for_click = view.clone();
    let view_for_select_move = view.clone();
    let view_for_mouse_up = view.clone();
    let view_for_mouse_up_out = view.clone();
    let visible_for_bounds = visible_for_mouse.clone();
    let input_view = view.clone();
    let focus_handle = view.read(cx).focus_handle();
    let focus_for_click = focus_handle.clone();
    let editor_area = div()
        .relative()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .track_focus(&focus_handle)
        .key_context(KEY_CONTEXT)
        .cursor(CursorStyle::IBeam)
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
        .child(indent_guides_overlay)
        .child(body)
        .child(scrollbar_overlay)
        .child(hover_overlay)
        .child(
            div()
                .absolute()
                .top_0()
                .right_0()
                .bottom_0()
                .left_0()
                .child(EditorInputElement { view: input_view }),
        )
        .on_children_prepainted(move |bounds, window, cx| {
            if let Some(bounds) = bounds.first().copied() {
                let input_layout = editor_input_layout(
                    bounds,
                    &view_for_bounds,
                    soft_wrap,
                    visible_for_bounds.clone(),
                    window,
                    cx,
                );
                view_for_bounds.update(cx, |view, _| {
                    view.set_editor_bounds(bounds);
                    view.set_input_layout(input_layout);
                });
            }
        })
        .id(("file-editor-area", view.entity_id()))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                window.focus(&focus_for_click);
                view_for_click.update(cx, |view, cx| {
                    view.begin_selection_at_point(
                        event.position,
                        event.modifiers.shift,
                        event.click_count,
                        cx,
                    )
                });
            }),
        )
        .on_mouse_move(move |event, window, cx| {
            let selecting = view_for_select_move.update(cx, |view, cx| {
                view.extend_selection_at_point(event.position, cx)
            });
            if selecting {
                cx.stop_propagation();
                return;
            }
            update_gutter_hover_from_mouse(
                &view_for_mouse,
                soft_wrap,
                &visible_for_mouse,
                &foldable_for_mouse,
                event.position,
                window,
                cx,
            );
            update_hover_visibility(&view_for_mouse, event, window, cx);
        })
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |_, _, _, cx| {
                view_for_mouse_up.update(cx, |view, _| view.finish_selection());
            }),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(move |_, _, _, cx| {
                view_for_mouse_up_out.update(cx, |view, _| view.finish_selection());
            }),
        )
        .on_hover(move |hovered, window, cx| {
            if !*hovered {
                update_gutter_hover_state(&view_for_leave, false, None, window, cx);
            }
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
    let editor_area = wire_editor_actions(editor_area, &view, cx);

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

struct EditorLineRowContext<'a> {
    buffer: &'a Entity<Buffer>,
    view: &'a Entity<EditorView>,
    snapshot: &'a Entity<SyntaxSnapshot>,
    root: &'a Option<PathBuf>,
    foldable_lines: &'a [bool],
    folded_lines: &'a HashSet<usize>,
    soft_wrap: bool,
    show_fold_arrows: bool,
    hovered_fold_line: Option<usize>,
}

fn render_editor_line_row(
    context: &EditorLineRowContext<'_>,
    line_index: usize,
    sticky_offset: Pixels,
    theme: &Theme,
    cx: &App,
) -> AnyElement {
    let (line, spans) = line_with_spans(context.buffer, context.snapshot, line_index, cx);
    let edit_state = edit_state_for_line(context.buffer, context.view, line_index, cx);
    render_row(
        EditorRow {
            line_number: line_index + 1,
            line,
            spans,
            soft_wrap: context.soft_wrap,
            sticky_offset,
            foldable: context
                .foldable_lines
                .get(line_index)
                .copied()
                .unwrap_or(false),
            folded: context.folded_lines.contains(&line_index),
            show_fold_arrow: context.show_fold_arrows,
            hovered_fold_line: context.hovered_fold_line,
            edit_state,
            hover: Some(LineHover {
                line_index,
                buffer: context.buffer.clone(),
                view: context.view.clone(),
                root: context.root.clone(),
            }),
        },
        context.view,
        theme,
        cx,
    )
    .into_any_element()
}

fn current_metrics(view: &Entity<EditorView>, soft_wrap: bool, cx: &App) -> EditorScrollMetrics {
    let editor_view = view.read(cx);
    if soft_wrap {
        EditorScrollMetrics::from_virtual(&editor_view.virtual_scroll())
    } else {
        EditorScrollMetrics::from_uniform(&editor_view.uniform_scroll())
    }
}

fn set_scroll_y(view: &Entity<EditorView>, soft_wrap: bool, scrolled: Pixels, cx: &App) {
    let editor_view = view.read(cx);
    if soft_wrap {
        editor_view.virtual_scroll().set_scroll_y(scrolled);
    } else {
        let handle = editor_view.uniform_scroll();
        let state = handle.0.borrow();
        let current = state.base_handle.offset();
        state
            .base_handle
            .set_offset(Point::new(current.x, -scrolled));
    }
}

fn set_scroll_x(view: &Entity<EditorView>, scrolled: Pixels, cx: &App) {
    let editor_view = view.read(cx);
    let handle = editor_view.uniform_scroll();
    let state = handle.0.borrow();
    let current = state.base_handle.offset();
    state
        .base_handle
        .set_offset(Point::new(-scrolled, current.y));
}

fn editor_input_layout(
    bounds: Bounds<Pixels>,
    view: &Entity<EditorView>,
    soft_wrap: bool,
    visible_lines: Vec<usize>,
    window: &mut Window,
    cx: &App,
) -> EditorInputLayout {
    let metrics = current_metrics(view, soft_wrap, cx);
    EditorInputLayout {
        bounds,
        visible_lines,
        row_height: rems(ROW_HEIGHT_REM).to_pixels(window.rem_size()),
        scroll_x: metrics
            .horizontal
            .map(|horizontal| horizontal.scrolled)
            .unwrap_or(Pixels::ZERO),
        scroll_y: metrics
            .vertical
            .map(|vertical| vertical.scrolled)
            .unwrap_or(Pixels::ZERO),
        text_left: rems(GUTTER_TOTAL_WIDTH_REM + BODY_PADDING_LEFT_REM)
            .to_pixels(window.rem_size()),
        char_width: monospace_char_width(window.rem_size()),
    }
}
