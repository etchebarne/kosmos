fn stage_checkbox<T: PaneDelegate + SettingsDelegate>(
    id: SharedString,
    staged: bool,
    conflict_paths: Vec<String>,
    root: PathBuf,
    path: String,
    cx: &mut Context<T>,
) -> AnyElement {
    Checkbox::new(id)
        .large()
        .flex_none()
        .tab_stop(false)
        .checked(staged)
        .on_click(cx.listener(move |_, _: &bool, _, cx| {
            cx.stop_propagation();
            let path = path.clone();
            if staged {
                run_git_action(
                    root.clone(),
                    move |root| kosmos_git::unstage_file(root, &path),
                    cx,
                );
            } else if !conflict_paths.is_empty() {
                open_resolve_conflicts_modal(root.clone(), conflict_paths.clone(), false, cx);
            } else {
                run_git_action(
                    root.clone(),
                    move |root| kosmos_git::stage_file(root, &path),
                    cx,
                );
            }
        }))
        .into_any_element()
}

fn empty_state<T: PaneDelegate + SettingsDelegate>(
    message: &'static str,
    cx: &mut Context<T>,
) -> AnyElement {
    centered_state(
        super::icon_for_kind(registry::GIT.id),
        message.to_string(),
        cx,
    )
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

fn ensure_state<T: PaneDelegate + SettingsDelegate>(
    window: &mut Window,
    cx: &mut Context<T>,
) {
    if cx.try_global::<GitUiState>().is_none() {
        let commit_message = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .placeholder("Commit message")
        });
        let branch_search = cx.new(|cx| InputState::new(window, cx).placeholder("Search branches"));
        let branch_name = cx.new(|cx| InputState::new(window, cx).placeholder("feature/my-branch"));
        let remote_name = cx.new(|cx| InputState::new(window, cx).placeholder("origin"));
        let remote_url = cx.new(|cx| InputState::new(window, cx).placeholder("https://github.com/user/repo.git"));
        let tag_name = cx.new(|cx| InputState::new(window, cx).placeholder("v1.0.0"));
        let tag_message = cx.new(|cx| InputState::new(window, cx).placeholder("Release notes"));
        let tag_sha = cx.new(|cx| InputState::new(window, cx).placeholder("HEAD"));
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
    cx.update_global::<GitUiState, _>(|state, _| {
        state.root = Some(root.clone());
        state.modal = Some(modal);
        state.last_error = None;
        state.pending_conflict_paths.clear();
        state.pending_conflict_resolution_stages_all = false;
    });
    refresh_modal_data(root, modal, cx);
    cx.notify();
}

fn open_modal_app(root: PathBuf, modal: GitModal, cx: &mut App) {
    cx.update_global::<GitUiState, _>(|state, _| {
        state.root = Some(root.clone());
        state.modal = Some(modal);
        state.last_error = None;
        state.pending_conflict_paths.clear();
        state.pending_conflict_resolution_stages_all = false;
    });
    refresh_modal_data_app(root, modal, cx);
    cx.refresh_windows();
}

fn close_modal(cx: &mut App) {
    cx.update_global::<GitUiState, _>(|state, _| {
        state.modal = None;
        state.pending_conflict_paths.clear();
        state.pending_conflict_resolution_stages_all = false;
    });
}

fn stage_all_changes<T: PaneDelegate + SettingsDelegate>(root: PathBuf, cx: &mut Context<T>) {
    let conflict_paths = cx
        .global::<GitUiState>()
        .summary
        .as_ref()
        .map(conflict_paths_in_summary)
        .unwrap_or_default();
    if conflict_paths.is_empty() {
        run_git_action(root, kosmos_git::stage_all, cx);
    } else {
        open_resolve_conflicts_modal(root, conflict_paths, true, cx);
    }
}

fn open_resolve_conflicts_modal<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    conflict_paths: Vec<String>,
    stage_all_changes: bool,
    cx: &mut Context<T>,
) {
    if conflict_paths.is_empty() {
        return;
    }
    cx.update_global::<GitUiState, _>(|state, _| {
        state.root = Some(root);
        state.pending_conflict_paths = conflict_paths;
        state.pending_conflict_resolution_stages_all = stage_all_changes;
        state.modal = Some(GitModal::ConfirmResolveConflicts);
        state.last_error = None;
    });
    cx.notify();
}

fn conflict_paths_in_summary(summary: &RepositorySummary) -> Vec<String> {
    summary
        .files
        .iter()
        .filter(|file| file.kind == FileChangeKind::Conflicted)
        .map(|file| file.path.clone())
        .collect()
}

fn selected_change_paths(cx: &mut App) -> Vec<String> {
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

fn toggle_stash(id: &str, cx: &mut App) {
    cx.update_global::<GitUiState, _>(|state, _| {
        if !state.expanded_stashes.remove(id) {
            state.expanded_stashes.insert(id.to_string());
        }
    });
    cx.refresh_windows();
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
        GitModal::CreateBranch
        | GitModal::ConfirmDiscardSelected
        | GitModal::ConfirmDiscard
        | GitModal::ConfirmResolveConflicts => {}
    })
    .detach();
}

fn refresh_modal_data_app(root: PathBuf, modal: GitModal, cx: &mut App) {
    cx.spawn(async move |cx| {
        match modal {
            GitModal::Branches => {
                let result = cx
                    .background_executor()
                    .spawn(async move { kosmos_git::list_branches(root) })
                    .await;
                let _ = cx.update(|cx| {
                    apply_modal_list_result_app(modal, result.map(ModalList::Branches), cx);
                });
            }
            GitModal::Remotes => {
                let result = cx
                    .background_executor()
                    .spawn(async move { kosmos_git::list_remotes(root) })
                    .await;
                let _ = cx.update(|cx| {
                    apply_modal_list_result_app(modal, result.map(ModalList::Remotes), cx);
                });
            }
            GitModal::Stashes => {
                let result = cx
                    .background_executor()
                    .spawn(async move { kosmos_git::list_stashes(root) })
                    .await;
                let _ = cx.update(|cx| {
                    apply_modal_list_result_app(modal, result.map(ModalList::Stashes), cx);
                });
            }
            GitModal::Tags => {
                let result = cx
                    .background_executor()
                    .spawn(async move { kosmos_git::list_tags(root) })
                    .await;
                let _ = cx.update(|cx| {
                    apply_modal_list_result_app(modal, result.map(ModalList::Tags), cx);
                });
            }
            GitModal::CreateBranch
            | GitModal::ConfirmDiscardSelected
            | GitModal::ConfirmDiscard
            | GitModal::ConfirmResolveConflicts => {}
        }
    })
    .detach();
}

enum ModalList {
    Branches(Vec<Branch>),
    Remotes(Vec<Remote>),
    Stashes(Vec<Stash>),
    Tags(Vec<Tag>),
}

fn run_modal_action_app(
    root: PathBuf,
    modal: GitModal,
    action: impl FnOnce(PathBuf) -> Result<(), kosmos_git::Error> + Send + 'static,
    cx: &mut App,
) {
    run_modal_action_after_success_app(root, modal, action, |_| {}, cx);
}

fn run_modal_action_after_success_app(
    root: PathBuf,
    modal: GitModal,
    action: impl FnOnce(PathBuf) -> Result<(), kosmos_git::Error> + Send + 'static,
    on_success: impl FnOnce(&mut App) + 'static,
    cx: &mut App,
) {
    clear_error(cx);
    cx.update_global::<GitUiState, _>(|state, _| state.loading = true);
    cx.refresh_windows();

    cx.spawn(async move |cx| {
        let action_root = root.clone();
        let result = cx
            .background_executor()
            .spawn(async move { action(action_root) })
            .await;
        let _ = cx.update(|cx| match result {
            Ok(()) => {
                on_success(cx);
                refresh_modal_data_app(root.clone(), modal, cx);
                refresh_summary_app(root, true, false, cx);
            }
            Err(error) => {
                cx.update_global::<GitUiState, _>(|state, _| {
                    state.last_error = Some(error.to_string())
                });
                cx.refresh_windows();
            }
        });
    })
    .detach();
}

fn spawn_git_action<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    action: impl FnOnce(PathBuf) -> Result<(), kosmos_git::Error> + Send + 'static,
    on_success: impl FnOnce(PathBuf, &mut Context<T>) + 'static,
    on_error: impl FnOnce(kosmos_git::Error, &mut Context<T>) + 'static,
    cx: &mut Context<T>,
) {
    cx.spawn(async move |this, cx| {
        let action_root = root.clone();
        let result = cx
            .background_executor()
            .spawn(async move { action(action_root) })
            .await;
        let _ = this.update(cx, |_, cx| match result {
            Ok(()) => on_success(root, cx),
            Err(error) => on_error(error, cx),
        });
    })
    .detach();
}

fn apply_git_action_error<T: PaneDelegate + SettingsDelegate>(
    error: kosmos_git::Error,
    cx: &mut Context<T>,
) {
    cx.update_global::<GitUiState, _>(|state, _| {
        state.loading = false;
        state.last_error = Some(error.to_string());
    });
    cx.notify();
}

fn ensure_summary<T: PaneDelegate + SettingsDelegate>(root: &Path, cx: &mut Context<T>) {
    ensure_summary_watch(root, cx);

    let needs_refresh = {
        let state = cx.global::<GitUiState>();
        state.root.as_deref() != Some(root)
            || (!state.loading && state.summary.is_none() && !state.can_initialize_repository)
    };
    if needs_refresh {
        refresh_summary(root.to_path_buf(), false, true, cx);
    }
}

fn ensure_summary_watch<T: PaneDelegate + SettingsDelegate>(root: &Path, cx: &mut Context<T>) {
    let generation = cx.update_global::<GitUiState, _>(|state, _| {
        if state.root.as_deref() == Some(root) && state.watch_task.is_some() {
            return None;
        }

        state.watch_generation = state.watch_generation.wrapping_add(1);
        Some(state.watch_generation)
    });

    let Some(generation) = generation else {
        return;
    };

    let root = root.to_path_buf();
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
    root: &Path,
    generation: u64,
    result: Result<RepositorySummary, kosmos_git::Error>,
    cx: &mut Context<T>,
) -> bool {
    let mut changed = false;
    let should_continue = cx.update_global::<GitUiState, _>(|state, _| {
        if state.watch_generation != generation || state.root.as_deref() != Some(root) {
            return false;
        }

        match result {
            Ok(summary) => {
                changed = state.summary.as_ref() != Some(&summary)
                    || state.last_error.is_some()
                    || state.can_initialize_repository;
                state.summary = Some(summary);
                state.last_error = None;
                state.can_initialize_repository = false;
            }
            Err(error) => {
                let is_missing_repository = is_missing_repository(&error);
                let error = error.to_string();
                changed = state.summary.is_some()
                    || state.last_error.as_ref() != Some(&error)
                    || state.can_initialize_repository != is_missing_repository;
                state.summary = None;
                state.last_error = Some(error);
                state.can_initialize_repository = is_missing_repository;
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
            state.can_initialize_repository = false;
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
                        state.can_initialize_repository = false;
                    }
                    Err(error) => {
                        state.can_initialize_repository = is_missing_repository(&error);
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
    clear_error(cx);
    cx.update_global::<GitUiState, _>(|state, _| state.loading = true);
    cx.notify();

    spawn_git_action(
        root,
        action,
        |root, cx| refresh_summary(root, true, true, cx),
        apply_git_action_error,
        cx,
    );
}

fn run_git_action_app(
    root: PathBuf,
    action: impl FnOnce(PathBuf) -> Result<(), kosmos_git::Error> + Send + 'static,
    cx: &mut App,
) {
    clear_error(cx);
    cx.update_global::<GitUiState, _>(|state, _| state.loading = true);
    cx.refresh_windows();

    cx.spawn(async move |cx| {
        let action_root = root.clone();
        let result = cx
            .background_executor()
            .spawn(async move { action(action_root) })
            .await;
        let _ = cx.update(|cx| match result {
            Ok(()) => refresh_summary_app(root, true, true, cx),
            Err(error) => apply_git_action_error_app(error, cx),
        });
    })
    .detach();
}

fn apply_git_action_error_app(error: kosmos_git::Error, cx: &mut App) {
    cx.update_global::<GitUiState, _>(|state, _| {
        state.loading = false;
        state.last_error = Some(error.to_string());
    });
    cx.refresh_windows();
}

fn refresh_summary_app(root: PathBuf, notify_now: bool, show_loading: bool, cx: &mut App) {
    let generation = cx.update_global::<GitUiState, _>(|state, _| {
        if state.root.as_ref() != Some(&root) {
            state.summary = None;
            state.last_error = None;
            state.can_initialize_repository = false;
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
        cx.refresh_windows();
    }

    cx.spawn(async move |cx| {
        let result = cx
            .background_executor()
            .spawn(async move { RepositorySummary::discover(&root) })
            .await;
        let _ = cx.update(|cx| {
            cx.update_global::<GitUiState, _>(|state, _| {
                if state.refresh_generation != generation {
                    return;
                }
                state.loading = false;
                match result {
                    Ok(summary) => {
                        state.summary = Some(summary);
                        state.last_error = None;
                        state.can_initialize_repository = false;
                    }
                    Err(error) => {
                        state.can_initialize_repository = is_missing_repository(&error);
                        state.summary = None;
                        state.last_error = Some(error.to_string());
                    }
                }
            });
            cx.refresh_windows();
        });
    })
    .detach();
}

fn run_git_action_with_toast<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    success_title: &'static str,
    error_title: &'static str,
    action: impl FnOnce(PathBuf) -> Result<(), kosmos_git::Error> + Send + 'static,
    cx: &mut Context<T>,
) {
    clear_error(cx);
    cx.update_global::<GitUiState, _>(|state, _| state.loading = true);
    cx.notify();

    spawn_git_action(
        root,
        action,
        move |root, cx| {
            toast::show_success(cx, success_title);
            refresh_summary(root, true, true, cx);
        },
        move |error, cx| {
            cx.update_global::<GitUiState, _>(|state, _| {
                state.loading = false;
            });
            toast::show_error(cx, error_title, git_error_message(error));
            cx.notify();
        },
        cx,
    );
}

fn run_sync_action<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    action: GitSyncAction,
    remember: bool,
    cx: &mut Context<T>,
) {
    if remember {
        cx.update_global::<GitUiState, _>(|state, _| {
            state.last_sync_action = action;
        });
    }

    run_git_action_with_toast(
        root,
        action.success_title(),
        action.error_title(),
        move |root| match action {
            GitSyncAction::Fetch => kosmos_git::fetch(root),
            GitSyncAction::Pull => kosmos_git::pull(root),
            GitSyncAction::PullRebase => kosmos_git::pull_rebase(root),
            GitSyncAction::Push => kosmos_git::push(root),
            GitSyncAction::ForcePush => kosmos_git::force_push(root),
        },
        cx,
    );
}

fn git_error_message(error: kosmos_git::Error) -> String {
    match error {
        kosmos_git::Error::Status(message) if message.trim().is_empty() => {
            "Git command failed".to_string()
        }
        kosmos_git::Error::Status(message) => message,
        error => error.to_string(),
    }
}

fn is_missing_repository(error: &kosmos_git::Error) -> bool {
    matches!(error, kosmos_git::Error::Discover { .. })
}

fn commit_tracked<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    message: String,
    input: Entity<InputState>,
    window: &mut Window,
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

    clear_error(cx);
    input.update(cx, |input, cx| input.set_value("", window, cx));
    cx.update_global::<GitUiState, _>(|state, _| state.loading = true);
    cx.notify();

    spawn_git_action(
        root,
        move |root| kosmos_git::commit_staged(root, &message),
        move |root, cx| {
            refresh_summary(root, true, true, cx);
        },
        apply_git_action_error,
        cx,
    );
}

fn clear_error(cx: &mut App) {
    cx.update_global::<GitUiState, _>(|state, _| state.last_error = None);
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}
