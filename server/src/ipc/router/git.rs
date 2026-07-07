use core::git::GitError;

use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::git::{
    CommitGitChangesParams, CreateGitBranchParams, GitPathsParams, GitRepositorySnapshotPayload,
    GitStashParams, GitStashPayload, GitTabParams, PullGitChangesParams, PushGitChangesParams,
    SwitchGitBranchParams,
};
use super::{parse_params, unsupported_action};

pub(super) fn route(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match request.action.as_str() {
        "init" => init(state, request),
        "status" => status(state, request),
        "stagePaths" => stage_paths(state, request),
        "unstagePaths" => unstage_paths(state, request),
        "stageAll" => stage_all(state, request),
        "unstageAll" => unstage_all(state, request),
        "commit" => commit(state, request),
        "switchBranch" => switch_branch(state, request),
        "trackRemoteBranch" => track_remote_branch(state, request),
        "createBranch" => create_branch(state, request),
        "deleteBranch" => delete_branch(state, request),
        "fetch" => fetch(state, request),
        "pull" => pull(state, request),
        "push" => push(state, request),
        "stash" => stash(state, request),
        "stashStaged" => stash_staged(state, request),
        "stashes" => stashes(state, request),
        "applyStash" => apply_stash(state, request),
        "dropStash" => drop_stash(state, request),
        "discardAll" => discard_all(state, request),
        "discardStaged" => discard_staged(state, request),
        _ => unsupported_action(request),
    }
}

fn init(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<GitTabParams>(request) {
        Ok(params) => command_result(
            state.init_git_repository(params.workspace_id.map(Into::into), params.tab_id.into()),
            request.id,
        ),
        Err(response) => response,
    }
}

fn status(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<GitTabParams>(request) {
        Ok(params) => {
            match state.git_status(params.workspace_id.map(Into::into), params.tab_id.into()) {
                Ok(snapshot) => ServerMessage::ok(
                    request.id,
                    GitRepositorySnapshotPayload::from_snapshot(&snapshot),
                ),
                Err(error) => git_error(request.id, error),
            }
        }
        Err(response) => response,
    }
}

fn stage_paths(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<GitPathsParams>(request) {
        Ok(params) => command_result(
            state.stage_git_paths(
                params.workspace_id.map(Into::into),
                params.tab_id.into(),
                &params.paths,
            ),
            request.id,
        ),
        Err(response) => response,
    }
}

fn unstage_paths(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<GitPathsParams>(request) {
        Ok(params) => command_result(
            state.unstage_git_paths(
                params.workspace_id.map(Into::into),
                params.tab_id.into(),
                &params.paths,
            ),
            request.id,
        ),
        Err(response) => response,
    }
}

fn stage_all(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<GitTabParams>(request) {
        Ok(params) => command_result(
            state.stage_all_git_changes(params.workspace_id.map(Into::into), params.tab_id.into()),
            request.id,
        ),
        Err(response) => response,
    }
}

fn unstage_all(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<GitTabParams>(request) {
        Ok(params) => command_result(
            state
                .unstage_all_git_changes(params.workspace_id.map(Into::into), params.tab_id.into()),
            request.id,
        ),
        Err(response) => response,
    }
}

fn commit(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<CommitGitChangesParams>(request) {
        Ok(params) => command_result(
            state.commit_git_changes(
                params.workspace_id.map(Into::into),
                params.tab_id.into(),
                &params.message,
            ),
            request.id,
        ),
        Err(response) => response,
    }
}

fn switch_branch(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<SwitchGitBranchParams>(request) {
        Ok(params) => command_result(
            state.switch_git_branch(
                params.workspace_id.map(Into::into),
                params.tab_id.into(),
                &params.branch,
            ),
            request.id,
        ),
        Err(response) => response,
    }
}

fn track_remote_branch(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<SwitchGitBranchParams>(request) {
        Ok(params) => command_result(
            state.track_git_remote_branch(
                params.workspace_id.map(Into::into),
                params.tab_id.into(),
                &params.branch,
            ),
            request.id,
        ),
        Err(response) => response,
    }
}

fn create_branch(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<CreateGitBranchParams>(request) {
        Ok(params) => command_result(
            state.create_git_branch(
                params.workspace_id.map(Into::into),
                params.tab_id.into(),
                &params.name,
                &params.start_point,
            ),
            request.id,
        ),
        Err(response) => response,
    }
}

fn delete_branch(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<SwitchGitBranchParams>(request) {
        Ok(params) => command_result(
            state.delete_git_branch(
                params.workspace_id.map(Into::into),
                params.tab_id.into(),
                &params.branch,
            ),
            request.id,
        ),
        Err(response) => response,
    }
}

fn fetch(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<GitTabParams>(request) {
        Ok(params) => command_result(
            state.fetch_git_changes(params.workspace_id.map(Into::into), params.tab_id.into()),
            request.id,
        ),
        Err(response) => response,
    }
}

fn pull(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<PullGitChangesParams>(request) {
        Ok(params) => command_result(
            state.pull_git_changes(
                params.workspace_id.map(Into::into),
                params.tab_id.into(),
                params.rebase,
            ),
            request.id,
        ),
        Err(response) => response,
    }
}

fn push(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<PushGitChangesParams>(request) {
        Ok(params) => command_result(
            state.push_git_changes(
                params.workspace_id.map(Into::into),
                params.tab_id.into(),
                params.force,
            ),
            request.id,
        ),
        Err(response) => response,
    }
}

fn stash(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<GitTabParams>(request) {
        Ok(params) => command_result(
            state.stash_git_changes(params.workspace_id.map(Into::into), params.tab_id.into()),
            request.id,
        ),
        Err(response) => response,
    }
}

fn stash_staged(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<GitTabParams>(request) {
        Ok(params) => command_result(
            state.stash_staged_git_changes(
                params.workspace_id.map(Into::into),
                params.tab_id.into(),
            ),
            request.id,
        ),
        Err(response) => response,
    }
}

fn stashes(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<GitTabParams>(request) {
        Ok(params) => {
            match state.git_stashes(params.workspace_id.map(Into::into), params.tab_id.into()) {
                Ok(stashes) => ServerMessage::ok(
                    request.id,
                    stashes
                        .iter()
                        .map(GitStashPayload::from_stash)
                        .collect::<Vec<_>>(),
                ),
                Err(error) => git_error(request.id, error),
            }
        }
        Err(response) => response,
    }
}

fn apply_stash(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<GitStashParams>(request) {
        Ok(params) => command_result(
            state.apply_git_stash(
                params.workspace_id.map(Into::into),
                params.tab_id.into(),
                &params.selector,
            ),
            request.id,
        ),
        Err(response) => response,
    }
}

fn drop_stash(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<GitStashParams>(request) {
        Ok(params) => command_result(
            state.drop_git_stash(
                params.workspace_id.map(Into::into),
                params.tab_id.into(),
                &params.selector,
            ),
            request.id,
        ),
        Err(response) => response,
    }
}

fn discard_all(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<GitTabParams>(request) {
        Ok(params) => command_result(
            state
                .discard_all_git_changes(params.workspace_id.map(Into::into), params.tab_id.into()),
            request.id,
        ),
        Err(response) => response,
    }
}

fn discard_staged(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<GitTabParams>(request) {
        Ok(params) => command_result(
            state.discard_staged_git_changes(
                params.workspace_id.map(Into::into),
                params.tab_id.into(),
            ),
            request.id,
        ),
        Err(response) => response,
    }
}

fn command_result(result: Result<(), GitError>, id: u64) -> ServerMessage {
    match result {
        Ok(()) => ServerMessage::ok(id, true),
        Err(error) => git_error(id, error),
    }
}

fn git_error(id: u64, error: GitError) -> ServerMessage {
    ServerMessage::error(id, git_error_code(&error), error.to_string())
}

fn git_error_code(error: &GitError) -> &'static str {
    match error {
        GitError::WorkspaceNotFound => "git.workspace_not_found",
        GitError::TabNotFound => "git.tab_not_found",
        GitError::Discover { .. } => "git.repository_not_found",
        GitError::NotWorktree(_) => "git.not_worktree",
        GitError::InvalidPath(_) => "git.invalid_path",
        GitError::InvalidStash(_) => "git.invalid_stash",
        GitError::CommitMessageRequired => "git.commit_message_required",
        GitError::BranchRequired => "git.branch_required",
        GitError::CommandFailed { .. } => "git.command_failed",
        GitError::Io(_) => "git.io_failed",
        GitError::Utf8(_) => "git.invalid_utf8",
    }
}
