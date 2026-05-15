use std::time::Duration;

use gpui::{
    AnyElement, App, ClickEvent, ElementId, Global, IntoElement, SharedString, Window, div,
    prelude::*, rems,
};
use icons::{Icon, IconName};
use theme::ActiveTheme;

const TOAST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_TOASTS: usize = 4;

#[derive(Clone, Copy)]
enum ToastKind {
    Success,
    Error,
}

#[derive(Clone)]
struct Toast {
    id: u64,
    kind: ToastKind,
    title: SharedString,
    message: Option<SharedString>,
}

#[derive(Default)]
struct ToastState {
    next_id: u64,
    toasts: Vec<Toast>,
}

impl Global for ToastState {}

pub fn show_success(cx: &mut App, title: impl Into<SharedString>) {
    show(cx, ToastKind::Success, title.into(), None);
}

pub fn show_error(cx: &mut App, title: impl Into<SharedString>, message: impl Into<SharedString>) {
    show(cx, ToastKind::Error, title.into(), Some(message.into()));
}

pub fn render(cx: &mut App) -> AnyElement {
    let toasts = cx
        .try_global::<ToastState>()
        .map(|state| state.toasts.clone())
        .unwrap_or_default();

    div()
        .id("toast-region")
        .absolute()
        .bottom(rems(3.0))
        .right(rems(1.0))
        .w(rems(24.0))
        .flex()
        .flex_col()
        .gap_2()
        .children(toasts.into_iter().map(|toast| toast_element(toast, cx)))
        .into_any_element()
}

fn show(cx: &mut App, kind: ToastKind, title: SharedString, message: Option<SharedString>) {
    let id = {
        let state = cx.default_global::<ToastState>();
        let id = state.next_id;
        state.next_id = state.next_id.wrapping_add(1);
        state.toasts.push(Toast {
            id,
            kind,
            title,
            message,
        });
        let overflow = state.toasts.len().saturating_sub(MAX_TOASTS);
        if overflow > 0 {
            state.toasts.drain(0..overflow);
        }
        id
    };

    cx.refresh_windows();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(TOAST_TIMEOUT).await;
        let _ = cx.update(|cx| dismiss(id, cx));
    })
    .detach();
}

fn dismiss(id: u64, cx: &mut App) {
    let state = cx.default_global::<ToastState>();
    let before = state.toasts.len();
    state.toasts.retain(|toast| toast.id != id);
    if state.toasts.len() != before {
        cx.refresh_windows();
    }
}

fn toast_element(toast: Toast, cx: &mut App) -> AnyElement {
    let theme = *cx.theme();
    let accent = match toast.kind {
        ToastKind::Success => theme.accent,
        ToastKind::Error => theme.danger,
    };
    let id = toast.id;

    div()
        .id(ElementId::Name(format!("toast-{id}").into()))
        .flex()
        .items_start()
        .gap_2()
        .rounded(rems(0.5))
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.bg_elevated)
        .shadow_lg()
        .p_3()
        .text_color(theme.text)
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
                        .text_color(theme.text_emphasis)
                        .child(toast.title),
                )
                .when_some(toast.message, |this, message| {
                    this.child(
                        div()
                            .text_xs()
                            .line_height(rems(1.2))
                            .text_color(theme.text_muted)
                            .child(message),
                    )
                }),
        )
        .child(
            div()
                .id(ElementId::Name(format!("toast-dismiss-{id}").into()))
                .size(rems(1.25))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(rems(0.25))
                .hover(move |this| this.bg(theme.bg_hover))
                .on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                    cx.stop_propagation();
                    dismiss(id, cx);
                })
                .child(
                    Icon::new(IconName::Close)
                        .size(12.0)
                        .color(theme.text_muted),
                ),
        )
        .into_any_element()
}
