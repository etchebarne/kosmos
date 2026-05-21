use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

use gpui::{
    Animation, AnimationExt, AnyElement, App, ClickEvent, Context, Entity, Global, IntoElement,
    MouseButton, MouseDownEvent, Pixels, Point, SharedString, Task, Window, anchored, deferred, div,
    point, prelude::*, rems, rgb,
};

use file_tree::ActiveFileTree;
use icons::{Icon, IconName};
use kosmos_git::{
    Branch, CommitInfo, FileChange, FileChangeKind, Remote, RepositorySummary, Stash, Tag,
};
use gpui_component::{
    Disableable, Icon as ComponentIcon, Sizable,
    button::{Button, ButtonVariants},
    dialog::Dialog,
};
use tabs::registry;
use theme::ActiveTheme;

use crate::components::{TextArea, TextInput, ValueChanged, left_aligned_button_label, toast};
use crate::delegate::{PaneDelegate, SettingsDelegate};

thread_local! {
    static GIT_UI_NAMESPACE: RefCell<Vec<SharedString>> = const { RefCell::new(Vec::new()) };
}

pub fn with_git_ui_namespace<R>(namespace: impl Into<SharedString>, f: impl FnOnce() -> R) -> R {
    let _guard = GitUiNamespaceGuard::new(namespace.into());
    f()
}

struct GitUiNamespaceGuard;

impl GitUiNamespaceGuard {
    fn new(namespace: SharedString) -> Self {
        GIT_UI_NAMESPACE.with(|stack| stack.borrow_mut().push(namespace));
        Self
    }
}

impl Drop for GitUiNamespaceGuard {
    fn drop(&mut self) {
        GIT_UI_NAMESPACE.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}

fn current_git_ui_namespace() -> SharedString {
    GIT_UI_NAMESPACE.with(|stack| {
        stack
            .borrow()
            .last()
            .cloned()
            .unwrap_or_else(|| SharedString::new_static("git"))
    })
}

fn component_icon(icon: IconName) -> ComponentIcon {
    ComponentIcon::empty().path(icon.path())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GitModal {
    Branches,
    CreateBranch,
    Remotes,
    Stashes,
    Tags,
    ConfirmDiscardSelected,
    ConfirmDiscard,
    ConfirmResolveConflicts,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum GitSyncAction {
    #[default]
    Fetch,
    Pull,
    PullRebase,
    Push,
    ForcePush,
}

impl GitSyncAction {
    const ALL: [Self; 5] = [
        Self::Fetch,
        Self::Pull,
        Self::PullRebase,
        Self::Push,
        Self::ForcePush,
    ];

    fn id(self) -> &'static str {
        match self {
            Self::Fetch => "git-sync-menu-fetch",
            Self::Pull => "git-sync-menu-pull",
            Self::PullRebase => "git-sync-menu-pull-rebase",
            Self::Push => "git-sync-menu-push",
            Self::ForcePush => "git-sync-menu-force-push",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Fetch => "Fetch",
            Self::Pull => "Pull",
            Self::PullRebase => "Pull (Rebase)",
            Self::Push => "Push",
            Self::ForcePush => "Push (Force)",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::Fetch => IconName::Refresh,
            Self::Pull | Self::PullRebase => IconName::ArrowDown,
            Self::Push | Self::ForcePush => IconName::ArrowUp,
        }
    }

    fn success_title(self) -> &'static str {
        match self {
            Self::Fetch => "Fetch completed",
            Self::Pull => "Pull completed",
            Self::PullRebase => "Pull with rebase completed",
            Self::Push => "Push completed",
            Self::ForcePush => "Force push completed",
        }
    }

    fn error_title(self) -> &'static str {
        match self {
            Self::Fetch => "Fetch failed",
            Self::Pull => "Pull failed",
            Self::PullRebase => "Pull with rebase failed",
            Self::Push => "Push failed",
            Self::ForcePush => "Force push failed",
        }
    }

    fn is_danger(self) -> bool {
        matches!(self, Self::ForcePush)
    }
}

#[derive(Default)]
struct GitUiState {
    root: Option<PathBuf>,
    summary: Option<RepositorySummary>,
    can_initialize_repository: bool,
    loading: bool,
    refresh_generation: u64,
    refresh_task: Option<Task<()>>,
    watch_generation: u64,
    watch_task: Option<Task<()>>,
    menu_position: Option<Point<Pixels>>,
    menu_namespace: Option<SharedString>,
    sync_menu_position: Option<Point<Pixels>>,
    sync_menu_namespace: Option<SharedString>,
    modal: Option<GitModal>,
    last_error: Option<String>,
    pending_conflict_paths: Vec<String>,
    pending_conflict_resolution_stages_all: bool,
    last_sync_action: GitSyncAction,
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
const COMMIT_PANEL_HEIGHT_REM: f32 = 13.25;
const COMMIT_MESSAGE_HEIGHT_REM: f32 = 8.25;
const COMMIT_MESSAGE_PADDING_X_REM: f32 = 1.25;
const COMMIT_MESSAGE_PADDING_TOP_REM: f32 = 1.25;
const COMMIT_MESSAGE_PADDING_BOTTOM_REM: f32 = 0.5;
const COMMIT_CONTROLS_INSET_X_REM: f32 = 1.0;
const SYNC_PANEL_INSET_X_REM: f32 = 0.5;
const SYNC_MENU_GAP_REM: f32 = 0.5;
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
    let namespace = current_git_ui_namespace();

    let (summary, can_initialize_repository, loading, menu_position, sync_menu_position) = {
        let state = cx.global::<GitUiState>();
        (
            state.summary.clone(),
            state.can_initialize_repository,
            state.loading,
            state
                .menu_namespace
                .as_ref()
                .filter(|active| *active == &namespace)
                .and(state.menu_position),
            state
                .sync_menu_namespace
                .as_ref()
                .filter(|active| *active == &namespace)
                .and(state.sync_menu_position),
        )
    };
    let dismiss_layer = (menu_position.is_some() || sync_menu_position.is_some())
        .then(|| menu_dismiss_layer::<T>(cx));
    let menu_overlay = menu_position.map(|position| more_menu::<T>(&root, position, cx));
    let sync_menu_overlay =
        sync_menu_position.map(|position| sync_action_menu::<T>(&root, position, cx));

    div()
        .relative()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(theme.bg_surface)
        .text_color(theme.text)
        .child(header(&root, summary.as_ref(), loading, cx))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .when(summary.is_none(), |this| {
                    this.child(if loading {
                        loading_state(cx)
                    } else if can_initialize_repository {
                        init_repository_panel(&root, cx)
                    } else {
                        empty_panel("Git status unavailable", cx)
                    })
                })
                .when_some(summary.as_ref(), |this, summary| {
                    this.child(change_list(&root, summary, cx))
                }),
        )
        .when(!can_initialize_repository, |this| {
            this.child(commit_panel(&root, summary.as_ref(), cx))
        })
        .when_some(
            summary
                .as_ref()
                .and_then(|summary| summary.latest_commit.clone()),
            |this, commit| this.child(latest_commit_panel(commit, cx)),
        )
        .when_some(dismiss_layer, |this, layer| this.child(layer))
        .when_some(menu_overlay, |this, menu| this.child(menu))
        .when_some(sync_menu_overlay, |this, menu| this.child(menu))
        .into_any_element()
}

pub fn render_modal_overlay<T: PaneDelegate + SettingsDelegate>(
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let modal = cx
        .try_global::<GitUiState>()
        .and_then(|state| Some((state.root.clone()?, state.modal?)));
    let Some((root, modal_state)) = modal else {
        return div().into_any_element();
    };

    let backdrop = theme::Theme::dark().bg_root;
    let viewport_size = window.viewport_size();
    let backdrop_animation = Animation::new(Duration::from_millis(140));

    deferred(
        anchored()
            .position(point(Pixels::ZERO, Pixels::ZERO))
            .snap_to_window()
            .child(
                div()
                    .id("git-modal-overlay")
                    .relative()
                    .w(viewport_size.width)
                    .h(viewport_size.height)
                    .occlude()
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .bg(gpui::Hsla::from(backdrop).opacity(0.72))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|_, _, _, cx| {
                                    cx.stop_propagation();
                                    close_modal(cx);
                                }),
                            )
                            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                            .with_animation(
                                "git-modal-backdrop-fade",
                                backdrop_animation,
                                |this, delta| this.opacity(delta),
                            ),
                    )
                    .child(render_git_modal(&root, modal_state, window, cx)),
            ),
    )
    .with_priority(3)
    .into_any_element()
}

fn header<T: PaneDelegate + SettingsDelegate>(
    root: &Path,
    summary: Option<&RepositorySummary>,
    loading: bool,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let root_refresh = root.to_path_buf();
    let root_stage = root.to_path_buf();
    let root_unstage = root.to_path_buf();
    let root_stash = root.to_path_buf();

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
                    summary,
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
                        stage_all_changes(root_stage.clone(), cx);
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
    summary: Option<&RepositorySummary>,
    loading: bool,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let label = match summary {
        Some(summary) if conflict_count(summary) > 0 => {
            let conflicts = conflict_count(summary);
            if summary.changes == conflicts {
                format!("{conflicts} Conflict{}", plural(conflicts))
            } else {
                format!(
                    "{} Change{}, {conflicts} Conflict{}",
                    summary.changes,
                    plural(summary.changes),
                    plural(conflicts)
                )
            }
        }
        Some(summary) if summary.changes == 0 => "No Changes".to_string(),
        Some(summary) => format!("{} Change{}", summary.changes, plural(summary.changes)),
        None if loading => "Loading Changes".to_string(),
        None => "No Changes".to_string(),
    };
    div()
        .text_xs()
        .text_color(theme.text_emphasis)
        .child(label)
        .into_any_element()
}

fn conflict_count(summary: &RepositorySummary) -> usize {
    summary
        .files
        .iter()
        .filter(|file| file.kind == FileChangeKind::Conflicted)
        .count()
}
