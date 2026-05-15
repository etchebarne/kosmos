fn render_gutter(
    row_index: usize,
    line_number: Option<usize>,
    sticky_offset: Pixels,
    foldable: bool,
    folded: bool,
    show_fold_arrow: bool,
    hovered_fold_line: Option<usize>,
    view: &Entity<EditorView>,
    theme: Theme,
) -> impl IntoElement {
    let label: SharedString = match line_number {
        Some(n) => format!("{n}").into(),
        None => SharedString::default(),
    };
    let mut gutter = div()
        .id(gpui::ElementId::Name(
            format!("file-editor-gutter:{:?}:{row_index}", view.entity_id()).into(),
        ))
        .absolute()
        .top_0()
        .bottom_0()
        .left(sticky_offset)
        .w(rems(GUTTER_TOTAL_WIDTH_REM))
        .pr(rems(GUTTER_PADDING_REM + GUTTER_FOLD_COLUMN_REM))
        .text_right()
        .text_color(theme.text_subtle)
        .bg(theme.bg_surface)
        .child(label);

    if foldable {
        let arrow_color = if hovered_fold_line == Some(row_index) {
            theme.text_emphasis
        } else {
            theme.text_subtle
        };
        let icon_name = if folded {
            IconName::ChevronRight
        } else {
            IconName::ChevronDown
        };
        let view_for_click = view.clone();
        let mut arrow = div()
            .id(gpui::ElementId::Name(
                format!("file-editor-fold-arrow:{:?}:{row_index}", view.entity_id()).into(),
            ))
            .absolute()
            .left(rems(GUTTER_FOLD_HOVER_LEFT_REM))
            .top(rems(0.0))
            .h_full()
            .w(rems(GUTTER_FOLD_HOVER_WIDTH_REM))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                view_for_click.update(cx, |view, _| view.toggle_folded_line(row_index));
                window.refresh();
            });

        if show_fold_arrow {
            arrow = arrow.child(Icon::new(icon_name).size(12.0).color(arrow_color));
        }

        gutter = gutter.child(arrow);
    }

    gutter
}

fn update_gutter_hover_from_mouse(
    view: &Entity<EditorView>,
    soft_wrap: bool,
    visible_lines: &[usize],
    foldable_lines: &[bool],
    position: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let rem_size = window.rem_size();
    let gutter_hover_width =
        rems(GUTTER_TOTAL_WIDTH_REM + GUTTER_HOVER_RIGHT_SLOP_REM).to_pixels(rem_size);
    let fold_hover_left = rems(GUTTER_FOLD_HOVER_LEFT_REM).to_pixels(rem_size);
    let fold_hover_right =
        rems(GUTTER_TOTAL_WIDTH_REM + GUTTER_FOLD_HOVER_RIGHT_SLOP_REM).to_pixels(rem_size);
    let (gutter_hovered, hovered_fold_line) = {
        let view_ref = view.read(cx);
        let Some(bounds) = view_ref.editor_bounds() else {
            return;
        };
        match bounds.localize(&position) {
            Some(local) if local.x >= Pixels::ZERO && local.x <= gutter_hover_width => {
                let line = hovered_row_index(&view_ref, soft_wrap, local.y, window)
                    .and_then(|row| visible_lines.get(row).copied());
                let in_fold_hover_zone = local.x >= fold_hover_left && local.x <= fold_hover_right;
                let hovered_fold_line = line.filter(|line| {
                    in_fold_hover_zone && foldable_lines.get(*line).copied().unwrap_or(false)
                });
                (true, hovered_fold_line)
            }
            _ => (false, None),
        }
    };
    update_gutter_hover_state(view, gutter_hovered, hovered_fold_line, window, cx);
}

fn hovered_row_index(
    view: &EditorView,
    soft_wrap: bool,
    local_y: Pixels,
    window: &mut Window,
) -> Option<usize> {
    if local_y < Pixels::ZERO {
        return None;
    }

    if soft_wrap {
        view.virtual_scroll()
            .visible_rows()
            .into_iter()
            .find_map(|(index, top, bottom)| (local_y >= top && local_y < bottom).then_some(index))
    } else {
        let row_height = rems(ROW_HEIGHT_REM).to_pixels(window.rem_size());
        if row_height <= Pixels::ZERO {
            return None;
        }
        let scroll_y = -view.uniform_scroll().0.borrow().base_handle.offset().y;
        Some(((local_y + scroll_y) / row_height).floor() as usize)
    }
}

fn update_gutter_hover_state(
    view: &Entity<EditorView>,
    hovered: bool,
    hovered_fold_line: Option<usize>,
    window: &mut Window,
    cx: &mut App,
) {
    let changed = view.update(cx, |view, _| {
        view.set_gutter_hover_state(hovered, hovered_fold_line)
    });
    if changed {
        window.refresh();
    }
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
            Icon::new(super::icon_for_kind(registry::FILE_EDITOR.id))
                .size(32.0)
                .color(theme.text_muted),
        )
        .child(div().text_sm().child("No file"))
        .into_any_element()
}
