use gpui::{Context, Window};
use ui::delegate::{HeaderDelegate, HeaderMenuAction, WorkspaceDelegate};

use crate::app::KosmosApp;

impl HeaderDelegate for KosmosApp {
    fn activate_header_menu_action(
        &mut self,
        action: HeaderMenuAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
