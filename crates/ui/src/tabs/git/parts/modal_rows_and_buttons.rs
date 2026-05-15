fn tag_row<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    tag: Tag,
    cx: &mut Context<T>,
) -> AnyElement {
    let name = tag.name.clone();
    list_row(
        tag.name,
        tag.message,
        cx.listener(move |_, _, _, cx| {
            let name = name.clone();
            run_modal_action(
                root.clone(),
                GitModal::Tags,
                move |root| kosmos_git::delete_tag(root, &name),
                cx,
            );
        }),
        cx,
    )
}

fn list_row<T: PaneDelegate + SettingsDelegate>(
    title: String,
    subtitle: String,
    delete: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .rounded(rems(0.375))
        .border_1()
        .border_color(theme.border_subtle)
        .p_2()
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_sm().text_color(theme.text).child(title))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.text_subtle)
                        .child(subtitle),
                ),
        )
        .child(delete_button("git-delete-list-item", delete, cx))
        .into_any_element()
}

fn delete_button<T: PaneDelegate + SettingsDelegate>(
    id: impl Into<gpui::ElementId>,
    listener: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    icon_action_button(id, IconName::Trash, theme.danger, listener, cx)
}

fn icon_action_button<T: PaneDelegate + SettingsDelegate>(
    id: impl Into<gpui::ElementId>,
    icon: IconName,
    color: gpui::Rgba,
    listener: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    div()
        .id(id)
        .size(rems(1.375))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(rems(0.25))
        .text_color(color)
        .hover(move |this| this.bg(theme.bg_hover))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |event, window, cx| {
            cx.stop_propagation();
            listener(event, window, cx);
        })
        .child(Icon::new(icon).size(14.0).color(color))
        .into_any_element()
}

fn modal_footer<T: PaneDelegate + SettingsDelegate>(
    button: AnyElement,
    _cx: &mut Context<T>,
) -> AnyElement {
    div().flex().justify_end().child(button).into_any_element()
}

fn close_modal_button<T: PaneDelegate + SettingsDelegate>(cx: &mut Context<T>) -> AnyElement {
    action_button(
        "git-close-modal",
        "Close",
        false,
        cx.listener(|_, _, _, cx| close_modal(cx)),
        cx,
    )
}

fn action_button<T: PaneDelegate + SettingsDelegate>(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    danger: bool,
    listener: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    div()
        .id(id)
        .rounded(rems(0.3125))
        .border_1()
        .border_color(if danger { theme.danger } else { theme.border })
        .bg(theme.bg_elevated)
        .px_3()
        .py_1()
        .text_sm()
        .text_color(if danger { theme.danger } else { theme.text })
        .hover(move |this| this.bg(theme.bg_hover))
        .on_click(listener)
        .child(label)
        .into_any_element()
}

fn commit_button<T: PaneDelegate + SettingsDelegate>(
    enabled: bool,
    listener: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    div()
        .id("git-commit-tracked")
        .rounded(rems(0.3125))
        .border_1()
        .border_color(if enabled {
            gpui::Hsla::from(theme.accent)
        } else {
            gpui::Hsla::from(theme.border)
        })
        .bg(if enabled {
            theme.accent
        } else {
            theme.bg_elevated
        })
        .px_3()
        .py_1()
        .text_sm()
        .text_color(if enabled {
            theme.bg_surface
        } else {
            theme.text_subtle
        })
        .when(enabled, |this| {
            this.hover(move |this| this.bg(gpui::Hsla::from(theme.accent).opacity(0.85)))
                .on_click(listener)
        })
        .child("Commit Tracked")
        .into_any_element()
}

