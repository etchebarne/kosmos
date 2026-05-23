#[derive(Clone, Copy)]
struct HeaderMenuItem {
    action: HeaderMenuAction,
    label: &'static str,
}

#[derive(Clone)]
struct HeaderPopupMenuItem {
    label: &'static str,
    shortcut: Option<Keystroke>,
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
                    .and_then(|action| shortcuts::primary_keystroke_for_action(action, cx)),
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
                        .child(Kbd::new(shortcut)),
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
