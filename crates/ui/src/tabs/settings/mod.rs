mod cards;
mod content;
mod controls;
pub mod state;

pub use state::SettingsInputs;

use gpui::{AnyElement, Context, IntoElement, Window, div, prelude::*};

use crate::delegate::{ActiveSettingsUi, SettingsDelegate};
use crate::tabs::settings::state::ActiveSettingsInputs;

pub fn render<T: SettingsDelegate>(window: &mut Window, cx: &mut Context<T>) -> AnyElement {
    let open_dropdown = cx.settings_ui().open_dropdown;
    let search = cx.settings_inputs().search();

    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(content::render(search, open_dropdown, window, cx))
        .into_any_element()
}
