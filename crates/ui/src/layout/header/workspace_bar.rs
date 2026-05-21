fn render_workspace_bar<T: WorkspaceDelegate>(
    manager: &WorkspaceManager,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let active = manager.active_id();
    let previous_active = manager.previous_active_id();
    let active_changed = active != previous_active;
    let mut elements: Vec<AnyElement> = Vec::new();
    for workspace in manager.workspaces() {
        let is_active = active == Some(workspace.id);
        let should_animate = active_changed
            && (Some(workspace.id) == active || Some(workspace.id) == previous_active);
        elements.push(render_workspace_button(
            workspace,
            is_active,
            should_animate,
            window,
            cx,
        ));
    }
    elements.push(render_add_button(cx));

    div()
        .flex()
        .items_center()
        .gap_1()
        .px_1()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .children(elements)
        .into_any_element()
}

fn render_add_button<T: WorkspaceDelegate>(cx: &mut Context<T>) -> AnyElement {
    let theme = *cx.theme();
    let hover_group = SharedString::from("workspace-add");
    let accent = theme.accent;
    div()
        .id("workspace-add")
        .group(hover_group.clone())
        .relative()
        .size(rems(1.75))
        .flex()
        .items_center()
        .justify_center()
        .rounded(rems(0.3125))
        .text_color(theme.text_muted)
        .hover(move |this| this.bg(theme.bg_hover).text_color(theme.text_emphasis))
        .can_drop(|drag, _, _| drag.downcast_ref::<WorkspaceDrag>().is_some())
        .on_drop(cx.listener(|this, drag: &WorkspaceDrag, _, cx| {
            cx.stop_propagation();
            this.move_workspace_to_end(drag.id, cx);
        }))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(|this, _, _, cx| {
            cx.stop_propagation();
            this.open_workspace_picker(cx);
        }))
        .child(
            div()
                .absolute()
                .left(rems(-0.1875))
                .top(rems(0.25))
                .bottom(rems(0.25))
                .w(rems(0.125))
                .rounded_full()
                .hover(|s| s)
                .group_drag_over::<WorkspaceDrag>(hover_group, move |s| s.bg(accent)),
        )
        .child(Icon::new(IconName::Add).size(16.0).color(theme.text_muted))
        .into_any_element()
}

fn render_workspace_button<T: WorkspaceDelegate>(
    workspace: &Workspace,
    is_active: bool,
    should_animate: bool,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let id = workspace.id;
    let initial = SharedString::from(workspace.initial());
    let name = SharedString::from(workspace.name.clone());
    let hover_group = SharedString::from(format!("workspace-{id}"));
    let accent = theme.accent;
    let drag_initial = initial.clone();
    let close_listener: HeaderMenuHandler = Rc::new(cx.listener(move |this, _, _, cx| {
        this.close_workspace(id, cx);
    }));

    let inactive_w = 1.75_f32;
    let active_w = measure_text_rems(window, name.as_ref()) + 1.25;
    let anim_id = SharedString::from(format!("ws-anim-{id}-{}", is_active as u8));

    let content = div().relative().h_full().overflow_hidden();
    let content = if should_animate {
        content
            .with_animation(
                anim_id,
                Animation::new(Duration::from_millis(180)).with_easing(ease_in_out),
                move |el, delta| {
                    let animation_progress = if is_active { delta } else { 1.0 - delta };
                    let width_rem = inactive_w + (active_w - inactive_w) * animation_progress;
                    el.w(rems(width_rem))
                        .child(
                            div()
                                .absolute()
                                .top(rems(0.))
                                .left(rems(0.))
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .opacity(1.0 - animation_progress)
                                .child(initial.clone()),
                        )
                        .child(
                            div()
                                .absolute()
                                .top(rems(0.))
                                .left(rems(0.))
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .opacity(animation_progress)
                                .child(name.clone()),
                        )
                },
            )
            .into_any_element()
    } else {
        let width_rem = if is_active { active_w } else { inactive_w };
        content
            .w(rems(width_rem))
            .flex()
            .items_center()
            .justify_center()
            .child(if is_active { name } else { initial })
            .into_any_element()
    };

    div()
        .id(("workspace", id))
        .group(hover_group.clone())
        .relative()
        .h(rems(1.75))
        .flex()
        .items_center()
        .rounded(rems(0.3125))
        .text_sm()
        .bg(if is_active {
            theme.bg_selected
        } else {
            theme.bg_surface
        })
        .text_color(if is_active {
            theme.text_emphasis
        } else {
            theme.text_muted
        })
        .hover(move |this| this.bg(theme.bg_hover).text_color(theme.text_emphasis))
        .can_drop(move |drag, _, _| {
            drag.downcast_ref::<WorkspaceDrag>()
                .is_some_and(|drag| drag.id != id)
        })
        .on_drop(cx.listener(move |this, drag: &WorkspaceDrag, _, cx| {
            cx.stop_propagation();
            this.move_workspace_before(drag.id, id, cx);
        }))
        .on_drag(
            WorkspaceDrag::new(id, drag_initial),
            |drag, position, _, cx| cx.new(|_| drag.clone().position(position)),
        )
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.select_workspace(id, cx);
        }))
        .child(
            div()
                .absolute()
                .left(rems(-0.1875))
                .top(rems(0.25))
                .bottom(rems(0.25))
                .w(rems(0.125))
                .rounded_full()
                .hover(|s| s)
                .group_drag_over::<WorkspaceDrag>(hover_group, move |s| s.bg(accent)),
        )
        .child(content)
        .context_menu(move |menu, window, _| {
            let menu_width = rems(10.0).to_pixels(window.rem_size());
            menu.min_w(menu_width).item(workspace_menu_item(
                "Close",
                ComponentIcon::empty().path(IconName::Close.path()),
                close_listener.clone(),
            ))
        })
        .into_any_element()
}

fn workspace_menu_item(
    label: &'static str,
    icon: ComponentIcon,
    listener: HeaderMenuHandler,
) -> PopupMenuItem {
    PopupMenuItem::element(move |_, cx| {
        let theme = *cx.theme();
        div()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .text_color(theme.text)
            .child(
                div()
                    .w(rems(1.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme.text_muted)
                    .child(icon.clone().small()),
            )
            .child(label)
    })
    .on_click(move |event, window, cx| listener(event, window, cx))
}

fn measure_text_rems(window: &mut Window, text: &str) -> f32 {
    let style = window.text_style();
    let run = style.to_run(text.len());
    let rem_size = window.rem_size();
    let font_size = rem_size * 0.875_f32;
    let layout = window
        .text_system()
        .layout_line(text, font_size, &[run], None);
    f32::from(layout.width) / f32::from(rem_size)
}
