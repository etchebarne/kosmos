mod header;
mod pane_tree;

pub use header::*;
pub use pane_tree::*;

use std::path::PathBuf;

use gpui::{
    AnyElement, Context, IntoElement, MouseButton, SharedString, div, prelude::*, px,
};
use serde::{Deserialize, Serialize};

use icons::{Icon, IconName};
use theme::ActiveTheme;

pub trait WorkspaceDelegate: Sized + 'static {
    fn open_workspace_picker(&mut self, cx: &mut Context<Self>);
    fn select_workspace(&mut self, id: usize, cx: &mut Context<Self>);
}

#[derive(Serialize, Deserialize)]
pub struct Workspace {
    pub id: usize,
    pub path: PathBuf,
    pub name: SharedString,
    pub pane_tree: PaneTree,
}

impl Workspace {
    pub fn initial(&self) -> SharedString {
        self.name
            .chars()
            .next()
            .map(|c| c.to_ascii_uppercase().to_string())
            .unwrap_or_else(|| "?".to_string())
            .into()
    }
}

pub struct WorkspaceManager {
    workspaces: Vec<Workspace>,
    active: Option<usize>,
    next_id: usize,
}

impl WorkspaceManager {
    pub fn new() -> Self {
        Self {
            workspaces: Vec::new(),
            active: None,
            next_id: 0,
        }
    }

    pub fn from_parts(workspaces: Vec<Workspace>, active: Option<usize>, next_id: usize) -> Self {
        Self {
            workspaces,
            active,
            next_id,
        }
    }

    pub fn workspaces(&self) -> &[Workspace] {
        &self.workspaces
    }

    pub fn active_id(&self) -> Option<usize> {
        self.active
    }

    pub fn next_id(&self) -> usize {
        self.next_id
    }

    pub fn active_workspace(&self) -> Option<&Workspace> {
        let id = self.active?;
        self.workspaces.iter().find(|w| w.id == id)
    }

    pub fn add(&mut self, path: PathBuf) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        let name: SharedString = path
            .file_name()
            .and_then(|os| os.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| path.display().to_string())
            .into();
        self.workspaces.push(Workspace {
            id,
            path,
            name,
            pane_tree: PaneTree::new(),
        });
        self.active = Some(id);
        id
    }

    pub fn active_pane_tree(&self) -> Option<&PaneTree> {
        let id = self.active?;
        self.workspaces
            .iter()
            .find(|w| w.id == id)
            .map(|w| &w.pane_tree)
    }

    pub fn active_pane_tree_mut(&mut self) -> Option<&mut PaneTree> {
        let id = self.active?;
        self.workspaces
            .iter_mut()
            .find(|w| w.id == id)
            .map(|w| &mut w.pane_tree)
    }

    pub fn select(&mut self, id: usize) -> bool {
        if self.active == Some(id) {
            return false;
        }
        if !self.workspaces.iter().any(|w| w.id == id) {
            return false;
        }
        self.active = Some(id);
        true
    }
}

pub fn render_workspace_bar<T: WorkspaceDelegate>(
    manager: &WorkspaceManager,
    cx: &mut Context<T>,
) -> AnyElement {
    let mut elements: Vec<AnyElement> = Vec::new();
    for workspace in manager.workspaces() {
        let is_active = manager.active_id() == Some(workspace.id);
        elements.push(render_workspace_button(workspace, is_active, cx));
    }
    elements.push(render_add_button(cx));

    div()
        .flex()
        .items_center()
        .gap_1()
        .px_1()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .children(elements)
        .into_any_element()
}

fn render_add_button<T: WorkspaceDelegate>(cx: &mut Context<T>) -> AnyElement {
    let theme = *cx.theme();
    div()
        .id("workspace-add")
        .size(px(28.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.0))
        .text_color(theme.text_muted)
        .hover(move |this| this.bg(theme.bg_hover).text_color(theme.text_emphasis))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(|this, _, _, cx| {
            cx.stop_propagation();
            this.open_workspace_picker(cx);
        }))
        .child(Icon::new(IconName::Add).size(16.0).color(theme.text_muted))
        .into_any_element()
}

fn render_workspace_button<T: WorkspaceDelegate>(
    workspace: &Workspace,
    is_active: bool,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let id = workspace.id;
    div()
        .id(("workspace", id))
        .size(px(28.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.0))
        .text_sm()
        .bg(if is_active {
            theme.bg_selected
        } else {
            theme.bg_surface
        })
        .text_color(if is_active {
            theme.text_emphasis
        } else {
            theme.text_muted
        })
        .hover(move |this| this.bg(theme.bg_hover).text_color(theme.text_emphasis))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.select_workspace(id, cx);
        }))
        .child(workspace.initial())
        .into_any_element()
}

pub fn render_landing<T: WorkspaceDelegate>(cx: &mut Context<T>) -> AnyElement {
    let theme = *cx.theme();
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_3()
        .bg(theme.bg_surface)
        .rounded(px(8.0))
        .border_1()
        .border_color(theme.border)
        .text_color(theme.text)
        .child(div().text_2xl().child("Welcome to Kosmos!"))
        .child(
            div()
                .text_color(theme.text_subtle)
                .child("Open your first workspace to get started"),
        )
        .child(
            div()
                .id("landing-open-workspace")
                .mt_2()
                .flex()
                .items_center()
                .gap_2()
                .px(px(16.0))
                .py(px(8.0))
                .rounded(px(6.0))
                .bg(theme.bg_selected)
                .text_color(theme.text)
                .text_sm()
                .hover(move |this| {
                    this.bg(theme.bg_hover_strong).text_color(theme.text_emphasis)
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.open_workspace_picker(cx);
                }))
                .child(Icon::new(IconName::Add).size(16.0).color(theme.text))
                .child("Open Workspace"),
        )
        .into_any_element()
}
