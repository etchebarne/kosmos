use std::{rc::Rc, time::Duration};

use gpui::{
    Animation, AnimationExt, AnyElement, App, ClickEvent, Context, IntoElement, MouseButton,
    SharedString, Window, WindowControlArea, div, ease_in_out, prelude::*, rems, svg,
};
use gpui_component::{
    Disableable, Icon as ComponentIcon, Sizable,
    button::{Button, ButtonVariants},
    menu::{ContextMenuExt, DropdownMenu, PopupMenuItem},
};

use icons::{Icon, IconName};
use theme::{ActiveTheme, Theme};
use workspace::{Workspace, WorkspaceManager};

use crate::delegate::{
    HeaderDelegate, HeaderMenu, HeaderMenuAction, HeaderMenuAvailability, WorkspaceDelegate,
};
use crate::drag::WorkspaceDrag;

pub fn render<T: HeaderDelegate>(
    workspace_manager: &WorkspaceManager,
    menu_availability: HeaderMenuAvailability,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    div()
        .id("app-header")
        .h(rems(2.5))
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .bg(theme.bg_surface)
        .rounded(rems(0.5))
        .border_1()
        .border_color(theme.border)
        .text_color(theme.text_header)
        .on_mouse_down(MouseButton::Left, |event, window, cx| {
            cx.stop_propagation();
            if event.click_count >= 2 {
                window.zoom_window();
            } else {
                window.start_window_move();
            }
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .px_1()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .h(rems(1.75))
                        .px(rems(0.5))
                        .child(
                            svg()
                                .path("brand/kosmos-icon.svg")
                                .h(rems(1.125))
                                .w(rems(1.35))
                                .flex_none()
                                .text_color(theme.text_emphasis),
                        ),
                )
                .child(render_menu_button::<T>(
                    HeaderMenu::File,
                    "File",
                    menu_availability,
                    cx,
                ))
                .child(render_menu_button::<T>(
                    HeaderMenu::Edit,
                    "Edit",
                    menu_availability,
                    cx,
                ))
                .child(render_menu_button::<T>(
                    HeaderMenu::Selection,
                    "Selection",
                    menu_availability,
                    cx,
                )),
        )
        .child(
            div()
                .flex_1()
                .h_full()
                .window_control_area(WindowControlArea::Drag),
        )
        .child(render_workspace_bar(workspace_manager, window, cx))
        .child(
            div()
                .flex_1()
                .h_full()
                .window_control_area(WindowControlArea::Drag),
        )
        .child(
            div()
                .flex()
                .items_center()
                .h_full()
                .child(render_window_button(
                    "window-minimize",
                    IconName::ChromeMinimize,
                    theme.bg_hover_strong,
                    WindowControlArea::Min,
                    false,
                    |window| window.minimize_window(),
                    &theme,
                ))
                .child(render_window_button(
                    "window-maximize",
                    IconName::ChromeMaximize,
                    theme.bg_hover_strong,
                    WindowControlArea::Max,
                    false,
                    |window| window.zoom_window(),
                    &theme,
                ))
                .child(render_window_button(
                    "window-close",
                    IconName::ChromeClose,
                    theme.danger,
                    WindowControlArea::Close,
                    true,
                    |window| window.remove_window(),
                    &theme,
                )),
        )
        .into_any_element()
}
