fn update_hover_source_bounds(
    hover: &LineHover,
    text_layout: &TextLayout,
    display_byte_offset: usize,
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
    let source_bounds = hover_source_bounds(
        hover,
        text_layout,
        display_byte_offset,
        source_bounds,
        &active,
        cx,
    );
    hover.view.update(cx, |view, _| {
        view.set_hover_source_bounds(hover.line_index, active.byte_range, source_bounds)
    });
    update_hover_visibility_at(&hover.view, window.mouse_position(), window, cx);
}

fn hover_source_bounds(
    hover: &LineHover,
    text_layout: &TextLayout,
    display_byte_offset: usize,
    source_bounds: Bounds<Pixels>,
    active: &file_editor::EditorHover,
    cx: &App,
) -> Bounds<Pixels> {
    let buffer = hover.buffer.read(cx);
    let Some(line) = buffer.line(active.line_index) else {
        return source_bounds;
    };
    let Some(display_range) = shift_range_for_display(
        active.byte_range.clone(),
        display_byte_offset,
        line.len().saturating_sub(display_byte_offset),
    ) else {
        return source_bounds;
    };
    let start = display_range.start;
    let end = display_range.end.max(start);
    let Some(start_position) = text_layout.position_for_index(start) else {
        return source_bounds;
    };

    let fallback_char_width =
        source_bounds.size.width / line[display_byte_offset..].chars().count().max(1) as f32;
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
