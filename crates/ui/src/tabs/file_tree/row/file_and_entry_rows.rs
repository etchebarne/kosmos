use gpui_component::input::Input;

pub fn render_file<T: PaneDelegate + SettingsDelegate>(
    window: &mut Window,
    entity: &Entity<FileTree>,
    path: PathBuf,
    name: SharedString,
    depth: usize,
    state: RowState,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let icon_name = icon_for_file(&path);

    let body = if state.is_renaming {
        rename_input_body::<T>(window, entity, depth, icon_name, cx)
    } else {
        node_label(depth, icon_name, name.clone(), state.is_selected, theme)
    };

    let entity_click = entity.clone();
    let entity_drop = entity.clone();
    let menu_entity = entity.clone();
    let click_path = path.clone();
    let drag_path = path.clone();
    let menu_target = path.clone();
    let drag_name = name.clone();
    let drop_filter_path = path.clone();
    let drop_target_dir = path.parent().map(Path::to_path_buf);
    let drop_highlight = drop_highlight_color(theme);

    let mut row = div()
        .id(path_id("file-tree-file-row", &path))
        .flex()
        .flex_shrink_0()
        .items_center()
        .h(ROW_HEIGHT)
        .px(rems(0.375))
        .text_sm()
        .text_color(if state.is_selected {
            theme.text_emphasis
        } else {
            theme.text
        })
        .when(!state.is_renaming, |this| {
            this.hover(move |s| s.bg(theme.bg_hover))
                .when(state.is_selected, |s| s.bg(theme.bg_selected))
                .drag_over::<FileNodeDrag>(move |style, _, _, _| style.bg(drop_highlight))
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
            let target = click_path.clone();
            let shift = event.modifiers().shift;
            entity_click.update(cx, |t, cx| {
                if shift {
                    t.extend_selection_to(target.clone(), cx);
                } else {
                    t.select(target.clone(), cx);
                }
            });
            if !shift {
                this.open_file(target, cx);
            }
        }))
        .can_drop({
            let parent = drop_target_dir.clone();
            move |drag, _, _| {
                let (Some(d), Some(parent)) =
                    (drag.downcast_ref::<FileNodeDrag>(), parent.as_ref())
                else {
                    return false;
                };
                can_drop_into_dir(d, parent)
            }
        })
        .on_drop(cx.listener(move |_, drag: &FileNodeDrag, _, cx| {
            cx.stop_propagation();
            let Some(dest) = drop_target_dir.clone() else {
                return;
            };
            let srcs = drag.paths.clone();
            entity_drop.update(cx, |t, cx| t.move_into(srcs, dest, cx));
        }))
        .child(body);

    let _ = drop_filter_path;

    if !state.is_renaming {
        let paths = drag_paths_for(entity, &drag_path, cx);
        row = row.on_drag(
            FileNodeDrag::new(paths, drag_name, icon_name, NodeKind::File),
            |drag, position, _, cx| cx.new(|_| drag.clone().position(position)),
        );
    }

    row.context_menu(move |popup_menu, window, cx| {
        super::menu::build(menu_entity.clone(), menu_target.clone(), popup_menu, window, cx)
    })
    .into_any_element()
}

pub fn render_new_entry<T: PaneDelegate + SettingsDelegate>(
    _window: &mut Window,
    draft: &NewEntryDraft,
    entity: &Entity<FileTree>,
    depth: usize,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let icon_name = match draft.kind {
        NodeKind::Directory => IconName::Folder,
        NodeKind::File => IconName::File,
    };
    let entity_submit = entity.clone();
    let entity_cancel = entity.clone();

    let Some(input) = cx.file_tree_ui().map(|ui| ui.input()) else {
        return div().into_any_element();
    };
    let input_for_submit = input.clone();

    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .h(ROW_HEIGHT)
        .px(rems(0.375))
        .text_sm()
        .bg(theme.bg_hover)
        .child(indent_guides(depth, theme))
        .child(
            div()
                .w(rems(ICON_WIDTH_REM))
                .h(ROW_HEIGHT)
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(icon_name).size(14.0).color(theme.text_muted)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .capture_key_down(cx.listener(
                    move |_, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<T>| {
                        match event.keystroke.key.as_str() {
                            "enter" => {
                                cx.stop_propagation();
                                let value = input_for_submit.read(cx).value().to_string();
                                entity_submit.update(cx, |t, cx| t.apply_new_entry(value, cx));
                            }
                            "escape" => {
                                cx.stop_propagation();
                                entity_cancel.update(cx, |t, cx| t.cancel_new_entry(cx));
                            }
                            _ => {}
                        }
                    },
                ))
                .child(Input::new(&input)),
        )
        .into_any_element()
}

pub fn rename_input_body<T: PaneDelegate + SettingsDelegate>(
    _window: &mut Window,
    entity: &Entity<FileTree>,
    depth: usize,
    icon_name: IconName,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let Some(input) = cx.file_tree_ui().map(|ui| ui.input()) else {
        return div().into_any_element();
    };
    let entity_submit = entity.clone();
    let entity_cancel = entity.clone();
    let input_for_submit = input.clone();

    div()
        .flex()
        .items_center()
        .h(ROW_HEIGHT)
        .w_full()
        .child(indent_guides(depth, theme))
        .child(
            div()
                .w(rems(ICON_WIDTH_REM))
                .h(ROW_HEIGHT)
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(icon_name).size(14.0).color(theme.text_muted)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .capture_key_down(cx.listener(
                    move |_, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<T>| {
                        match event.keystroke.key.as_str() {
                            "enter" => {
                                cx.stop_propagation();
                                let value = input_for_submit.read(cx).value().to_string();
                                entity_submit.update(cx, |t, cx| t.apply_rename(value, cx));
                            }
                            "escape" => {
                                cx.stop_propagation();
                                entity_cancel.update(cx, |t, cx| t.cancel_rename(cx));
                            }
                            _ => {}
                        }
                    },
                ))
                .child(Input::new(&input)),
        )
        .into_any_element()
}

fn node_label(
    depth: usize,
    icon: IconName,
    name: SharedString,
    is_selected: bool,
    theme: Theme,
) -> AnyElement {
    let icon_color = if is_selected {
        theme.text
    } else {
        theme.text_muted
    };

    div()
        .flex()
        .items_center()
        .h(ROW_HEIGHT)
        .w_full()
        .child(indent_guides(depth, theme))
        .child(
            div()
                .w(rems(ICON_WIDTH_REM))
                .h(ROW_HEIGHT)
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(icon).size(14.0).color(icon_color)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .pl(rems(0.25))
                .pr(rems(0.5))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(name),
        )
        .into_any_element()
}

fn icon_for_file(path: &Path) -> IconName {
    if let Some(name) = path.file_name().and_then(|n| n.to_str())
        && let Some(icon) = IconName::for_file_name(name)
    {
        return icon;
    }
    language::from_path(path)
        .and_then(|id| IconName::for_language(id.as_str()))
        .unwrap_or(IconName::File)
}

fn drop_highlight_color(theme: Theme) -> gpui::Hsla {
    gpui::Hsla::from(theme.accent).opacity(0.18)
}
