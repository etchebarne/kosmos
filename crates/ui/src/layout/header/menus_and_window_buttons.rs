#[derive(Clone, Copy)]
struct HeaderMenuItem {
    action: HeaderMenuAction,
    label: &'static str,
}

#[derive(Clone)]
struct HeaderPopupMenuItem {
    label: &'static str,
    shortcut: Option<SharedString>,
    is_enabled: bool,
    listener: HeaderMenuHandler,
}

type HeaderMenuHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

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

    let item = Button::new("workspace-menu-close")
        .ghost()
        .tab_stop(false)
        .w_full()
        .h(rems(1.625))
        .icon(ComponentIcon::empty().path(IconName::Close.path()))
        .child(left_aligned_button_label("Close"))
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.close_workspace(id, cx);
            this.close_workspace_menu(cx);
        }));

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
    menu: HeaderMenu,
    label: &'static str,
    availability: HeaderMenuAvailability,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let is_enabled = availability.menu_enabled(menu);

    let button = Button::new(("menu-button", menu.id()))
        .ghost()
        .tab_stop(false)
        .disabled(!is_enabled)
        .h(rems(1.75))
        .px_3()
        .text_sm()
        .text_color(if is_enabled {
            theme.text_header
        } else {
            theme.text_muted
        })
        .hover(move |this| this.bg(theme.bg_hover).text_color(theme.text_emphasis))
        .when(!is_enabled, |this| this.opacity(0.45))
        .child(label);

    if !is_enabled {
        return button.into_any_element();
    }

    let items = menu_items(menu)
        .iter()
        .map(|item| {
            let action = item.action;
            let listener: HeaderMenuHandler = Rc::new(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.activate_header_menu_action(action, window, cx);
            }));

            HeaderPopupMenuItem {
                label: item.label,
                shortcut: action
                    .shortcut_action_name()
                    .and_then(|action| shortcuts::primary_label_for_action(action, cx))
                    .map(SharedString::from),
                is_enabled: availability.action_enabled(action),
                listener,
            }
        })
        .collect::<Vec<_>>();

    button
        .dropdown_menu(move |popup_menu, window, _| {
            let menu_width = rems(17.0).to_pixels(window.rem_size());
            items.iter().cloned().fold(
                popup_menu.min_w(menu_width).max_w(menu_width),
                |popup_menu, item| popup_menu.item(render_menu_item(item)),
            )
        })
        .into_any_element()
}

fn render_menu_item(item: HeaderPopupMenuItem) -> PopupMenuItem {
    let HeaderPopupMenuItem {
        label,
        shortcut,
        is_enabled,
        listener,
    } = item;

    PopupMenuItem::element(move |_, cx| {
        let theme = *cx.theme();
        let text_color = if is_enabled {
            theme.text
        } else {
            theme.text_subtle
        };
        let shortcut_color = if is_enabled {
            theme.text_muted
        } else {
            theme.text_subtle
        };

        div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .text_color(text_color)
            .child(div().flex_1().min_w_0().child(label))
            .when_some(shortcut.clone(), |this, shortcut| {
                this.child(
                    div()
                        .flex_none()
                        .text_color(shortcut_color)
                        .child(shortcut),
                )
            })
    })
    .disabled(!is_enabled)
    .on_click(move |event, window, cx| listener(event, window, cx))
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
