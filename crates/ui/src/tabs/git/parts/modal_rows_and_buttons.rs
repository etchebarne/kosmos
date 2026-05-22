fn tag_row(root: PathBuf, tag: Tag, cx: &mut App) -> AnyElement {
    let name = tag.name.clone();
    list_row(
        tag.name,
        tag.message,
        move |_, _, cx| {
            let name = name.clone();
            run_modal_action_app(
                root.clone(),
                GitModal::Tags,
                move |root| kosmos_git::delete_tag(root, &name),
                cx,
            );
        },
        cx,
    )
}

fn list_row(
    title: String,
    subtitle: String,
    delete: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut App,
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

fn delete_button(
    id: impl Into<gpui::ElementId>,
    listener: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut App,
) -> AnyElement {
    let theme = *cx.theme();
    icon_action_button(id, IconName::Trash, theme.danger, listener, cx)
}

fn icon_action_button(
    id: impl Into<gpui::ElementId>,
    icon: IconName,
    color: gpui::Rgba,
    listener: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut App,
) -> AnyElement {
    let _ = cx;
    Button::new(id)
        .ghost()
        .small()
        .tab_stop(false)
        .size(rems(1.375))
        .text_color(color)
        .icon(component_icon(icon))
        .on_click(move |event, window, cx| {
            cx.stop_propagation();
            listener(event, window, cx);
        })
        .into_any_element()
}

fn modal_footer(button: AnyElement, _cx: &mut App) -> AnyElement {
    div().flex().justify_end().child(button).into_any_element()
}

fn close_modal_button(cx: &mut App) -> AnyElement {
    action_button(
        "git-close-modal",
        "Close",
        false,
        |_, window, cx| {
            close_modal(cx);
            window.close_dialog(cx);
        },
        cx,
    )
}

fn action_button(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    danger: bool,
    listener: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut App,
) -> AnyElement {
    let _ = cx;
    Button::new(id)
        .outline()
        .when(danger, |this| this.danger())
        .label(label)
        .on_click(listener)
        .into_any_element()
}

fn commit_button<T: PaneDelegate + SettingsDelegate>(
    enabled: bool,
    listener: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut Context<T>,
) -> AnyElement {
    let _ = cx;
    Button::new("git-commit-tracked")
        .primary()
        .disabled(!enabled)
        .label("Commit Tracked")
        .on_click(listener)
        .into_any_element()
}
