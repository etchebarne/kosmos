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
    spawn_git_action(
        root,
        action,
        move |root, cx| {
            on_success(cx);
            refresh_modal_data(root.clone(), modal, cx);
            refresh_summary(root, true, false, cx);
        },
        |error, cx| {
            cx.update_global::<GitUiState, _>(|state, _| {
                state.last_error = Some(error.to_string())
            });
            cx.notify();
        },
        cx,
    );
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
    close_menu(cx);
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

fn run_git_action_with_toast<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    success_title: &'static str,
    error_title: &'static str,
    action: impl FnOnce(PathBuf) -> Result<(), kosmos_git::Error> + Send + 'static,
    cx: &mut Context<T>,
) {
    close_menu(cx);
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

    spawn_git_action(
        root,
        move |root| kosmos_git::commit_staged(root, &message),
        move |root, cx| {
            input.update(cx, |input, cx| input.set_value("", cx));
            refresh_summary(root, true, true, cx);
        },
        apply_git_action_error,
        cx,
    );
}

fn clear_error(cx: &mut App) {
    cx.update_global::<GitUiState, _>(|state, _| state.last_error = None);
}

pub fn close_menu(cx: &mut App) -> bool {
    if cx.try_global::<GitUiState>().is_none() {
        return false;
    }
    cx.update_global::<GitUiState, _>(|state, _| {
        let had_menu = state.menu_position.is_some() || state.sync_menu_position.is_some();
        state.menu_position = None;
        state.menu_namespace = None;
        state.sync_menu_position = None;
        state.sync_menu_namespace = None;
        had_menu
    })
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}
