use gpui::{AnyElement, IntoElement, MouseButton, div, prelude::*, rems};
use theme::Theme;

pub fn render(
    id: &'static str,
    title: &'static str,
    body: AnyElement,
    footer: AnyElement,
    theme: Theme,
    on_close: impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .bottom_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(gpui::Hsla::from(theme.bg_root).opacity(0.72))
        .on_mouse_down(MouseButton::Left, on_close)
        .child(
            div()
                .w(rems(30.0))
                .max_w(rems(40.0))
                .max_h(rems(34.0))
                .flex()
                .flex_col()
                .rounded(rems(0.5))
                .border_1()
                .border_color(theme.border_strong)
                .bg(theme.bg_surface)
                .shadow_lg()
                .text_color(theme.text)
                .block_mouse_except_scroll()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .flex_none()
                        .border_b_1()
                        .border_color(theme.border_subtle)
                        .px_4()
                        .py_3()
                        .text_sm()
                        .text_color(theme.text_emphasis)
                        .child(title),
                )
                .child(
                    div()
                        .id("modal-body")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .p_4()
                        .child(body),
                )
                .child(
                    div()
                        .flex_none()
                        .border_t_1()
                        .border_color(theme.border_subtle)
                        .px_4()
                        .py_3()
                        .child(footer),
                ),
        )
        .into_any_element()
}
