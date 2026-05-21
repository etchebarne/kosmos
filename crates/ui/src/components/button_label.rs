use gpui::{AnyElement, IntoElement, SharedString, div, prelude::*};

pub fn left_aligned_button_label(label: impl Into<SharedString>) -> AnyElement {
    let label = label.into();

    div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(label)
        .into_any_element()
}
