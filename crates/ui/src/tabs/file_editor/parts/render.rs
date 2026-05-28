pub fn render<T: 'static>(tab: &Tab, window: &mut Window, cx: &mut Context<T>) -> AnyElement {
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
    let editor_state = ComponentEditorStore::state_for_tab(
        tab.id,
        &buffer,
        file_tree_root.clone(),
        window,
        cx,
    );
    let input = editor_state.input;
    let completions = editor_state.completions;
    let input_focus = input.focus_handle(cx);
    let input_for_copy = input.clone();
    let input_for_cut = input.clone();
    let input_for_auto_pair = input.clone();

    div()
        .size_full()
        .min_h_0()
        .min_w_0()
        .flex()
        .flex_col()
        .bg(theme.bg_surface)
        .text_color(theme.text)
        .child(breadcrumb)
        .child(
            div()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .track_focus(&input_focus)
                .capture_action(cx.listener({
                    let completions = completions.clone();
                    move |_, _: &gpui_component::input::Enter, window, cx| {
                        let handled = completions.update(cx, |menu, cx| {
                            menu.accept_selected(window, cx)
                        });
                        if handled {
                            cx.stop_propagation();
                        } else {
                            cx.propagate();
                        }
                    }
                }))
                .capture_action(cx.listener({
                    let completions = completions.clone();
                    move |_, _: &gpui_component::input::IndentInline, window, cx| {
                        let handled = completions.update(cx, |menu, cx| {
                            menu.accept_selected(window, cx)
                        });
                        if handled {
                            cx.stop_propagation();
                        } else {
                            cx.propagate();
                        }
                    }
                }))
                .capture_action(cx.listener({
                    let completions = completions.clone();
                    move |_, _: &gpui_component::input::Escape, _, cx| {
                        let handled = completions.update(cx, |menu, cx| menu.hide(cx));
                        if handled {
                            cx.stop_propagation();
                        } else {
                            cx.propagate();
                        }
                    }
                }))
                .capture_action(cx.listener({
                    let completions = completions.clone();
                    move |_, _: &gpui_component::input::MoveUp, _, cx| {
                        let handled = completions.update(cx, |menu, cx| menu.select_previous(cx));
                        if handled {
                            cx.stop_propagation();
                        } else {
                            cx.propagate();
                        }
                    }
                }))
                .capture_action(cx.listener({
                    let completions = completions.clone();
                    move |_, _: &gpui_component::input::MoveDown, _, cx| {
                        let handled = completions.update(cx, |menu, cx| menu.select_next(cx));
                        if handled {
                            cx.stop_propagation();
                        } else {
                            cx.propagate();
                        }
                    }
                }))
                .on_action(cx.listener(move |_, _: &gpui_component::input::Copy, _, cx| {
                    copy_current_component_line(&input_for_copy, cx);
                }))
                .capture_action(cx.listener(
                    move |_, _: &gpui_component::input::Cut, window, cx| {
                        if cut_current_component_line(&input_for_cut, window, cx) {
                            cx.stop_propagation();
                        } else {
                            cx.propagate();
                        }
                    },
                ))
                .capture_key_down(move |event: &KeyDownEvent, window: &mut Window, cx: &mut App| {
                    if insert_component_auto_pair(&input_for_auto_pair, event, window, cx) {
                        cx.stop_propagation();
                    }
                })
                .child(
                    div()
                        .relative()
                        .size_full()
                        .child(
                            Input::new(&input)
                                .appearance(false)
                                .bordered(false)
                                .focus_bordered(false)
                                .font_family(FONT_FAMILY)
                                .text_size(rems(0.875))
                                .size_full(),
                        )
                        .child(completions),
                ),
        )
        .into_any_element()
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
