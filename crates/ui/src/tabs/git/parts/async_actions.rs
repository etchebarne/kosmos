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

fn apply_modal_list_result_app(
    modal: GitModal,
    result: Result<ModalList, kosmos_git::Error>,
    cx: &mut App,
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
    cx.refresh_windows();
}
