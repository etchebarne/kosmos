mod blank;
mod diff;
mod file_editor;
pub mod file_search;
pub mod file_tree;
pub mod git;
pub mod settings;
pub mod terminal;

use std::path::Path;

use gpui::{AnyElement, App, Context, IntoElement, Window, div};
use icons::IconName;

use tabs::{Tab, registry};

use crate::delegate::{PaneDelegate, SettingsDelegate};

pub fn render<T: PaneDelegate + SettingsDelegate + gpui::Render>(
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
        "file_search" => file_search::render(workspace_path, window, cx),
        "git" => git::render(window, cx),
        "diff" => diff::render(workspace_path, tab, window, cx),
        "settings" => settings::render(window, cx),
        "file_editor" => file_editor::render(tab, window, cx),
        _ => div().into_any_element(),
    }
}

pub fn install_keybindings(cx: &mut App) {
    file_editor::install_default_keybindings(cx);
    terminal::install_default_keybindings(cx);
}

pub fn drop_file_editor_tab(tab_id: usize, cx: &mut App) {
    file_editor::drop_tab(tab_id, cx);
}

pub fn request_file_editor_reveal(
    path: std::path::PathBuf,
    line: usize,
    column: usize,
    cx: &mut App,
) {
    file_editor::request_reveal(path, line, column, cx);
}

pub fn request_diff_focus(root: std::path::PathBuf, file_path: String, cx: &mut App) {
    diff::request_focus(root, file_path, cx);
}

pub fn refresh_diff_if_loaded<T: PaneDelegate + SettingsDelegate>(
    root: std::path::PathBuf,
    notify_now: bool,
    cx: &mut Context<T>,
) {
    diff::refresh_if_loaded(root, notify_now, cx);
}

pub fn refresh_diff_paths_if_loaded<T: PaneDelegate + SettingsDelegate>(
    root: std::path::PathBuf,
    paths: Vec<String>,
    notify_now: bool,
    cx: &mut Context<T>,
) {
    diff::refresh_paths_if_loaded(root, paths, notify_now, cx);
}

pub fn prewarm_diff<T: PaneDelegate + SettingsDelegate>(
    root: std::path::PathBuf,
    notify_now: bool,
    cx: &mut Context<T>,
) {
    diff::prewarm(root, notify_now, cx);
}

pub fn prewarm_diff_paths<T: PaneDelegate + SettingsDelegate>(
    root: std::path::PathBuf,
    paths: Vec<String>,
    notify_now: bool,
    cx: &mut Context<T>,
) {
    diff::prewarm_paths(root, paths, notify_now, cx);
}

pub fn refresh_diff_if_loaded_app(root: std::path::PathBuf, notify_now: bool, cx: &mut App) {
    diff::refresh_if_loaded_app(root, notify_now, cx);
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
        id if id == registry::DIFF.id => IconName::Diff,
        id if id == registry::TERMINAL.id => IconName::Terminal,
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
