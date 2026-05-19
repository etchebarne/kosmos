use gpui::{Context, Window};
use ui::delegate::{HeaderDelegate, HeaderMenu, HeaderMenuAction, WorkspaceDelegate};

use crate::app::KosmosApp;

impl HeaderDelegate for KosmosApp {
    fn toggle_header_menu(&mut self, menu: HeaderMenu, cx: &mut Context<Self>) {
        self.active_menu = if self.active_menu == Some(menu) {
            None
        } else {
            Some(menu)
        };
        cx.notify();
    }

    fn activate_header_menu_action(
        &mut self,
        action: HeaderMenuAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.active_menu = None;
        match action {
            HeaderMenuAction::OpenFolder => self.open_workspace_picker(cx),
            HeaderMenuAction::Save => self.save_active_editor(cx),
            HeaderMenuAction::SaveAll => self.save_all_files(cx),
            HeaderMenuAction::Undo
            | HeaderMenuAction::Redo
            | HeaderMenuAction::Cut
            | HeaderMenuAction::Copy
            | HeaderMenuAction::Paste
            | HeaderMenuAction::SelectAll => self.run_header_editor_action(action, window, cx),
            HeaderMenuAction::ExpandSelection | HeaderMenuAction::ShrinkSelection => {}
        }
        cx.notify();
    }
}
