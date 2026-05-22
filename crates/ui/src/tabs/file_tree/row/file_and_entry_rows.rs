use gpui_component::input::Input;

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
