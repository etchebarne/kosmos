fn render_workspace_menu<T: WorkspaceDelegate>(
    state: WorkspaceMenuState,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let id = state.id;
    let hover_bg = theme.bg_selected;
    let hover_text = theme.text_emphasis;

    let item = div()
        .id("workspace-menu-close")
        .flex()
        .items_center()
        .gap_2()
        .h(rems(1.625))
        .px_2()
        .rounded(rems(0.25))
        .text_color(theme.text)
        .hover(move |this| this.bg(hover_bg).text_color(hover_text))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.close_workspace(id, cx);
            this.close_workspace_menu(cx);
        }))
        .child(
            div()
                .w(rems(1.0))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(IconName::Close)
                        .size(14.0)
                        .color(theme.text_muted),
                ),
        )
        .child("Close");

    deferred(
        anchored().position(state.position).snap_to_window().child(
            div()
                .id("workspace-context-menu")
                .min_w(rems(10.0))
                .p_1()
                .flex()
                .flex_col()
                .gap_0p5()
                .rounded(rems(0.375))
                .border_1()
                .border_color(theme.border_strong)
                .bg(theme.bg_elevated)
                .shadow_lg()
                .text_sm()
                .text_color(theme.text)
                .block_mouse_except_scroll()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                .child(item),
        ),
    )
    .with_priority(2)
    .into_any_element()
}

fn render_workspace_menu_dismiss<T: WorkspaceDelegate>(cx: &mut Context<T>) -> AnyElement {
    deferred(
        div()
            .id("workspace-menu-dismiss")
            .occlude()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.close_workspace_menu(cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.close_workspace_menu(cx);
                }),
            ),
    )
    .with_priority(1)
    .into_any_element()
}

fn render_menu_button<T: HeaderDelegate>(
    active_menu: Option<HeaderMenu>,
    menu: HeaderMenu,
    label: &'static str,
    items: &'static [&'static str],
    cx: &mut Context<T>,
) -> impl IntoElement + 'static {
    let theme = *cx.theme();
    let is_active = active_menu == Some(menu);
    let dropdown = is_active.then(|| render_menu_dropdown(menu, items, &theme));

    div()
        .id(("menu-button", menu.id()))
        .relative()
        .h(rems(1.75))
        .px_3()
        .flex()
        .items_center()
        .rounded(rems(0.3125))
        .text_sm()
        .bg(if is_active {
            theme.bg_selected
        } else {
            theme.bg_surface
        })
        .hover(move |this| this.bg(theme.bg_hover).text_color(theme.text_emphasis))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.toggle_header_menu(menu, cx);
        }))
        .child(label)
        .children(dropdown)
}

fn render_menu_dropdown(menu: HeaderMenu, items: &[&'static str], theme: &Theme) -> AnyElement {
    let item_text = theme.text_header;
    let item_hover_bg = theme.bg_selected;
    let item_hover_text = theme.text_emphasis;

    let mut item_elements = Vec::new();
    for (index, item) in items.iter().enumerate() {
        item_elements.push(
            div()
                .id(("menu-item", menu.id() * 100 + index))
                .h(rems(1.75))
                .px_3()
                .flex()
                .items_center()
                .rounded(rems(0.25))
                .text_sm()
                .text_color(item_text)
                .hover(move |this| this.bg(item_hover_bg).text_color(item_hover_text))
                .child(*item),
        );
    }

    deferred(
        div()
            .id(("menu-dropdown", menu.id()))
            .absolute()
            .top(rems(2.0))
            .left(rems(0.0))
            .w(rems(11.5))
            .p_1()
            .flex()
            .flex_col()
            .gap_1()
            .rounded(rems(0.375))
            .border_1()
            .border_color(theme.border_strong)
            .bg(theme.bg_elevated)
            .shadow_lg()
            .block_mouse_except_scroll()
            .children(item_elements),
    )
    .into_any_element()
}

fn render_window_button(
    id: &'static str,
    icon: IconName,
    hover_background: gpui::Rgba,
    control_area: WindowControlArea,
    round_right: bool,
    action: impl Fn(&mut Window) + 'static,
    theme: &Theme,
) -> impl IntoElement + 'static {
    let text_color = theme.text_muted;
    let hover_text = theme.text_emphasis;
    div()
        .id(id)
        .h_full()
        .w(rems(2.875))
        .flex()
        .items_center()
        .justify_center()
        .text_sm()
        .text_color(text_color)
        .when(round_right, |this| this.rounded_r(rems(0.4375)))
        .window_control_area(control_area)
        .hover(move |this| this.bg(hover_background).text_color(hover_text))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            action(window);
        })
        .child(Icon::new(icon).size(16.0).color(text_color))
}
