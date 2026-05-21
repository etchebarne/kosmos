#[derive(Clone, Copy)]
struct HeaderMenuItem {
    action: HeaderMenuAction,
    label: &'static str,
}

impl HeaderMenuItem {
    const fn new(action: HeaderMenuAction, label: &'static str) -> Self {
        Self { action, label }
    }
}

const FILE_MENU_ITEMS: &[HeaderMenuItem] = &[
    HeaderMenuItem::new(HeaderMenuAction::OpenFolder, "Open Folder..."),
    HeaderMenuItem::new(HeaderMenuAction::Save, "Save"),
    HeaderMenuItem::new(HeaderMenuAction::SaveAll, "Save All"),
];

const EDIT_MENU_ITEMS: &[HeaderMenuItem] = &[
    HeaderMenuItem::new(HeaderMenuAction::Undo, "Undo"),
    HeaderMenuItem::new(HeaderMenuAction::Redo, "Redo"),
    HeaderMenuItem::new(HeaderMenuAction::Cut, "Cut"),
    HeaderMenuItem::new(HeaderMenuAction::Copy, "Copy"),
    HeaderMenuItem::new(HeaderMenuAction::Paste, "Paste"),
];

const SELECTION_MENU_ITEMS: &[HeaderMenuItem] = &[
    HeaderMenuItem::new(HeaderMenuAction::SelectAll, "Select All"),
    HeaderMenuItem::new(HeaderMenuAction::ExpandSelection, "Expand Selection"),
    HeaderMenuItem::new(HeaderMenuAction::ShrinkSelection, "Shrink Selection"),
];

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
    availability: HeaderMenuAvailability,
    cx: &mut Context<T>,
) -> impl IntoElement + 'static {
    let theme = *cx.theme();
    let is_enabled = availability.menu_enabled(menu);
    let is_active = is_enabled && active_menu == Some(menu);
    let dropdown = is_active.then(|| render_menu_dropdown::<T>(menu, availability, &theme, cx));

    div()
        .id(("menu-button", menu.id()))
        .relative()
        .h(rems(1.75))
        .px_3()
        .flex()
        .items_center()
        .rounded(rems(0.3125))
        .text_sm()
        .text_color(if is_enabled {
            theme.text_header
        } else {
            theme.text_muted
        })
        .bg(if is_active {
            theme.bg_selected
        } else {
            theme.bg_surface
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .when(is_enabled, |this| {
            this.hover(move |this| this.bg(theme.bg_hover).text_color(theme.text_emphasis))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.toggle_header_menu(menu, cx);
                }))
        })
        .when(!is_enabled, |this| {
            this.opacity(0.45)
                .on_click(|_, _, cx| cx.stop_propagation())
        })
        .child(label)
        .children(dropdown)
}

fn render_menu_dropdown<T: HeaderDelegate>(
    menu: HeaderMenu,
    availability: HeaderMenuAvailability,
    theme: &Theme,
    cx: &mut Context<T>,
) -> AnyElement {
    let item_elements = menu_items(menu)
        .iter()
        .enumerate()
        .map(|(index, item)| {
            render_menu_item::<T>(
                menu,
                index,
                *item,
                availability.action_enabled(item.action),
                theme,
                cx,
            )
        })
        .collect::<Vec<_>>();

    deferred(
        div()
            .id(("menu-dropdown", menu.id()))
            .absolute()
            .top(rems(2.0))
            .left(rems(0.0))
            .w(rems(17.0))
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

fn render_menu_item<T: HeaderDelegate>(
    menu: HeaderMenu,
    index: usize,
    item: HeaderMenuItem,
    is_enabled: bool,
    theme: &Theme,
    cx: &mut Context<T>,
) -> AnyElement {
    let action = item.action;
    let shortcut = item
        .action
        .shortcut_action_name()
        .and_then(|action| shortcuts::primary_label_for_action(action, cx))
        .map(SharedString::from);

    Button::new(("menu-item", menu.id() * 100 + index))
        .ghost()
        .tab_stop(false)
        .disabled(!is_enabled)
        .h(rems(1.75))
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .child(div().flex_1().min_w_0().child(item.label))
                .when_some(shortcut, |this, shortcut| {
                    this.child(
                        div()
                            .flex_none()
                            .text_color(theme.text_muted)
                            .child(shortcut),
                    )
                }),
        )
        .on_click(cx.listener(move |this, _, window, cx| {
            cx.stop_propagation();
            if is_enabled {
                this.activate_header_menu_action(action, window, cx);
            }
        }))
        .into_any_element()
}

fn menu_items(menu: HeaderMenu) -> &'static [HeaderMenuItem] {
    match menu {
        HeaderMenu::File => FILE_MENU_ITEMS,
        HeaderMenu::Edit => EDIT_MENU_ITEMS,
        HeaderMenu::Selection => SELECTION_MENU_ITEMS,
    }
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
