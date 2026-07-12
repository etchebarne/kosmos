use core::tabs::git::GitError;

use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::git::{
    AddGitRemoteParams, CommitGitChangesParams, CreateGitBranchParams, GitDiffPayload,
    GitPathsParams, GitRemoteParams, GitRemotePayload, GitRepositorySnapshotPayload,
    GitStashParams, GitStashPayload, GitTabParams, GitTagParams, GitTagPayload,
    OpenGitDiffTabParams, PullGitChangesParams, PushGitChangesParams, SaveGitDiffFileParams,
    SwitchGitBranchParams,
};
use super::{Route, RouteDefinition, find_route, parse_params, workspace_list_response};

pub(super) const ROUTES: &[Route] = &[
    Route {
        action: "init",
        definition: RouteDefinition::external(init),
    },
    Route {
        action: "status",
        definition: RouteDefinition::external(status),
    },
    Route {
        action: "openDiffTab",
        definition: RouteDefinition::full(open_diff_tab),
    },
    Route {
        action: "diff",
        definition: RouteDefinition::external(diff),
    },
    Route {
        action: "saveDiffFile",
        definition: RouteDefinition::external(save_diff_file),
    },
    Route {
        action: "stagePaths",
        definition: RouteDefinition::external(stage_paths),
    },
    Route {
        action: "unstagePaths",
        definition: RouteDefinition::external(unstage_paths),
    },
    Route {
        action: "stageAll",
        definition: RouteDefinition::external(stage_all),
    },
    Route {
        action: "unstageAll",
        definition: RouteDefinition::external(unstage_all),
    },
    Route {
        action: "commit",
        definition: RouteDefinition::external(commit),
    },
    Route {
        action: "switchBranch",
        definition: RouteDefinition::external(switch_branch),
    },
    Route {
        action: "trackRemoteBranch",
        definition: RouteDefinition::external(track_remote_branch),
    },
    Route {
        action: "createBranch",
        definition: RouteDefinition::external(create_branch),
    },
    Route {
        action: "deleteBranch",
        definition: RouteDefinition::external(delete_branch),
    },
    Route {
        action: "fetch",
        definition: RouteDefinition::external(fetch),
    },
    Route {
        action: "pull",
        definition: RouteDefinition::external(pull),
    },
    Route {
        action: "push",
        definition: RouteDefinition::external(push),
    },
    Route {
        action: "stash",
        definition: RouteDefinition::external(stash),
    },
    Route {
        action: "stashStaged",
        definition: RouteDefinition::external(stash_staged),
    },
    Route {
        action: "stashes",
        definition: RouteDefinition::external(stashes),
    },
    Route {
        action: "applyStash",
        definition: RouteDefinition::external(apply_stash),
    },
    Route {
        action: "dropStash",
        definition: RouteDefinition::external(drop_stash),
    },
    Route {
        action: "remotes",
        definition: RouteDefinition::external(remotes),
    },
    Route {
        action: "addRemote",
        definition: RouteDefinition::external(add_remote),
    },
    Route {
        action: "removeRemote",
        definition: RouteDefinition::external(remove_remote),
    },
    Route {
        action: "tags",
        definition: RouteDefinition::external(tags),
    },
    Route {
        action: "createTag",
        definition: RouteDefinition::external(create_tag),
    },
    Route {
        action: "deleteTag",
        definition: RouteDefinition::external(delete_tag),
    },
    Route {
        action: "discardAll",
        definition: RouteDefinition::external(discard_all),
    },
    Route {
        action: "discardStaged",
        definition: RouteDefinition::external(discard_staged),
    },
];

pub(super) fn resolve(action: &str) -> Option<RouteDefinition> {
    find_route(ROUTES, action)
}

fn open_diff_tab(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<OpenGitDiffTabParams>(request) {
        Ok(params) => match state.open_git_diff_tab(
            params.workspace_id.map(Into::into),
            params.tab_id.into(),
            &params.path,
        ) {
            Ok(()) => workspace_list_response(request.id, state),
            Err(error) => git_error(request.id, error),
        },
        Err(response) => response,
    }
}

fn diff(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<GitTabParams>(request) {
        Ok(params) => {
            match state.git_diff(params.workspace_id.map(Into::into), params.tab_id.into()) {
                Ok(diff) => ServerMessage::ok(request.id, GitDiffPayload::from_diff(&diff)),
                Err(error) => git_error(request.id, error),
            }
        }
        Err(response) => response,
    }
}

fn save_diff_file(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<SaveGitDiffFileParams>(request) {
        Ok(params) => command_result(
            state.save_git_diff_file(
                params.workspace_id.map(Into::into),
                params.tab_id.into(),
                &params.path,
                &params.content,
                params.stage,
            ),
            request.id,
        ),
        Err(response) => response,
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

fn remotes(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<GitTabParams>(request) {
        Ok(params) => {
            match state.git_remotes(params.workspace_id.map(Into::into), params.tab_id.into()) {
                Ok(remotes) => ServerMessage::ok(
                    request.id,
                    remotes
                        .iter()
                        .map(GitRemotePayload::from_remote)
                        .collect::<Vec<_>>(),
                ),
                Err(error) => git_error(request.id, error),
            }
        }
        Err(response) => response,
    }
}

fn add_remote(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<AddGitRemoteParams>(request) {
        Ok(params) => command_result(
            state.add_git_remote(
                params.workspace_id.map(Into::into),
                params.tab_id.into(),
                &params.name,
                &params.url,
            ),
            request.id,
        ),
        Err(response) => response,
    }
}

fn remove_remote(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<GitRemoteParams>(request) {
        Ok(params) => command_result(
            state.remove_git_remote(
                params.workspace_id.map(Into::into),
                params.tab_id.into(),
                &params.name,
            ),
            request.id,
        ),
        Err(response) => response,
    }
}

fn tags(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<GitTabParams>(request) {
        Ok(params) => {
            match state.git_tags(params.workspace_id.map(Into::into), params.tab_id.into()) {
                Ok(tags) => ServerMessage::ok(
                    request.id,
                    tags.iter().map(GitTagPayload::from_tag).collect::<Vec<_>>(),
                ),
                Err(error) => git_error(request.id, error),
            }
        }
        Err(response) => response,
    }
}

fn create_tag(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<GitTagParams>(request) {
        Ok(params) => command_result(
            state.create_git_tag(
                params.workspace_id.map(Into::into),
                params.tab_id.into(),
                &params.name,
            ),
            request.id,
        ),
        Err(response) => response,
    }
}

fn delete_tag(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<GitTagParams>(request) {
        Ok(params) => command_result(
            state.delete_git_tag(
                params.workspace_id.map(Into::into),
                params.tab_id.into(),
                &params.name,
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
        GitError::File(_) => "git.diff_file_failed",
        GitError::CommitMessageRequired => "git.commit_message_required",
        GitError::BranchRequired => "git.branch_required",
        GitError::RemoteNameRequired => "git.remote_name_required",
        GitError::RemoteUrlRequired => "git.remote_url_required",
        GitError::TagNameRequired => "git.tag_name_required",
        GitError::CommandFailed { .. } => "git.command_failed",
        GitError::Io(_) => "git.io_failed",
        GitError::Utf8(_) => "git.invalid_utf8",
    }
}
