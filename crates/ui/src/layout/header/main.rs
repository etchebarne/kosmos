use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, Context, IntoElement, MouseButton, MouseDownEvent,
    SharedString, Window, WindowControlArea, anchored, deferred, div, ease_in_out, prelude::*,
    rems, svg,
};
use gpui_component::{
    Disableable,
    button::{Button, ButtonVariants},
};

use icons::{Icon, IconName};
use theme::{ActiveTheme, Theme};
use workspace::{Workspace, WorkspaceManager};

use crate::delegate::{
    HeaderDelegate, HeaderMenu, HeaderMenuAction, HeaderMenuAvailability, WorkspaceDelegate,
    WorkspaceMenuState,
};
use crate::drag::WorkspaceDrag;

pub fn render<T: HeaderDelegate>(
    active_menu: Option<HeaderMenu>,
    workspace_manager: &WorkspaceManager,
    workspace_menu: Option<WorkspaceMenuState>,
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
                    active_menu,
                    HeaderMenu::File,
                    "File",
                    menu_availability,
                    cx,
                ))
                .child(render_menu_button::<T>(
                    active_menu,
                    HeaderMenu::Edit,
                    "Edit",
                    menu_availability,
                    cx,
                ))
                .child(render_menu_button::<T>(
                    active_menu,
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
        .child(render_workspace_bar(
            workspace_manager,
            workspace_menu,
            window,
            cx,
        ))
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
