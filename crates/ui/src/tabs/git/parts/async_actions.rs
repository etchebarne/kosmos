fn apply_modal_list_result<T: PaneDelegate + SettingsDelegate>(
    modal: GitModal,
    result: Result<ModalList, kosmos_git::Error>,
    cx: &mut Context<T>,
) {
    apply_modal_list_result_state(modal, result, cx);
    cx.notify();
}

fn apply_modal_list_result_app(
    modal: GitModal,
    result: Result<ModalList, kosmos_git::Error>,
    cx: &mut App,
) {
    apply_modal_list_result_state(modal, result, cx);
    cx.refresh_windows();
}

fn apply_modal_list_result_state(
    modal: GitModal,
    result: Result<ModalList, kosmos_git::Error>,
    cx: &mut App,
) {
    match result {
        Ok(ModalList::Branches(branches)) if modal == GitModal::Branches => {
            cx.update_global::<GitUiState, _>(|state, _| {
                state.branches = branches;
                state.last_error = None;
            });
        }
        Ok(ModalList::Remotes(remotes)) if modal == GitModal::Remotes => {
            cx.update_global::<GitUiState, _>(|state, _| {
                state.remotes = remotes;
                state.last_error = None;
            });
        }
        Ok(ModalList::Stashes(stashes)) if modal == GitModal::Stashes => {
            cx.update_global::<GitUiState, _>(|state, _| {
                state.stashes = stashes;
                state.last_error = None;
            });
        }
        Ok(ModalList::Tags(tags)) if modal == GitModal::Tags => {
            cx.update_global::<GitUiState, _>(|state, _| {
                state.tags = tags;
                state.last_error = None;
            });
        }
        Ok(_) => {}
        Err(error) => {
            cx.update_global::<GitUiState, _>(|state, _| state.last_error = None);
            show_git_error(cx, modal_load_error_title(modal), git_error_message(error));
        }
    }
}

fn modal_load_error_title(modal: GitModal) -> &'static str {
    match modal {
        GitModal::Branches => "Failed to load branches",
        GitModal::Remotes => "Failed to load remotes",
        GitModal::Stashes => "Failed to load stashes",
        GitModal::Tags => "Failed to load tags",
        _ => "Git operation failed",
    }
}
