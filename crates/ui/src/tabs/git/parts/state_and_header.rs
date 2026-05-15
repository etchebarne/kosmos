use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, Global, IntoElement, MouseButton, MouseDownEvent,
    Pixels, Point, SharedString, Task, Window, anchored, deferred, div, prelude::*, rems, rgb,
};

use file_tree::ActiveFileTree;
use icons::{Icon, IconName};
use kosmos_git::{
    Branch, CommitInfo, FileChange, FileChangeKind, Remote, RepositorySummary, Stash, Tag,
};
use tabs::registry;
use theme::ActiveTheme;

use crate::components::{TextArea, TextInput, Tooltip, TooltipPosition, ValueChanged, modal};
use crate::delegate::{PaneDelegate, SettingsDelegate};

#[derive(Clone, Copy, PartialEq, Eq)]
enum GitModal {
    Branches,
    CreateBranch,
    Remotes,
    Stashes,
    Tags,
    ConfirmDiscardSelected,
    ConfirmDiscard,
}

#[derive(Default)]
struct GitUiState {
    root: Option<PathBuf>,
    summary: Option<RepositorySummary>,
    loading: bool,
    refresh_generation: u64,
    refresh_task: Option<Task<()>>,
    watch_generation: u64,
    watch_task: Option<Task<()>>,
    menu_position: Option<Point<Pixels>>,
    modal: Option<GitModal>,
    last_error: Option<String>,
    remotes: Vec<Remote>,
    stashes: Vec<Stash>,
    expanded_stashes: std::collections::HashSet<String>,
    collapsed_change_dirs: std::collections::HashSet<String>,
    tags: Vec<Tag>,
    branches: Vec<Branch>,
    commit_message: Option<Entity<TextArea>>,
    branch_search: Option<Entity<TextInput>>,
    branch_name: Option<Entity<TextInput>>,
    remote_name: Option<Entity<TextInput>>,
    remote_url: Option<Entity<TextInput>>,
    tag_name: Option<Entity<TextInput>>,
    tag_message: Option<Entity<TextInput>>,
    tag_sha: Option<Entity<TextInput>>,
}

impl Global for GitUiState {}

const CHANGE_ROW_HEIGHT_REM: f32 = 1.625;
const CHANGE_ROW_PADDING_REM: f32 = 1.00;
const CHANGE_INDENT_REM: f32 = 1.25;
const CHANGE_GUIDE_OFFSET_REM: f32 = 0.625;
const CHANGE_GUIDE_WIDTH_REM: f32 = 0.0625;
const CHANGE_ICON_WIDTH_REM: f32 = 1.25;
const CHANGE_LABEL_PADDING_REM: f32 = 0.25;
const COMMIT_PANEL_HEIGHT_REM: f32 = 11.0;
const COMMIT_MESSAGE_HEIGHT_REM: f32 = 8.25;
const COMMIT_MESSAGE_PADDING_X_REM: f32 = 1.25;
const COMMIT_MESSAGE_PADDING_TOP_REM: f32 = 1.25;
const COMMIT_MESSAGE_PADDING_BOTTOM_REM: f32 = 0.5;
const COMMIT_CONTROLS_INSET_X_REM: f32 = 1.0;
const COMMIT_CONTROLS_INSET_BOTTOM_REM: f32 = 1.0;

pub fn render<T: PaneDelegate + SettingsDelegate>(cx: &mut Context<T>) -> AnyElement {
    ensure_state(cx);
    let theme = *cx.theme();
    let Some(root) = cx
        .file_tree()
        .and_then(|tree| tree.read(cx).root().map(|path| path.to_path_buf()))
    else {
        return empty_state("No workspace open", cx);
    };

    ensure_summary(&root, cx);

    let (summary, loading, menu_position, last_error) = {
        let state = cx.global::<GitUiState>();
        (
            state.summary.clone(),
            state.loading,
            state.menu_position,
            state.last_error.clone(),
        )
    };
    let dismiss_layer = menu_position.map(|_| menu_dismiss_layer::<T>(cx));
    let menu_overlay = menu_position.map(|position| more_menu::<T>(&root, position, cx));

    div()
        .relative()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(theme.bg_surface)
        .text_color(theme.text)
        .child(header(&root, summary.as_ref(), loading, cx))
        .when_some(last_error, |this, error| {
            this.child(error_banner(error, cx))
        })
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .when(summary.is_none(), |this| {
                    this.child(if loading {
                        loading_state(cx)
                    } else {
                        empty_panel("Git status unavailable", cx)
                    })
                })
                .when_some(summary.as_ref(), |this, summary| {
                    this.child(change_list(&root, summary, cx))
                }),
        )
        .child(commit_panel(&root, summary.as_ref(), cx))
        .when_some(
            summary
                .as_ref()
                .and_then(|summary| summary.latest_commit.clone()),
            |this, commit| this.child(latest_commit_panel(commit, cx)),
        )
        .when_some(dismiss_layer, |this, layer| this.child(layer))
        .when_some(menu_overlay, |this, menu| this.child(menu))
        .into_any_element()
}

pub fn render_modal_overlay<T: PaneDelegate + SettingsDelegate>(cx: &mut Context<T>) -> AnyElement {
    let Some(state) = cx.try_global::<GitUiState>() else {
        return div().into_any_element();
    };
    let Some(modal_state) = state.modal else {
        return div().into_any_element();
    };
    let Some(root) = state.root.clone() else {
        return div().into_any_element();
    };
    render_git_modal(&root, modal_state, cx)
}

fn header<T: PaneDelegate + SettingsDelegate>(
    root: &PathBuf,
    summary: Option<&RepositorySummary>,
    loading: bool,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let root_refresh = root.clone();
    let root_stage = root.clone();
    let root_unstage = root.clone();
    let root_stash = root.clone();

    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .border_b_1()
        .border_color(theme.border_subtle)
        .px_3()
        .py_2()
        .child(
            div()
                .min_w_0()
                .flex()
                .items_center()
                .gap_1p5()
                .child(icon_button::<T>(
                    "git-refresh",
                    IconName::Refresh,
                    None,
                    move |_, _, cx| {
                        clear_error(cx);
                        refresh_summary(root_refresh.clone(), true, true, cx);
                    },
                    cx,
                ))
                .child(change_count(
                    summary.map(|summary| summary.changes),
                    loading,
                    cx,
                ))
                .when_some(summary, |this, summary| this.child(diff_stats(summary, cx))),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_1()
                .child(icon_button::<T>(
                    "git-stage-all",
                    IconName::Add,
                    Some("Stage All Changes"),
                    move |_, _, cx| {
                        run_git_action(root_stage.clone(), kosmos_git::stage_all, cx);
                    },
                    cx,
                ))
                .child(icon_button::<T>(
                    "git-unstage-all",
                    IconName::Remove,
                    Some("Unstage All Changes"),
                    move |_, _, cx| {
                        run_git_action(root_unstage.clone(), kosmos_git::unstage_all, cx);
                    },
                    cx,
                ))
                .child(icon_button::<T>(
                    "git-stash-staged",
                    IconName::Archive,
                    Some("Stash Staged Changes"),
                    move |_, _, cx| {
                        run_git_action(root_stash.clone(), kosmos_git::stash_staged, cx);
                    },
                    cx,
                ))
                .child(more_button(cx)),
        )
        .into_any_element()
}

fn change_count<T: PaneDelegate + SettingsDelegate>(
    changes: Option<usize>,
    loading: bool,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let label = match changes {
        Some(0) => "No Changes".to_string(),
        Some(changes) => format!("{changes} Change{}", plural(changes)),
        None if loading => "Loading Changes".to_string(),
        None => "No Changes".to_string(),
    };
    div()
        .text_xs()
        .text_color(theme.text_emphasis)
        .child(label)
        .into_any_element()
}
