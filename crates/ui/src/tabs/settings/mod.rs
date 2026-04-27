mod cards;
mod content;
mod controls;
mod sidebar;
pub mod state;

pub use state::SettingsInputs;

use gpui::{AnyElement, Context, IntoElement, div, prelude::*};

use settings::{Settings, registry};

use crate::delegate::{ActiveSettingsUi, SettingsDelegate};

pub fn render<T: SettingsDelegate>(cx: &mut Context<T>) -> AnyElement {
    let (active_id, open_dropdown) = {
        let state = cx.settings_ui();
        (state.active_category, state.open_dropdown)
    };
    let active = registry::category(active_id)
        .or_else(|| Settings::categories().first().copied())
        .expect("settings registry has at least one category");

    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_row()
        .child(sidebar::render(active.id, cx))
        .child(content::render(active, open_dropdown, cx))
        .into_any_element()
}
