use gpui::{AnyElement, Context, Global, Window};

use crate::delegate::{PaneDelegate, SettingsDelegate};

include!("parts/state_and_header.rs");
include!("parts/panels.rs");
include!("parts/branches_modal.rs");
include!("parts/branch_and_remote_modals.rs");
include!("parts/tags_and_stashes.rs");
include!("parts/modal_rows_and_buttons.rs");
include!("parts/change_tree.rs");
include!("parts/state_actions.rs");
include!("parts/async_actions.rs");

pub fn render<T: PaneDelegate + SettingsDelegate>(
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    render_git(window, cx)
}
