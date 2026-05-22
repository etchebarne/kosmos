mod blank;
mod file_editor;
mod file_search;
pub mod file_tree;
pub mod git;
pub mod infinity;
mod placeholder;
pub mod settings;
pub mod terminal;

use std::path::Path;

use gpui::{AnyElement, Context, IntoElement, Window, div};
use icons::IconName;

use tabs::{Tab, registry};

use crate::delegate::{PaneDelegate, SettingsDelegate};

pub fn render<T: PaneDelegate + SettingsDelegate>(
    workspace_id: usize,
    workspace_path: &Path,
    pane_id: usize,
    tab: &Tab,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    match tab.kind.as_str() {
        "blank" => blank::render(pane_id, tab.id, cx),
        "terminal" => terminal::render(
            workspace_id,
            terminal_cwd_for_tab(workspace_path, tab),
            tab.id,
            window,
            cx,
        ),
        "file_tree" => file_tree::render(window, cx),
        "file_search" => file_search::render(cx),
        "git" => git::render(window, cx),
        "infinity" => infinity::render(workspace_id, workspace_path, tab.id, window, cx),
        "settings" => settings::render(window, cx),
        "file_editor" => file_editor::render(tab, cx),
        _ => div().into_any_element(),
    }
}

pub fn icon_for_tab(tab: &Tab) -> IconName {
    if tab.kind.as_str() == registry::FILE_EDITOR.id
        && let Some(path) = &tab.path
        && let Some(icon) = icon_for_path(path)
    {
        return icon;
    }
    icon_for_kind(&tab.kind)
}

pub fn icon_for_kind(kind_id: &str) -> IconName {
    match kind_id {
        id if id == registry::BLANK.id => IconName::EmptyWindow,
        id if id == registry::FILE_TREE.id => IconName::ListTree,
        id if id == registry::FILE_SEARCH.id => IconName::Search,
        id if id == registry::GIT.id => IconName::SourceControl,
        id if id == registry::TERMINAL.id => IconName::Terminal,
        id if id == registry::INFINITY.id => IconName::Infinity,
        id if id == registry::SETTINGS.id => IconName::SettingsGear,
        id if id == registry::FILE_EDITOR.id => IconName::File,
        _ => IconName::File,
    }
}

fn terminal_cwd_for_tab<'a>(workspace_path: &'a Path, tab: &'a Tab) -> &'a Path {
    tab.path
        .as_deref()
        .filter(|path| path.is_absolute() && path.is_dir())
        .unwrap_or(workspace_path)
}

fn icon_for_path(path: &Path) -> Option<IconName> {
    if let Some(name) = path.file_name().and_then(|n| n.to_str())
        && let Some(icon) = IconName::for_file_name(name)
    {
        return Some(icon);
    }
    language::from_path(path).and_then(|id| IconName::for_language(id.as_str()))
}
