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
const COMMIT_MESSAGE_HEIGHT_REM: f32 = 11.0;
const COMMIT_MESSAGE_PADDING_X_REM: f32 = 1.25;
const COMMIT_MESSAGE_PADDING_TOP_REM: f32 = 1.25;
const COMMIT_MESSAGE_PADDING_BOTTOM_REM: f32 = 3.25;
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

fn loading_state<T: PaneDelegate + SettingsDelegate>(cx: &mut Context<T>) -> AnyElement {
    empty_panel("Loading Git status", cx)
}

fn commit_panel<T: PaneDelegate + SettingsDelegate>(
    root: &PathBuf,
    summary: Option<&RepositorySummary>,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let commit_message = cx
        .global::<GitUiState>()
        .commit_message
        .as_ref()
        .unwrap()
        .clone();
    let branch = summary
        .and_then(|summary| summary.branch.as_deref())
        .unwrap_or("Detached HEAD")
        .to_string();
    let has_staged = summary.is_some_and(|summary| summary.files.iter().any(|file| file.staged));
    let root_branch = root.clone();
    let root_commit = root.clone();
    let message_input = commit_message.clone();

    div()
        .flex_none()
        .border_t_1()
        .border_color(theme.border_subtle)
        .bg(theme.bg_surface)
        .child(
            div()
                .relative()
                .min_w_0()
                .w_full()
                .child(commit_message)
                .child(
                    div()
                        .absolute()
                        .left(rems(COMMIT_CONTROLS_INSET_X_REM))
                        .right(rems(COMMIT_CONTROLS_INSET_X_REM))
                        .bottom(rems(COMMIT_CONTROLS_INSET_BOTTOM_REM))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(
                            div()
                                .id("git-current-branch")
                                .min_w_0()
                                .max_w_full()
                                .rounded(rems(0.3125))
                                .border_1()
                                .border_color(theme.border)
                                .bg(theme.bg_elevated)
                                .px_2()
                                .py_1()
                                .text_sm()
                                .text_color(theme.text)
                                .hover(move |this| this.bg(theme.bg_hover))
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    open_modal(root_branch.clone(), GitModal::Branches, cx);
                                }))
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex()
                                        .items_center()
                                        .gap_1p5()
                                        .child(
                                            Icon::new(IconName::SourceControl)
                                                .size(14.0)
                                                .color(theme.text_muted),
                                        )
                                        .child(
                                            div()
                                                .min_w_0()
                                                .overflow_hidden()
                                                .whitespace_nowrap()
                                                .text_ellipsis()
                                                .child(branch),
                                        ),
                                ),
                        )
                        .child(commit_button(
                            has_staged,
                            cx.listener(move |_, _, _, cx| {
                                let message = message_input.read(cx).value().to_string();
                                commit_tracked(
                                    root_commit.clone(),
                                    message,
                                    message_input.clone(),
                                    cx,
                                );
                            }),
                            cx,
                        )),
                ),
        )
        .into_any_element()
}

fn latest_commit_panel<T: PaneDelegate + SettingsDelegate>(
    commit: CommitInfo,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let subject = commit
        .subject
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    if subject.is_empty() {
        return div().into_any_element();
    }

    div()
        .flex_none()
        .border_t_1()
        .border_color(theme.border_subtle)
        .bg(theme.bg_surface)
        .px_3()
        .py_2()
        .flex()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_xs()
                .text_color(theme.text_subtle)
                .child(SharedString::from(subject)),
        )
        .into_any_element()
}

fn empty_panel<T: PaneDelegate + SettingsDelegate>(
    message: &'static str,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    div()
        .flex()
        .items_center()
        .gap_2()
        .rounded(rems(0.5))
        .border_1()
        .border_color(theme.border_subtle)
        .bg(theme.bg_elevated)
        .p_3()
        .text_sm()
        .text_color(theme.text_subtle)
        .child(
            Icon::new(IconName::SourceControl)
                .size(14.0)
                .color(theme.text_muted),
        )
        .child(message)
        .into_any_element()
}

fn diff_stats<T: PaneDelegate + SettingsDelegate>(
    summary: &RepositorySummary,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let added = rgb(0x22c55e);
    div()
        .flex()
        .items_center()
        .gap_1()
        .text_xs()
        .when(summary.insertions > 0, |this| {
            this.child(
                div()
                    .rounded(rems(0.25))
                    .bg(gpui::Hsla::from(added).opacity(0.12))
                    .px_1p5()
                    .py_0p5()
                    .text_color(added)
                    .child(format!("+{}", summary.insertions)),
            )
        })
        .when(summary.deletions > 0, |this| {
            this.child(
                div()
                    .rounded(rems(0.25))
                    .bg(gpui::Hsla::from(theme.danger).opacity(0.12))
                    .px_1p5()
                    .py_0p5()
                    .text_color(theme.danger)
                    .child(format!("-{}", summary.deletions)),
            )
        })
        .into_any_element()
}

fn icon_button<T: PaneDelegate + SettingsDelegate>(
    id: &'static str,
    icon: IconName,
    tooltip: Option<&'static str>,
    listener: impl Fn(&ClickEvent, &mut Window, &mut Context<T>) + 'static,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let _ = cx;
    let button = div()
        .id(id)
        .size(rems(1.375))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(rems(0.25))
        .text_color(theme.text_muted)
        .hover(move |this| this.bg(theme.bg_hover).text_color(theme.text_emphasis))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |_, event: &ClickEvent, window, cx| {
            cx.stop_propagation();
            listener(event, window, cx);
        }))
        .child(Icon::new(icon).size(14.0).color(theme.text_muted));

    match tooltip {
        Some(tooltip) => Tooltip::new(format!("{id}-tooltip"), tooltip, button)
            .position(TooltipPosition::Bottom)
            .into_any_element(),
        None => button.into_any_element(),
    }
}

fn more_button<T: PaneDelegate + SettingsDelegate>(cx: &mut Context<T>) -> AnyElement {
    let theme = *cx.theme();
    div()
        .id("git-more")
        .size(rems(1.375))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(rems(0.25))
        .text_color(theme.text_muted)
        .hover(move |this| this.bg(theme.bg_hover).text_color(theme.text_emphasis))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|_, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                let position = event.position;
                cx.update_global::<GitUiState, _>(|state, _| {
                    state.menu_position = match state.menu_position {
                        Some(_) => None,
                        None => Some(position),
                    };
                });
                cx.notify();
            }),
        )
        .child(
            Icon::new(IconName::Ellipsis)
                .size(14.0)
                .color(theme.text_muted),
        )
        .into_any_element()
}

fn more_menu<T: PaneDelegate + SettingsDelegate>(
    root: &PathBuf,
    position: Point<Pixels>,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let root_push = root.clone();
    let root_pull = root.clone();
    let root_fetch = root.clone();
    let root_branches = root.clone();
    let root_remotes = root.clone();
    let root_stashes = root.clone();
    let root_tags = root.clone();
    let root_discard_selected = root.clone();
    let root_discard = root.clone();
    let has_selected_changes = cx
        .global::<GitUiState>()
        .summary
        .as_ref()
        .is_some_and(|summary| summary.files.iter().any(|file| file.staged));
    deferred(
        anchored().position(position).snap_to_window().child(
            div()
                .id("git-more-menu")
                .min_w(rems(11.0))
                .p_1()
                .flex()
                .flex_col()
                .gap_0p5()
                .rounded(rems(0.375))
                .border_1()
                .border_color(theme.border_strong)
                .bg(theme.bg_elevated)
                .shadow_lg()
                .text_sm()
                .text_color(theme.text)
                .block_mouse_except_scroll()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(menu_item::<T>(
                    "git-menu-push",
                    IconName::ArrowUp,
                    "Push",
                    true,
                    false,
                    move |_, _, cx| run_git_action(root_push.clone(), kosmos_git::push, cx),
                    cx,
                ))
                .child(menu_item::<T>(
                    "git-menu-pull",
                    IconName::ArrowDown,
                    "Pull",
                    true,
                    false,
                    move |_, _, cx| run_git_action(root_pull.clone(), kosmos_git::pull, cx),
                    cx,
                ))
                .child(menu_item::<T>(
                    "git-menu-fetch",
                    IconName::Refresh,
                    "Fetch",
                    true,
                    false,
                    move |_, _, cx| run_git_action(root_fetch.clone(), kosmos_git::fetch, cx),
                    cx,
                ))
                .child(menu_item::<T>(
                    "git-menu-branches",
                    IconName::SourceControl,
                    "Branches",
                    true,
                    false,
                    move |_, _, cx| open_modal(root_branches.clone(), GitModal::Branches, cx),
                    cx,
                ))
                .child(menu_item::<T>(
                    "git-menu-remotes",
                    IconName::Server,
                    "Remotes",
                    true,
                    false,
                    move |_, _, cx| open_modal(root_remotes.clone(), GitModal::Remotes, cx),
                    cx,
                ))
                .child(menu_item::<T>(
                    "git-menu-stashes",
                    IconName::Archive,
                    "Stashes",
                    true,
                    false,
                    move |_, _, cx| open_modal(root_stashes.clone(), GitModal::Stashes, cx),
                    cx,
                ))
                .child(menu_item::<T>(
                    "git-menu-tags",
                    IconName::Tag,
                    "Tags",
                    true,
                    false,
                    move |_, _, cx| open_modal(root_tags.clone(), GitModal::Tags, cx),
                    cx,
                ))
                .child(menu_separator(theme))
                .child(menu_item::<T>(
                    "git-menu-discard-selected",
                    IconName::Trash,
                    "Discard Selected Changes",
                    has_selected_changes,
                    true,
                    move |_, _, cx| {
                        open_modal(
                            root_discard_selected.clone(),
                            GitModal::ConfirmDiscardSelected,
                            cx,
                        )
                    },
                    cx,
                ))
                .child(menu_item::<T>(
                    "git-menu-discard-all",
                    IconName::Trash,
                    "Discard All Changes",
                    true,
                    true,
                    move |_, _, cx| open_modal(root_discard.clone(), GitModal::ConfirmDiscard, cx),
                    cx,
                )),
        ),
    )
    .with_priority(2)
    .into_any_element()
}

fn menu_item<T: PaneDelegate + SettingsDelegate>(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    enabled: bool,
    danger: bool,
    listener: impl Fn(&ClickEvent, &mut Window, &mut Context<T>) + 'static,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let text_color = if enabled {
        if danger { theme.danger } else { theme.text }
    } else {
        theme.text_subtle
    };
    let icon_color = if enabled {
        if danger {
            theme.danger
        } else {
            theme.text_muted
        }
    } else {
        theme.text_subtle
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .h(rems(1.625))
        .px_2()
        .rounded(rems(0.25))
        .text_color(text_color)
        .when(enabled, |this| {
            this.hover(move |this| this.bg(theme.bg_selected).text_color(theme.text_emphasis))
                .on_click(cx.listener(move |_, event: &ClickEvent, window, cx| {
                    cx.stop_propagation();
                    listener(event, window, cx);
                }))
        })
        .child(
            div()
                .w(rems(1.0))
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(icon).size(14.0).color(icon_color)),
        )
        .child(label)
        .into_any_element()
}

fn menu_separator(theme: theme::Theme) -> AnyElement {
    div()
        .h(rems(0.0625))
        .my(rems(0.25))
        .bg(theme.border_subtle)
        .into_any_element()
}

fn menu_dismiss_layer<T: PaneDelegate + SettingsDelegate>(cx: &mut Context<T>) -> AnyElement {
    div()
        .id("git-menu-dismiss")
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .bottom_0()
        .on_mouse_down(MouseButton::Left, cx.listener(|_, _, _, cx| close_menu(cx)))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|_, _, _, cx| close_menu(cx)),
        )
        .into_any_element()
}

fn render_git_modal<T: PaneDelegate + SettingsDelegate>(
    root: &PathBuf,
    modal_state: GitModal,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    match modal_state {
        GitModal::Branches => modal::render(
            "git-branches-modal",
            "Git Branches",
            branches_modal_body(root, cx),
            modal_footer(close_modal_button(cx), cx),
            theme,
            cx.listener(|_, _, _, cx| close_modal(cx)),
        ),
        GitModal::CreateBranch => modal::render(
            "git-create-branch-modal",
            "Create Branch",
            create_branch_modal_body(cx),
            create_branch_modal_footer(root, cx),
            theme,
            cx.listener(|_, _, _, cx| close_modal(cx)),
        ),
        GitModal::Remotes => modal::render(
            "git-remotes-modal",
            "Git Remotes",
            remotes_modal_body(root, cx),
            modal_footer(close_modal_button(cx), cx),
            theme,
            cx.listener(|_, _, _, cx| close_modal(cx)),
        ),
        GitModal::Stashes => modal::render(
            "git-stashes-modal",
            "Git Stashes",
            stashes_modal_body(root, cx),
            modal_footer(close_modal_button(cx), cx),
            theme,
            cx.listener(|_, _, _, cx| close_modal(cx)),
        ),
        GitModal::Tags => modal::render(
            "git-tags-modal",
            "Git Tags",
            tags_modal_body(root, cx),
            modal_footer(close_modal_button(cx), cx),
            theme,
            cx.listener(|_, _, _, cx| close_modal(cx)),
        ),
        GitModal::ConfirmDiscardSelected => {
            let root = root.clone();
            let selected_paths = selected_change_paths(cx);
            let selected_count = selected_paths.len();
            modal::render(
                "git-discard-selected-modal",
                "Discard Selected Changes",
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .text_sm()
                    .child(format!(
                        "This will permanently discard {selected_count} selected working tree change{}. This action cannot be undone.",
                        plural(selected_count)
                    ))
                    .into_any_element(),
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(close_modal_button(cx))
                    .child(action_button(
                        "git-confirm-discard-selected",
                        "Discard Selected",
                        true,
                        cx.listener(move |_, _, _, cx| {
                            close_modal(cx);
                            run_git_action(
                                root.clone(),
                                {
                                    let selected_paths = selected_paths.clone();
                                    move |root| kosmos_git::discard_files(root, &selected_paths)
                                },
                                cx,
                            );
                        }),
                        cx,
                    ))
                    .into_any_element(),
                theme,
                cx.listener(|_, _, _, cx| close_modal(cx)),
            )
        }
        GitModal::ConfirmDiscard => {
            let root = root.clone();
            modal::render(
                "git-discard-modal",
                "Discard All Changes",
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .text_sm()
                    .child("This will permanently discard all tracked and untracked working tree changes. This action cannot be undone.")
                    .into_any_element(),
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(close_modal_button(cx))
                    .child(action_button(
                        "git-confirm-discard",
                        "Discard All",
                        true,
                        cx.listener(move |_, _, _, cx| {
                            close_modal(cx);
                            run_git_action(root.clone(), kosmos_git::discard_all_changes, cx);
                        }),
                        cx,
                    ))
                    .into_any_element(),
                theme,
                cx.listener(|_, _, _, cx| close_modal(cx)),
            )
        }
    }
}

fn branches_modal_body<T: PaneDelegate + SettingsDelegate>(
    root: &PathBuf,
    cx: &mut Context<T>,
) -> AnyElement {
    let (branch_search, branches, last_error) = {
        let state = cx.global::<GitUiState>();
        (
            state.branch_search.as_ref().unwrap().clone(),
            state.branches.clone(),
            state.last_error.clone(),
        )
    };
    let theme = *cx.theme();
    let query = branch_search.read(cx).value().trim().to_lowercase();
    let has_branches = !branches.is_empty();
    let branches = if query.is_empty() {
        branches
    } else {
        branches
            .into_iter()
            .filter(|branch| {
                branch.name.to_lowercase().contains(&query)
                    || (if branch.remote { "remote" } else { "local" }).contains(&query)
            })
            .collect()
    };

    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(input_row("Search Branches", branch_search))
        .when_some(last_error, |this, error| {
            this.child(
                div()
                    .rounded(rems(0.375))
                    .border_1()
                    .border_color(gpui::Hsla::from(theme.danger).opacity(0.35))
                    .bg(gpui::Hsla::from(theme.danger).opacity(0.12))
                    .p_2()
                    .text_xs()
                    .text_color(theme.text)
                    .child(error),
            )
        })
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(create_branch_row(root.clone(), cx))
                .when(branches.is_empty(), |this| {
                    this.child(
                        div()
                            .rounded(rems(0.375))
                            .border_1()
                            .border_color(theme.border_subtle)
                            .p_3()
                            .text_sm()
                            .text_color(theme.text_subtle)
                            .child(if has_branches {
                                "No branches match your search"
                            } else {
                                "No branches"
                            }),
                    )
                })
                .when(!branches.is_empty(), |this| {
                    this.children(
                        branches
                            .into_iter()
                            .map(|branch| branch_row(root.clone(), branch, cx)),
                    )
                }),
        )
        .into_any_element()
}

fn create_branch_row<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let branch_name = cx
        .global::<GitUiState>()
        .branch_name
        .as_ref()
        .unwrap()
        .clone();
    div()
        .id("git-create-branch-row")
        .flex()
        .items_start()
        .gap_2()
        .rounded(rems(0.375))
        .border_1()
        .border_color(theme.border_subtle)
        .p_2p5()
        .text_sm()
        .text_color(theme.text)
        .hover(move |this| this.bg(theme.bg_hover))
        .on_click(cx.listener(move |_, _, _, cx| {
            branch_name.update(cx, |input, cx| input.set_value("", cx));
            open_modal(root.clone(), GitModal::CreateBranch, cx);
        }))
        .child(
            div()
                .size(rems(1.25))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(rems(0.25))
                .bg(gpui::Hsla::from(theme.accent).opacity(0.14))
                .child(Icon::new(IconName::Add).size(14.0).color(theme.accent)),
        )
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_color(theme.text_emphasis).child("Create Branch"))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.text_subtle)
                        .child("Create a new local branch"),
                ),
        )
        .into_any_element()
}

fn branch_row<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    branch: Branch,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let name = branch.name.clone();
    let is_current = branch.current;
    let is_remote = branch.remote;
    let row_id = SharedString::from(format!("git-branch:{name}"));
    let delete_id = SharedString::from(format!("git-delete-branch:{name}"));
    let switch_branch = name.clone();
    let delete_branch = name.clone();
    let root_switch = root.clone();
    let root_delete = root.clone();

    div()
        .id(row_id)
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .rounded(rems(0.375))
        .border_1()
        .border_color(if is_current {
            gpui::Hsla::from(theme.accent)
        } else {
            gpui::Hsla::from(theme.border_subtle)
        })
        .p_2p5()
        .text_sm()
        .text_color(if is_current {
            theme.text_emphasis
        } else {
            theme.text
        })
        .when(!is_current, |this| {
            this.hover(move |this| this.bg(theme.bg_hover))
                .on_click(cx.listener(move |_, _, _, cx| {
                    let branch = switch_branch.clone();
                    run_modal_action(
                        root_switch.clone(),
                        GitModal::Branches,
                        move |root| {
                            if is_remote {
                                kosmos_git::switch_remote_branch(root, &branch)
                            } else {
                                kosmos_git::switch_branch(root, &branch)
                            }
                        },
                        cx,
                    );
                }))
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_start()
                .gap_2()
                .child(
                    div().mt(rems(0.1875)).child(
                        Icon::new(IconName::SourceControl)
                            .size(14.0)
                            .color(theme.text_muted),
                    ),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .child(
                            div()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(name),
                        )
                        .when(is_remote, |this| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(theme.text_subtle)
                                    .child("Remote"),
                            )
                        }),
                ),
        )
        .when(!is_current && !is_remote, |this| {
            this.child(div().flex_none().child(delete_button(
                delete_id,
                cx.listener(move |_, _, _, cx| {
                    let branch = delete_branch.clone();
                    run_modal_action(
                        root_delete.clone(),
                        GitModal::Branches,
                        move |root| kosmos_git::delete_branch(root, &branch),
                        cx,
                    );
                }),
                cx,
            )))
        })
        .into_any_element()
}

fn create_branch_modal_body<T: PaneDelegate + SettingsDelegate>(cx: &mut Context<T>) -> AnyElement {
    let (branch_name, last_error) = {
        let state = cx.global::<GitUiState>();
        (
            state.branch_name.as_ref().unwrap().clone(),
            state.last_error.clone(),
        )
    };
    let theme = *cx.theme();

    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(input_row("Branch Name", branch_name))
        .when_some(last_error, |this, error| {
            this.child(
                div()
                    .rounded(rems(0.375))
                    .border_1()
                    .border_color(gpui::Hsla::from(theme.danger).opacity(0.35))
                    .bg(gpui::Hsla::from(theme.danger).opacity(0.12))
                    .p_2()
                    .text_xs()
                    .text_color(theme.text)
                    .child(error),
            )
        })
        .into_any_element()
}

fn create_branch_modal_footer<T: PaneDelegate + SettingsDelegate>(
    root: &PathBuf,
    cx: &mut Context<T>,
) -> AnyElement {
    let branch_name = cx
        .global::<GitUiState>()
        .branch_name
        .as_ref()
        .unwrap()
        .clone();
    let root_cancel = root.clone();
    let root_create = root.clone();
    let cancel_input = branch_name.clone();
    let create_input = branch_name.clone();

    div()
        .flex()
        .justify_end()
        .gap_2()
        .child(action_button(
            "git-cancel-create-branch",
            "Cancel",
            false,
            cx.listener(move |_, _, _, cx| {
                cancel_input.update(cx, |input, cx| input.set_value("", cx));
                open_modal(root_cancel.clone(), GitModal::Branches, cx);
            }),
            cx,
        ))
        .child(action_button(
            "git-confirm-create-branch",
            "Create",
            false,
            cx.listener(move |_, _, _, cx| {
                let branch = create_input.read(cx).value().trim().to_string();
                if branch.is_empty() {
                    return;
                }
                let input = create_input.clone();
                run_modal_action_after_success(
                    root_create.clone(),
                    GitModal::Branches,
                    move |root| kosmos_git::create_branch(root, &branch),
                    move |cx| {
                        input.update(cx, |input, cx| input.set_value("", cx));
                        cx.update_global::<GitUiState, _>(|state, _| {
                            state.modal = Some(GitModal::Branches)
                        });
                    },
                    cx,
                );
            }),
            cx,
        ))
        .into_any_element()
}

fn remotes_modal_body<T: PaneDelegate + SettingsDelegate>(
    root: &PathBuf,
    cx: &mut Context<T>,
) -> AnyElement {
    let (name, url, remotes) = {
        let state = cx.global::<GitUiState>();
        (
            state.remote_name.as_ref().unwrap().clone(),
            state.remote_url.as_ref().unwrap().clone(),
            state.remotes.clone(),
        )
    };
    let root_add = root.clone();
    let theme = *cx.theme();

    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(input_row("Remote Name", name.clone()))
        .child(input_row("Remote URL", url.clone()))
        .child(div().flex().justify_end().child(action_button(
            "git-add-remote",
            "Add Remote",
            false,
            cx.listener(move |_, _, _, cx| {
                let name_value = name.read(cx).value().to_string();
                let url_value = url.read(cx).value().to_string();
                if name_value.trim().is_empty() || url_value.trim().is_empty() {
                    return;
                }
                run_modal_action(
                    root_add.clone(),
                    GitModal::Remotes,
                    move |root| kosmos_git::add_remote(root, name_value.trim(), url_value.trim()),
                    cx,
                );
            }),
            cx,
        )))
        .child(section_label("Existing Remotes", theme))
        .children(
            remotes
                .into_iter()
                .map(|remote| remote_row(root.clone(), remote, cx)),
        )
        .into_any_element()
}

fn tags_modal_body<T: PaneDelegate + SettingsDelegate>(
    root: &PathBuf,
    cx: &mut Context<T>,
) -> AnyElement {
    let (name, message, sha, tags) = {
        let state = cx.global::<GitUiState>();
        (
            state.tag_name.as_ref().unwrap().clone(),
            state.tag_message.as_ref().unwrap().clone(),
            state.tag_sha.as_ref().unwrap().clone(),
            state.tags.clone(),
        )
    };
    let root_add = root.clone();
    let theme = *cx.theme();

    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(input_row("Tag Name", name.clone()))
        .child(input_row("Tag Message (optional)", message.clone()))
        .child(input_row("Commit SHA (optional)", sha.clone()))
        .child(div().flex().justify_end().child(action_button(
            "git-add-tag",
            "Add Tag",
            false,
            cx.listener(move |_, _, _, cx| {
                let name_value = name.read(cx).value().to_string();
                if name_value.trim().is_empty() {
                    return;
                }
                let message_value = message.read(cx).value().to_string();
                let sha_value = sha.read(cx).value().to_string();
                run_modal_action(
                    root_add.clone(),
                    GitModal::Tags,
                    move |root| {
                        kosmos_git::add_tag(
                            root,
                            name_value.trim(),
                            Some(message_value.trim()),
                            Some(sha_value.trim()),
                        )
                    },
                    cx,
                );
            }),
            cx,
        )))
        .child(section_label("Existing Tags", theme))
        .children(tags.into_iter().map(|tag| tag_row(root.clone(), tag, cx)))
        .into_any_element()
}

fn stashes_modal_body<T: PaneDelegate + SettingsDelegate>(
    root: &PathBuf,
    cx: &mut Context<T>,
) -> AnyElement {
    let (stashes, expanded) = {
        let state = cx.global::<GitUiState>();
        (state.stashes.clone(), state.expanded_stashes.clone())
    };
    let theme = *cx.theme();

    let mut body = div().flex().flex_col().gap_2();
    if stashes.is_empty() {
        body = body.child(
            div()
                .rounded(rems(0.375))
                .border_1()
                .border_color(theme.border_subtle)
                .p_3()
                .text_sm()
                .text_color(theme.text_subtle)
                .child("No stashes"),
        );
    } else {
        body = body.children(
            stashes
                .into_iter()
                .map(|stash| stash_row(root.clone(), stash, expanded.clone(), cx)),
        );
    }
    body.into_any_element()
}

fn stash_row<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    stash: Stash,
    expanded: std::collections::HashSet<String>,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let is_expanded = expanded.contains(&stash.id);
    let toggle_id = stash.id.clone();
    let apply_id = stash.id.clone();
    let delete_id = stash.id.clone();
    let root_apply = root.clone();
    let root_delete = root.clone();

    div()
        .flex()
        .flex_col()
        .gap_2()
        .rounded(rems(0.375))
        .border_1()
        .border_color(theme.border_subtle)
        .p_2()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .items_start()
                        .gap_2()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |_, _, _, cx| {
                                toggle_stash(&toggle_id, cx);
                            }),
                        )
                        .child(
                            div().mt(rems(0.1875)).child(
                                Icon::new(if is_expanded {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .size(14.0)
                                .color(theme.text_muted),
                            ),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(theme.text)
                                        .child(stash.id.clone()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.text_subtle)
                                        .child(stash.message.clone()),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(icon_action_button(
                            SharedString::from(format!("git-apply-stash:{}", stash.id)),
                            IconName::ArrowDown,
                            theme.text_muted,
                            cx.listener(move |_, _, _, cx| {
                                run_modal_action(
                                    root_apply.clone(),
                                    GitModal::Stashes,
                                    {
                                        let apply_id = apply_id.clone();
                                        move |root| kosmos_git::apply_stash(root, &apply_id)
                                    },
                                    cx,
                                );
                            }),
                            cx,
                        ))
                        .child(delete_button(
                            SharedString::from(format!("git-delete-stash:{}", stash.id)),
                            cx.listener(move |_, _, _, cx| {
                                run_modal_action(
                                    root_delete.clone(),
                                    GitModal::Stashes,
                                    {
                                        let delete_id = delete_id.clone();
                                        move |root| kosmos_git::delete_stash(root, &delete_id)
                                    },
                                    cx,
                                );
                            }),
                            cx,
                        )),
                ),
        )
        .when(is_expanded, |this| {
            this.child(
                div().ml_6().flex().flex_col().gap_1().children(
                    stash
                        .files
                        .into_iter()
                        .map(|file| div().text_xs().text_color(theme.text_subtle).child(file)),
                ),
            )
        })
        .into_any_element()
}

fn input_row(label: &'static str, input: Entity<TextInput>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_xs().child(label))
        .child(input)
        .into_any_element()
}

fn section_label(label: &'static str, theme: theme::Theme) -> AnyElement {
    div()
        .pt_2()
        .text_xs()
        .text_color(theme.text_subtle)
        .child(label)
        .into_any_element()
}

fn remote_row<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    remote: Remote,
    cx: &mut Context<T>,
) -> AnyElement {
    let name = remote.name.clone();
    list_row(
        remote.name,
        remote.url,
        cx.listener(move |_, _, _, cx| {
            let name = name.clone();
            run_modal_action(
                root.clone(),
                GitModal::Remotes,
                move |root| kosmos_git::delete_remote(root, &name),
                cx,
            );
        }),
        cx,
    )
}

fn tag_row<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    tag: Tag,
    cx: &mut Context<T>,
) -> AnyElement {
    let name = tag.name.clone();
    list_row(
        tag.name,
        tag.message,
        cx.listener(move |_, _, _, cx| {
            let name = name.clone();
            run_modal_action(
                root.clone(),
                GitModal::Tags,
                move |root| kosmos_git::delete_tag(root, &name),
                cx,
            );
        }),
        cx,
    )
}

fn list_row<T: PaneDelegate + SettingsDelegate>(
    title: String,
    subtitle: String,
    delete: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .rounded(rems(0.375))
        .border_1()
        .border_color(theme.border_subtle)
        .p_2()
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_sm().text_color(theme.text).child(title))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.text_subtle)
                        .child(subtitle),
                ),
        )
        .child(delete_button("git-delete-list-item", delete, cx))
        .into_any_element()
}

fn delete_button<T: PaneDelegate + SettingsDelegate>(
    id: impl Into<gpui::ElementId>,
    listener: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    icon_action_button(id, IconName::Trash, theme.danger, listener, cx)
}

fn icon_action_button<T: PaneDelegate + SettingsDelegate>(
    id: impl Into<gpui::ElementId>,
    icon: IconName,
    color: gpui::Rgba,
    listener: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    div()
        .id(id)
        .size(rems(1.375))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(rems(0.25))
        .text_color(color)
        .hover(move |this| this.bg(theme.bg_hover))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |event, window, cx| {
            cx.stop_propagation();
            listener(event, window, cx);
        })
        .child(Icon::new(icon).size(14.0).color(color))
        .into_any_element()
}

fn modal_footer<T: PaneDelegate + SettingsDelegate>(
    button: AnyElement,
    _cx: &mut Context<T>,
) -> AnyElement {
    div().flex().justify_end().child(button).into_any_element()
}

fn close_modal_button<T: PaneDelegate + SettingsDelegate>(cx: &mut Context<T>) -> AnyElement {
    action_button(
        "git-close-modal",
        "Close",
        false,
        cx.listener(|_, _, _, cx| close_modal(cx)),
        cx,
    )
}

fn action_button<T: PaneDelegate + SettingsDelegate>(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    danger: bool,
    listener: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    div()
        .id(id)
        .rounded(rems(0.3125))
        .border_1()
        .border_color(if danger { theme.danger } else { theme.border })
        .bg(theme.bg_elevated)
        .px_3()
        .py_1()
        .text_sm()
        .text_color(if danger { theme.danger } else { theme.text })
        .hover(move |this| this.bg(theme.bg_hover))
        .on_click(listener)
        .child(label)
        .into_any_element()
}

fn commit_button<T: PaneDelegate + SettingsDelegate>(
    enabled: bool,
    listener: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    div()
        .id("git-commit-tracked")
        .rounded(rems(0.3125))
        .border_1()
        .border_color(if enabled {
            gpui::Hsla::from(theme.accent)
        } else {
            gpui::Hsla::from(theme.border)
        })
        .bg(if enabled {
            theme.accent
        } else {
            theme.bg_elevated
        })
        .px_3()
        .py_1()
        .text_sm()
        .text_color(if enabled {
            theme.bg_surface
        } else {
            theme.text_subtle
        })
        .when(enabled, |this| {
            this.hover(move |this| this.bg(gpui::Hsla::from(theme.accent).opacity(0.85)))
                .on_click(listener)
        })
        .child("Commit Tracked")
        .into_any_element()
}

fn change_list<T: PaneDelegate + SettingsDelegate>(
    root: &PathBuf,
    summary: &RepositorySummary,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let tree = build_change_tree(&summary.files);
    div()
        .id("git-change-list")
        .flex_1()
        .min_h_0()
        .bg(theme.bg_surface)
        .overflow_y_scroll()
        .when(summary.files.is_empty(), |this| {
            this.flex().items_center().justify_center()
        })
        .when(!summary.files.is_empty(), |this| {
            this.child(
                div()
                    .flex_none()
                    .px_4()
                    .pt_3()
                    .pb_2()
                    .text_xs()
                    .text_color(theme.text_subtle)
                    .child("TRACKED"),
            )
        })
        .when(summary.files.is_empty(), |this| {
            this.child(
                div()
                    .text_sm()
                    .text_color(theme.text_subtle)
                    .child("No changes"),
            )
        })
        .children(
            tree.dirs
                .into_values()
                .map(|node| change_dir_row(root.clone(), node, 0, true, cx)),
        )
        .children(
            tree.files
                .into_iter()
                .map(|change| change_file_row(root.clone(), change, 0, cx)),
        )
        .into_any_element()
}

#[derive(Default)]
struct ChangeTreeNode {
    name: String,
    path: String,
    dirs: std::collections::BTreeMap<String, ChangeTreeNode>,
    files: Vec<FileChange>,
}

fn build_change_tree(files: &[FileChange]) -> ChangeTreeNode {
    let mut root = ChangeTreeNode::default();
    for change in files {
        let mut parts = change.path.split('/').collect::<Vec<_>>();
        let Some(file_name) = parts.pop() else {
            continue;
        };
        let mut node = &mut root;
        let mut path = String::new();
        for part in parts {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(part);
            node = node
                .dirs
                .entry(part.to_string())
                .or_insert_with(|| ChangeTreeNode {
                    name: part.to_string(),
                    path: path.clone(),
                    ..Default::default()
                });
        }
        let mut file = change.clone();
        file.path = if node.path.is_empty() {
            file_name.to_string()
        } else {
            format!("{}/{}", node.path, file_name)
        };
        node.files.push(file);
    }
    root
}

fn change_dir_row<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    mut node: ChangeTreeNode,
    depth: usize,
    keep_separate: bool,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let mut label = node.name.clone();
    while !keep_separate && node.files.is_empty() && node.dirs.len() == 1 {
        let (_, child) = node.dirs.into_iter().next().unwrap();
        label = format!("{label}/{}", child.name);
        node = child;
    }
    let stats = node_stats(&node);
    let path = node.path.clone();
    let is_expanded = !cx
        .global::<GitUiState>()
        .collapsed_change_dirs
        .contains(&path);
    let toggle_path = path.clone();

    div()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .h(rems(CHANGE_ROW_HEIGHT_REM))
                .px(rems(CHANGE_ROW_PADDING_REM))
                .hover(move |this| this.bg(theme.bg_hover))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |_, _, _, cx| {
                        toggle_change_dir(&toggle_path, cx);
                    }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .child(change_indent_guides(depth, theme))
                        .child(
                            div()
                                .w(rems(CHANGE_ICON_WIDTH_REM))
                                .h(rems(CHANGE_ROW_HEIGHT_REM))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    Icon::new(if is_expanded {
                                        IconName::FolderOpened
                                    } else {
                                        IconName::Folder
                                    })
                                    .size(14.0)
                                    .color(theme.text_muted),
                                ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .pl(rems(CHANGE_LABEL_PADDING_REM))
                                .text_sm()
                                .text_color(theme.text)
                                .child(label),
                        ),
                )
                .child(stage_checkbox(
                    SharedString::from(format!("git-folder-toggle:{path}")),
                    stats.staged == stats.total,
                    root.clone(),
                    path,
                    cx,
                )),
        )
        .when(is_expanded, |this| {
            this.children(
                node.dirs
                    .into_values()
                    .map(|child| change_dir_row(root.clone(), child, depth + 1, false, cx)),
            )
            .children(
                node.files
                    .into_iter()
                    .map(|change| change_file_row(root.clone(), change, depth + 1, cx)),
            )
        })
        .into_any_element()
}

fn change_file_row<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    change: FileChange,
    depth: usize,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let name = change
        .path
        .rsplit_once('/')
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| change.path.clone());
    let icon_name = icon_for_git_file(Path::new(&change.path));
    let icon_color = match change.kind {
        FileChangeKind::Created => rgb(0x22c55e),
        FileChangeKind::Modified => theme.text_muted,
        FileChangeKind::Deleted => theme.danger,
        FileChangeKind::Renamed => rgb(0xa855f7),
    };

    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .h(rems(CHANGE_ROW_HEIGHT_REM))
        .px(rems(CHANGE_ROW_PADDING_REM))
        .hover(move |this| this.bg(theme.bg_hover))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .child(change_indent_guides(depth, theme))
                .child(
                    div()
                        .w(rems(CHANGE_ICON_WIDTH_REM))
                        .h(rems(CHANGE_ROW_HEIGHT_REM))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Icon::new(icon_name).size(14.0).color(icon_color)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .pl(rems(CHANGE_LABEL_PADDING_REM))
                        .text_sm()
                        .text_color(if change.kind == FileChangeKind::Deleted {
                            theme.text_subtle
                        } else {
                            theme.text
                        })
                        .child(name),
                ),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_3()
                .child(change_stats(&change, theme))
                .child(stage_checkbox(
                    SharedString::from(format!("git-file-toggle:{}", change.path)),
                    change.staged,
                    root,
                    change.path,
                    cx,
                )),
        )
        .into_any_element()
}

#[derive(Default)]
struct ChangeNodeStats {
    total: usize,
    staged: usize,
}

fn node_stats(node: &ChangeTreeNode) -> ChangeNodeStats {
    let mut stats = ChangeNodeStats::default();
    for file in &node.files {
        stats.total += 1;
        if file.staged {
            stats.staged += 1;
        }
    }
    for child in node.dirs.values() {
        let child_stats = node_stats(child);
        stats.total += child_stats.total;
        stats.staged += child_stats.staged;
    }
    stats
}

fn change_indent_guides(depth: usize, theme: theme::Theme) -> AnyElement {
    if depth == 0 {
        return div().flex_none().into_any_element();
    }

    let mut row = div().flex().flex_none().h(rems(CHANGE_ROW_HEIGHT_REM));
    for _ in 0..depth {
        row = row.child(
            div()
                .relative()
                .w(rems(CHANGE_INDENT_REM))
                .h(rems(CHANGE_ROW_HEIGHT_REM))
                .flex_none()
                .child(
                    div()
                        .absolute()
                        .left(rems(CHANGE_GUIDE_OFFSET_REM))
                        .top_0()
                        .bottom_0()
                        .w(rems(CHANGE_GUIDE_WIDTH_REM))
                        .bg(gpui::Hsla::from(theme.text).opacity(0.1)),
                ),
        );
    }
    row.into_any_element()
}

fn icon_for_git_file(path: &Path) -> IconName {
    if let Some(name) = path.file_name().and_then(|name| name.to_str())
        && let Some(icon) = IconName::for_file_name(name)
    {
        return icon;
    }

    language::from_path(path)
        .and_then(|id| IconName::for_language(id.as_str()))
        .unwrap_or(IconName::File)
}

fn change_stats(change: &FileChange, theme: theme::Theme) -> AnyElement {
    let added = rgb(0x22c55e);
    div()
        .flex()
        .items_center()
        .gap_1()
        .text_sm()
        .when(change.insertions > 0, |this| {
            this.child(
                div()
                    .text_color(added)
                    .child(format!("+{}", change.insertions)),
            )
        })
        .when(change.deletions > 0, |this| {
            this.child(
                div()
                    .text_color(theme.danger)
                    .child(format!("-{}", change.deletions)),
            )
        })
        .when(change.insertions == 0 && change.deletions == 0, |this| {
            this.child(div().text_color(theme.text_subtle).child("0"))
        })
        .into_any_element()
}

fn stage_checkbox<T: PaneDelegate + SettingsDelegate>(
    id: SharedString,
    staged: bool,
    root: PathBuf,
    path: String,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let unselected_color = gpui::Hsla::from(if theme.is_dark {
        rgb(0xffffff)
    } else {
        rgb(0x000000)
    })
    .opacity(0.28);
    div()
        .id(id)
        .size(rems(1.125))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(rems(0.0625))
        .border_1()
        .border_color(if staged {
            gpui::Hsla::from(theme.accent)
        } else {
            unselected_color
        })
        .bg(gpui::Hsla::from(theme.bg_surface).opacity(0.0))
        .hover(move |this| {
            this.border_color(if staged {
                gpui::Hsla::from(theme.accent)
            } else {
                gpui::Hsla::from(theme.text_subtle)
            })
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |_, _, _, cx| {
            let path = path.clone();
            if staged {
                run_git_action(
                    root.clone(),
                    move |root| kosmos_git::unstage_file(root, &path),
                    cx,
                );
            } else {
                run_git_action(
                    root.clone(),
                    move |root| kosmos_git::stage_file(root, &path),
                    cx,
                );
            }
        }))
        .when(staged, |this| {
            this.child(
                div()
                    .size(rems(0.625))
                    .rounded(rems(0.03125))
                    .bg(theme.accent),
            )
        })
        .into_any_element()
}

fn error_banner<T: PaneDelegate + SettingsDelegate>(
    message: String,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .px_3()
        .py_1()
        .bg(gpui::Hsla::from(theme.danger).opacity(0.15))
        .text_xs()
        .text_color(theme.text)
        .child(div().flex_1().min_w_0().child(message))
        .child(
            div()
                .id("git-error-dismiss")
                .size(rems(1.25))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(rems(0.25))
                .hover(move |s| s.bg(theme.bg_hover))
                .on_click(cx.listener(|_, _, _, cx| {
                    clear_error(cx);
                    cx.notify();
                }))
                .child(
                    Icon::new(IconName::Close)
                        .size(12.0)
                        .color(theme.text_muted),
                ),
        )
        .into_any_element()
}

fn empty_state<T: PaneDelegate + SettingsDelegate>(
    message: &'static str,
    cx: &mut Context<T>,
) -> AnyElement {
    centered_state(registry::GIT.icon, message.to_string(), cx)
}

fn centered_state<T: PaneDelegate + SettingsDelegate>(
    icon: IconName,
    message: String,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .bg(theme.bg_surface)
        .text_color(theme.text_subtle)
        .child(Icon::new(icon).size(28.0).color(theme.text_muted))
        .child(div().text_sm().child(message))
        .into_any_element()
}

fn ensure_state<T: PaneDelegate + SettingsDelegate>(cx: &mut Context<T>) {
    if cx.try_global::<GitUiState>().is_none() {
        let commit_message = cx.new(|cx| {
            TextArea::new("", "Commit message", cx)
                .height_rem(COMMIT_MESSAGE_HEIGHT_REM)
                .padding_x_rem(COMMIT_MESSAGE_PADDING_X_REM)
                .padding_top_rem(COMMIT_MESSAGE_PADDING_TOP_REM)
                .padding_bottom_rem(COMMIT_MESSAGE_PADDING_BOTTOM_REM)
                .unframed()
        });
        let branch_search = cx.new(|cx| TextInput::new("", "Search branches", cx));
        cx.subscribe(&branch_search, |_, _, _: &ValueChanged, cx| cx.notify())
            .detach();
        let branch_name = cx.new(|cx| TextInput::new("", "feature/my-branch", cx));
        let remote_name = cx.new(|cx| TextInput::new("", "origin", cx));
        let remote_url = cx.new(|cx| TextInput::new("", "https://github.com/user/repo.git", cx));
        let tag_name = cx.new(|cx| TextInput::new("", "v1.0.0", cx));
        let tag_message = cx.new(|cx| TextInput::new("", "Release notes", cx));
        let tag_sha = cx.new(|cx| TextInput::new("", "HEAD", cx));
        cx.set_global(GitUiState {
            commit_message: Some(commit_message),
            branch_search: Some(branch_search),
            branch_name: Some(branch_name),
            remote_name: Some(remote_name),
            remote_url: Some(remote_url),
            tag_name: Some(tag_name),
            tag_message: Some(tag_message),
            tag_sha: Some(tag_sha),
            ..Default::default()
        });
    }
}

fn open_modal<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    modal: GitModal,
    cx: &mut Context<T>,
) {
    close_menu(cx);
    cx.update_global::<GitUiState, _>(|state, _| {
        state.root = Some(root.clone());
        state.modal = Some(modal);
        state.last_error = None;
    });
    refresh_modal_data(root, modal, cx);
    cx.notify();
}

fn close_modal(cx: &mut App) {
    cx.update_global::<GitUiState, _>(|state, _| state.modal = None);
}

fn selected_change_paths<T: PaneDelegate + SettingsDelegate>(cx: &mut Context<T>) -> Vec<String> {
    cx.global::<GitUiState>()
        .summary
        .as_ref()
        .map(|summary| {
            summary
                .files
                .iter()
                .filter(|file| file.staged)
                .map(|file| file.path.clone())
                .collect()
        })
        .unwrap_or_default()
}

fn toggle_stash<T: PaneDelegate + SettingsDelegate>(id: &str, cx: &mut Context<T>) {
    cx.update_global::<GitUiState, _>(|state, _| {
        if !state.expanded_stashes.remove(id) {
            state.expanded_stashes.insert(id.to_string());
        }
    });
    cx.notify();
}

fn toggle_change_dir<T: PaneDelegate + SettingsDelegate>(path: &str, cx: &mut Context<T>) {
    cx.update_global::<GitUiState, _>(|state, _| {
        if !state.collapsed_change_dirs.remove(path) {
            state.collapsed_change_dirs.insert(path.to_string());
        }
    });
    cx.notify();
}

fn refresh_modal_data<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    modal: GitModal,
    cx: &mut Context<T>,
) {
    cx.spawn(async move |this, cx| match modal {
        GitModal::Branches => {
            let result = cx
                .background_executor()
                .spawn(async move { kosmos_git::list_branches(root) })
                .await;
            let _ = this.update(cx, |_, cx| {
                apply_modal_list_result(modal, result.map(ModalList::Branches), cx);
            });
        }
        GitModal::Remotes => {
            let result = cx
                .background_executor()
                .spawn(async move { kosmos_git::list_remotes(root) })
                .await;
            let _ = this.update(cx, |_, cx| {
                apply_modal_list_result(modal, result.map(ModalList::Remotes), cx);
            });
        }
        GitModal::Stashes => {
            let result = cx
                .background_executor()
                .spawn(async move { kosmos_git::list_stashes(root) })
                .await;
            let _ = this.update(cx, |_, cx| {
                apply_modal_list_result(modal, result.map(ModalList::Stashes), cx);
            });
        }
        GitModal::Tags => {
            let result = cx
                .background_executor()
                .spawn(async move { kosmos_git::list_tags(root) })
                .await;
            let _ = this.update(cx, |_, cx| {
                apply_modal_list_result(modal, result.map(ModalList::Tags), cx);
            });
        }
        GitModal::CreateBranch | GitModal::ConfirmDiscardSelected | GitModal::ConfirmDiscard => {}
    })
    .detach();
}

enum ModalList {
    Branches(Vec<Branch>),
    Remotes(Vec<Remote>),
    Stashes(Vec<Stash>),
    Tags(Vec<Tag>),
}

fn apply_modal_list_result<T: PaneDelegate + SettingsDelegate>(
    modal: GitModal,
    result: Result<ModalList, kosmos_git::Error>,
    cx: &mut Context<T>,
) {
    cx.update_global::<GitUiState, _>(|state, _| match result {
        Ok(ModalList::Branches(branches)) if modal == GitModal::Branches => {
            state.branches = branches;
            state.last_error = None;
        }
        Ok(ModalList::Remotes(remotes)) if modal == GitModal::Remotes => {
            state.remotes = remotes;
            state.last_error = None;
        }
        Ok(ModalList::Stashes(stashes)) if modal == GitModal::Stashes => {
            state.stashes = stashes;
            state.last_error = None;
        }
        Ok(ModalList::Tags(tags)) if modal == GitModal::Tags => {
            state.tags = tags;
            state.last_error = None;
        }
        Ok(_) => {}
        Err(error) => state.last_error = Some(error.to_string()),
    });
    cx.notify();
}

fn run_modal_action<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    modal: GitModal,
    action: impl FnOnce(PathBuf) -> Result<(), kosmos_git::Error> + Send + 'static,
    cx: &mut Context<T>,
) {
    run_modal_action_after_success(root, modal, action, |_| {}, cx);
}

fn run_modal_action_after_success<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    modal: GitModal,
    action: impl FnOnce(PathBuf) -> Result<(), kosmos_git::Error> + Send + 'static,
    on_success: impl FnOnce(&mut Context<T>) + 'static,
    cx: &mut Context<T>,
) {
    clear_error(cx);
    cx.spawn(async move |this, cx| {
        let action_root = root.clone();
        let result = cx
            .background_executor()
            .spawn(async move { action(action_root) })
            .await;
        let _ = this.update(cx, |_, cx| match result {
            Ok(()) => {
                on_success(cx);
                refresh_modal_data(root.clone(), modal, cx);
                refresh_summary(root, true, false, cx);
            }
            Err(error) => {
                cx.update_global::<GitUiState, _>(|state, _| {
                    state.last_error = Some(error.to_string())
                });
                cx.notify();
            }
        });
    })
    .detach();
}

fn ensure_summary<T: PaneDelegate + SettingsDelegate>(root: &PathBuf, cx: &mut Context<T>) {
    ensure_summary_watch(root, cx);

    let needs_refresh = {
        let state = cx.global::<GitUiState>();
        state.root.as_ref() != Some(root) || (!state.loading && state.summary.is_none())
    };
    if needs_refresh {
        refresh_summary(root.clone(), false, true, cx);
    }
}

fn ensure_summary_watch<T: PaneDelegate + SettingsDelegate>(root: &PathBuf, cx: &mut Context<T>) {
    let generation = cx.update_global::<GitUiState, _>(|state, _| {
        if state.root.as_ref() == Some(root) && state.watch_task.is_some() {
            return None;
        }

        state.watch_generation = state.watch_generation.wrapping_add(1);
        Some(state.watch_generation)
    });

    let Some(generation) = generation else {
        return;
    };

    let root = root.clone();
    let task = cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(750))
                .await;

            let refresh_root = root.clone();
            let result = cx
                .background_executor()
                .spawn(async move { RepositorySummary::discover(refresh_root) })
                .await;

            let should_continue = this
                .update(cx, |_, cx| {
                    apply_watched_summary(&root, generation, result, cx)
                })
                .unwrap_or(false);

            if !should_continue {
                break;
            }
        }
    });

    cx.update_global::<GitUiState, _>(|state, _| {
        state.watch_task = Some(task);
    });
}

fn apply_watched_summary<T: PaneDelegate + SettingsDelegate>(
    root: &PathBuf,
    generation: u64,
    result: Result<RepositorySummary, kosmos_git::Error>,
    cx: &mut Context<T>,
) -> bool {
    let mut changed = false;
    let should_continue = cx.update_global::<GitUiState, _>(|state, _| {
        if state.watch_generation != generation || state.root.as_ref() != Some(root) {
            return false;
        }

        match result {
            Ok(summary) => {
                changed = state.summary.as_ref() != Some(&summary) || state.last_error.is_some();
                state.summary = Some(summary);
                state.last_error = None;
            }
            Err(error) => {
                let error = error.to_string();
                changed = state.summary.is_some() || state.last_error.as_ref() != Some(&error);
                state.summary = None;
                state.last_error = Some(error);
            }
        }
        true
    });

    if changed {
        cx.notify();
    }

    should_continue
}

fn refresh_summary<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    notify_now: bool,
    show_loading: bool,
    cx: &mut Context<T>,
) {
    let generation = cx.update_global::<GitUiState, _>(|state, _| {
        if state.root.as_ref() != Some(&root) {
            state.summary = None;
            state.last_error = None;
            state.collapsed_change_dirs.clear();
        }
        state.root = Some(root.clone());
        if show_loading {
            state.loading = true;
        }
        state.refresh_generation = state.refresh_generation.wrapping_add(1);
        state.refresh_generation
    });

    if notify_now {
        cx.notify();
    }

    let task = cx.spawn(async move |this, cx| {
        let result = cx
            .background_executor()
            .spawn(async move { RepositorySummary::discover(&root) })
            .await;
        let _ = this.update(cx, |_, cx| {
            cx.update_global::<GitUiState, _>(|state, _| {
                if state.refresh_generation != generation {
                    return;
                }
                state.loading = false;
                match result {
                    Ok(summary) => {
                        state.summary = Some(summary);
                        state.last_error = None;
                    }
                    Err(error) => {
                        state.summary = None;
                        state.last_error = Some(error.to_string());
                    }
                }
            });
            cx.notify();
        });
    });

    cx.update_global::<GitUiState, _>(|state, _| {
        state.refresh_task = Some(task);
    });
}

fn run_git_action<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    action: impl FnOnce(PathBuf) -> Result<(), kosmos_git::Error> + Send + 'static,
    cx: &mut Context<T>,
) {
    close_menu(cx);
    clear_error(cx);
    cx.update_global::<GitUiState, _>(|state, _| state.loading = true);
    cx.notify();

    cx.spawn(async move |this, cx| {
        let action_root = root.clone();
        let result = cx
            .background_executor()
            .spawn(async move { action(action_root) })
            .await;
        let _ = this.update(cx, |_, cx| match result {
            Ok(()) => refresh_summary(root, true, true, cx),
            Err(error) => {
                cx.update_global::<GitUiState, _>(|state, _| {
                    state.loading = false;
                    state.last_error = Some(error.to_string());
                });
                cx.notify();
            }
        });
    })
    .detach();
}

fn commit_tracked<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    message: String,
    input: Entity<TextArea>,
    cx: &mut Context<T>,
) {
    let message = message.trim().to_string();
    if message.is_empty() {
        cx.update_global::<GitUiState, _>(|state, _| {
            state.last_error = Some("Commit message is required".to_string())
        });
        cx.notify();
        return;
    }

    close_menu(cx);
    clear_error(cx);
    cx.update_global::<GitUiState, _>(|state, _| state.loading = true);
    cx.notify();

    cx.spawn(async move |this, cx| {
        let action_root = root.clone();
        let result = cx
            .background_executor()
            .spawn(async move { kosmos_git::commit_staged(action_root, &message) })
            .await;
        let _ = this.update(cx, |_, cx| match result {
            Ok(()) => {
                input.update(cx, |input, cx| input.set_value("", cx));
                refresh_summary(root, true, true, cx);
            }
            Err(error) => {
                cx.update_global::<GitUiState, _>(|state, _| {
                    state.loading = false;
                    state.last_error = Some(error.to_string());
                });
                cx.notify();
            }
        });
    })
    .detach();
}

fn clear_error(cx: &mut App) {
    cx.update_global::<GitUiState, _>(|state, _| state.last_error = None);
}

fn close_menu(cx: &mut App) {
    cx.update_global::<GitUiState, _>(|state, _| state.menu_position = None);
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}
