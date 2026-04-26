use gpui::Context;
use pane_tree::DropZone;

use crate::drag::TabDrag;

pub trait WorkspaceDelegate: Sized + 'static {
    fn open_workspace_picker(&mut self, cx: &mut Context<Self>);
    fn select_workspace(&mut self, id: usize, cx: &mut Context<Self>);
}

pub trait HeaderDelegate: WorkspaceDelegate {
    fn toggle_header_menu(&mut self, menu: HeaderMenu, cx: &mut Context<Self>);
}

pub trait PaneDelegate: Sized + 'static {
    fn add_tab(&mut self, pane_id: usize, kind_id: &'static str, cx: &mut Context<Self>);
    fn select_tab(&mut self, pane_id: usize, tab_id: usize, cx: &mut Context<Self>);
    fn close_tab(&mut self, pane_id: usize, tab_id: usize, cx: &mut Context<Self>);
    fn move_tab_before(
        &mut self,
        drag: TabDrag,
        target_pane_id: usize,
        target_tab_id: usize,
        cx: &mut Context<Self>,
    );
    fn move_tab_to_pane(&mut self, drag: TabDrag, target_pane_id: usize, cx: &mut Context<Self>);
    fn split_pane(
        &mut self,
        drag: TabDrag,
        target_pane_id: usize,
        drop_zone: DropZone,
        cx: &mut Context<Self>,
    );
    fn resize_split(&mut self, split_id: usize, ratio: f32, cx: &mut Context<Self>);
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HeaderMenu {
    File,
    Edit,
    Selection,
}

impl HeaderMenu {
    pub fn id(self) -> usize {
        match self {
            Self::File => 0,
            Self::Edit => 1,
            Self::Selection => 2,
        }
    }
}
