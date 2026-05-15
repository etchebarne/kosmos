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
