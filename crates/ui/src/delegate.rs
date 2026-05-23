use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{App, Context, Global, ScrollHandle, Window};
use pane_tree::DropZone;
use settings::SettingValue;

use crate::drag::TabDrag;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabAnimationPhase {
    Opening,
    Closing,
}

impl TabAnimationPhase {
    pub fn progress(self, delta: f32) -> f32 {
        match self {
            Self::Opening => delta,
            Self::Closing => 1.0 - delta,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Opening => "opening",
            Self::Closing => "closing",
        }
    }
}

#[derive(Default)]
pub struct TabAnimationState {
    opening: HashSet<(usize, usize, usize)>,
    closing: HashSet<(usize, usize, usize)>,
}

impl TabAnimationState {
    pub fn phase(
        &self,
        workspace_id: usize,
        pane_id: usize,
        tab_id: usize,
    ) -> Option<TabAnimationPhase> {
        let key = (workspace_id, pane_id, tab_id);
        if self.closing.contains(&key) {
            return Some(TabAnimationPhase::Closing);
        }
        if self.opening.contains(&key) {
            return Some(TabAnimationPhase::Opening);
        }
        None
    }

    pub fn start_opening(&mut self, workspace_id: usize, pane_id: usize, tab_id: usize) -> bool {
        let key = (workspace_id, pane_id, tab_id);
        self.closing.remove(&key);
        self.opening.insert(key)
    }

    pub fn finish_opening(&mut self, workspace_id: usize, pane_id: usize, tab_id: usize) -> bool {
        self.opening.remove(&(workspace_id, pane_id, tab_id))
    }

    pub fn start_closing(&mut self, workspace_id: usize, pane_id: usize, tab_id: usize) -> bool {
        let key = (workspace_id, pane_id, tab_id);
        self.opening.remove(&key);
        self.closing.insert(key)
    }

    pub fn finish_closing(&mut self, workspace_id: usize, pane_id: usize, tab_id: usize) -> bool {
        self.closing.remove(&(workspace_id, pane_id, tab_id))
    }
}

impl Global for TabAnimationState {}

pub trait WorkspaceDelegate: Sized + 'static {
    fn open_workspace_picker(&mut self, cx: &mut Context<Self>);
    fn select_workspace(&mut self, id: usize, cx: &mut Context<Self>);
    fn move_workspace_before(&mut self, drag_id: usize, target_id: usize, cx: &mut Context<Self>);
    fn move_workspace_to_end(&mut self, drag_id: usize, cx: &mut Context<Self>);
    fn close_workspace(&mut self, id: usize, cx: &mut Context<Self>);
}

pub trait HeaderDelegate: WorkspaceDelegate {
    fn activate_header_menu_action(
        &mut self,
        action: HeaderMenuAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    );
}

pub trait PaneDelegate: Sized + 'static {
    fn focus_pane(&mut self, pane_id: usize, cx: &mut Context<Self>);
    fn add_tab(&mut self, pane_id: usize, kind_id: &'static str, cx: &mut Context<Self>);
    fn replace_tab_kind(
        &mut self,
        pane_id: usize,
        tab_id: usize,
        kind_id: &'static str,
        cx: &mut Context<Self>,
    );
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
    fn move_tab_to_end(&mut self, drag: TabDrag, target_pane_id: usize, cx: &mut Context<Self>);
    fn split_pane(
        &mut self,
        drag: TabDrag,
        target_pane_id: usize,
        drop_zone: DropZone,
        cx: &mut Context<Self>,
    );
    fn resize_split(&mut self, split_id: usize, ratio: f32, cx: &mut Context<Self>);
    fn finish_resize_split(&mut self, _cx: &mut Context<Self>) {}
    fn open_file(&mut self, path: PathBuf, cx: &mut Context<Self>);
    fn open_file_in_pane(&mut self, path: PathBuf, target_pane_id: usize, cx: &mut Context<Self>);
    fn open_file_before(
        &mut self,
        path: PathBuf,
        target_pane_id: usize,
        target_tab_id: usize,
        cx: &mut Context<Self>,
    );
    fn split_pane_with_file(
        &mut self,
        path: PathBuf,
        target_pane_id: usize,
        drop_zone: DropZone,
        cx: &mut Context<Self>,
    );
}

pub trait SettingsDelegate: Sized + 'static {
    fn select_settings_category(&mut self, category_id: &'static str, cx: &mut Context<Self>);
    fn toggle_settings_dropdown(&mut self, setting_id: &'static str, cx: &mut Context<Self>);
    fn set_setting_value(&mut self, key: &'static str, value: SettingValue, cx: &mut Context<Self>);
    fn install_tool(&mut self, entry: &'static registry::RegistryEntry, cx: &mut Context<Self>);
    fn uninstall_tool(&mut self, entry: &'static registry::RegistryEntry, cx: &mut Context<Self>);
}

#[derive(Clone, Debug)]
pub struct SettingsUiState {
    pub active_category: &'static str,
    pub open_dropdown: Option<&'static str>,
    pub installing: std::collections::HashSet<&'static str>,
    pub install_errors: std::collections::HashMap<&'static str, gpui::SharedString>,
}

impl SettingsUiState {
    pub fn new() -> Self {
        let active_category = settings::Settings::categories()
            .first()
            .map(|c| c.id)
            .unwrap_or("");
        Self {
            active_category,
            open_dropdown: None,
            installing: Default::default(),
            install_errors: Default::default(),
        }
    }
}

impl Default for SettingsUiState {
    fn default() -> Self {
        Self::new()
    }
}

impl Global for SettingsUiState {}

pub trait ActiveSettingsUi {
    fn settings_ui(&self) -> &SettingsUiState;
}

impl ActiveSettingsUi for App {
    fn settings_ui(&self) -> &SettingsUiState {
        self.global::<SettingsUiState>()
    }
}

#[derive(Clone, Default)]
pub struct TabScrollHandles(Rc<RefCell<HashMap<usize, ScrollHandle>>>);

impl TabScrollHandles {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle(&self, pane_id: usize) -> ScrollHandle {
        self.0.borrow_mut().entry(pane_id).or_default().clone()
    }

    pub fn scroll_to_index(&self, pane_id: usize, index: usize) {
        if let Some(handle) = self.0.borrow().get(&pane_id) {
            handle.scroll_to_item(index);
        }
    }
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HeaderMenuAction {
    OpenFolder,
    Save,
    SaveAll,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    ExpandSelection,
    ShrinkSelection,
}

impl HeaderMenuAction {
    pub fn shortcut_action_name(self) -> Option<&'static str> {
        match self {
            Self::Save => Some("file_editor::Save"),
            Self::Undo => Some("text_input::Undo"),
            Self::Redo => Some("text_input::Redo"),
            Self::Cut => Some("text_input::Cut"),
            Self::Copy => Some("text_input::Copy"),
            Self::Paste => Some("text_input::Paste"),
            Self::SelectAll => Some("text_input::SelectAll"),
            Self::OpenFolder | Self::SaveAll | Self::ExpandSelection | Self::ShrinkSelection => {
                None
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct HeaderMenuAvailability {
    pub active_editor: bool,
    pub active_editor_dirty: bool,
    pub any_dirty_file: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub can_cut: bool,
    pub can_copy: bool,
    pub can_paste: bool,
    pub can_select_all: bool,
    pub can_expand_selection: bool,
    pub can_shrink_selection: bool,
}

impl HeaderMenuAvailability {
    pub fn menu_enabled(self, menu: HeaderMenu) -> bool {
        match menu {
            HeaderMenu::File => true,
            HeaderMenu::Edit | HeaderMenu::Selection => self.active_editor,
        }
    }

    pub fn action_enabled(self, action: HeaderMenuAction) -> bool {
        match action {
            HeaderMenuAction::OpenFolder => true,
            HeaderMenuAction::Save => self.active_editor_dirty,
            HeaderMenuAction::SaveAll => self.any_dirty_file,
            HeaderMenuAction::Undo => self.can_undo,
            HeaderMenuAction::Redo => self.can_redo,
            HeaderMenuAction::Cut => self.can_cut,
            HeaderMenuAction::Copy => self.can_copy,
            HeaderMenuAction::Paste => self.can_paste,
            HeaderMenuAction::SelectAll => self.can_select_all,
            HeaderMenuAction::ExpandSelection => self.can_expand_selection,
            HeaderMenuAction::ShrinkSelection => self.can_shrink_selection,
        }
    }
}
