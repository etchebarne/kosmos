use gpui::{AnyElement, App, IntoElement, SharedString, div, prelude::*, rems};
use gpui_component::{ActiveTheme, WindowExt, notification::Notification};

#[derive(Clone, Copy)]
enum ToastKind {
    Success,
    Error,
}

pub fn show_success(cx: &mut App, title: impl Into<SharedString>) {
    push_notification(
        cx,
        toast_notification(ToastKind::Success, title.into(), None),
    );
}

pub fn show_error(cx: &mut App, title: impl Into<SharedString>, message: impl Into<SharedString>) {
    push_notification(
        cx,
        toast_notification(ToastKind::Error, title.into(), Some(message.into())),
    );
}

fn toast_notification(
    kind: ToastKind,
    title: SharedString,
    message: Option<SharedString>,
) -> Notification {
    Notification::new()
        .content(move |_, _, cx| toast_content(kind, title.clone(), message.clone(), cx))
}

fn toast_content(
    kind: ToastKind,
    title: SharedString,
    message: Option<SharedString>,
    cx: &mut gpui::Context<Notification>,
) -> AnyElement {
    let theme = cx.theme();
    let accent = match kind {
        ToastKind::Success => theme.success,
        ToastKind::Error => theme.danger,
    };

    div()
        .flex()
        .items_start()
        .gap_2()
        .child(
            div()
                .mt(rems(0.375))
                .size(rems(0.5))
                .flex_none()
                .rounded(rems(0.25))
                .bg(accent),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.popover_foreground)
                        .child(title),
                )
                .when_some(message, |this, message| {
                    this.child(
                        div()
                            .text_xs()
                            .line_height(rems(1.2))
                            .text_color(theme.muted_foreground)
                            .child(message),
                    )
                }),
        )
        .into_any_element()
}

fn push_notification(cx: &mut App, notification: Notification) {
    let Some(window_handle) = cx
        .active_window()
        .or_else(|| cx.windows().into_iter().next())
    else {
        return;
    };

    let _ = window_handle.update(cx, move |_, window, cx| {
        window.push_notification(notification, cx);
    });
}
