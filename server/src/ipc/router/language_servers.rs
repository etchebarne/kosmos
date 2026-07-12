use core::language_servers::LanguageServerError;

use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::language_servers::{
    ChangeLanguageServerDocumentParams, CloseLanguageServerDocumentParams,
    ExecuteLanguageServerCommandParams, LanguageServerCodeActionPayload,
    LanguageServerCodeActionsParams, LanguageServerColorInformationPayload,
    LanguageServerColorPresentationParams, LanguageServerColorPresentationPayload,
    LanguageServerCompletionItemPayload, LanguageServerCompletionListPayload,
    LanguageServerCompletionParams, LanguageServerDiagnosticSnapshotPayload,
    LanguageServerDiagnosticsParams, LanguageServerDocumentSymbolPayload,
    LanguageServerFormattingParams, LanguageServerHoverParams, LanguageServerHoverPayload,
    LanguageServerListSnapshot, LanguageServerLocationPayload, LanguageServerParams,
    LanguageServerPositionParams, LanguageServerPrepareRenamePayload,
    LanguageServerReferencesParams, LanguageServerRenameParams, LanguageServerSignatureHelpPayload,
    LanguageServerSnapshot, LanguageServerTextEditPayload, LanguageServerWorkspaceSymbolPayload,
    LanguageServerWorkspaceSymbolsParams, OpenLanguageServerDocumentParams,
    ResolveLanguageServerCodeActionParams, ResolveLanguageServerCompletionParams,
    ResolveLanguageServerWorkspaceSymbolParams, ResolveWorkspaceEditRecoveryParams,
    ResolvedToolingCapabilitiesParams, ResolvedToolingSnapshotPayload,
    SaveLanguageServerDocumentParams, StageLanguageServerCodeActionParams,
    StagedWorkspaceEditPayload, TrustLanguageServerWorkspaceParams, WorkspaceEditRecoveryPayload,
    WorkspaceEditTransactionParams, WorkspaceEditTransactionStatusPayload,
};
use super::super::messages::{AnyJson, EmptyParams};
use super::{Route, RouteDefinition, find_route, parse_params};

pub(super) const ROUTES: &[Route] = &[
    Route::new::<EmptyParams, LanguageServerListSnapshot>("list", RouteDefinition::snapshot(list)),
    Route::new::<LanguageServerParams, LanguageServerSnapshot>(
        "status",
        RouteDefinition::snapshot(status),
    ),
    Route::new::<ResolvedToolingCapabilitiesParams, ResolvedToolingSnapshotPayload>(
        "toolingCapabilities",
        RouteDefinition::snapshot(tooling_capabilities),
    ),
    Route::new::<LanguageServerParams, LanguageServerSnapshot>(
        "install",
        RouteDefinition::live(install),
    ),
    Route::new::<LanguageServerParams, LanguageServerSnapshot>(
        "uninstall",
        RouteDefinition::live(uninstall),
    ),
    Route::new::<LanguageServerParams, LanguageServerSnapshot>(
        "restart",
        RouteDefinition::language_server(restart),
    ),
    Route::new::<OpenLanguageServerDocumentParams, bool>(
        "openDocument",
        RouteDefinition::language_server(open_document),
    ),
    Route::new::<ChangeLanguageServerDocumentParams, bool>(
        "changeDocument",
        RouteDefinition::language_server(change_document),
    ),
    Route::new::<CloseLanguageServerDocumentParams, bool>(
        "closeDocument",
        RouteDefinition::language_server(close_document),
    ),
    Route::new::<SaveLanguageServerDocumentParams, bool>(
        "saveDocument",
        RouteDefinition::language_server(save_document),
    ),
    Route::new::<LanguageServerHoverParams, Option<LanguageServerHoverPayload>>(
        "hover",
        RouteDefinition::language_server_feature(hover),
    ),
    Route::new::<LanguageServerPositionParams, Option<LanguageServerSignatureHelpPayload>>(
        "signatureHelp",
        RouteDefinition::language_server_feature(signature_help),
    ),
    Route::new::<LanguageServerPositionParams, Vec<LanguageServerLocationPayload>>(
        "definition",
        RouteDefinition::language_server_feature(definition),
    ),
    Route::new::<LanguageServerPositionParams, Vec<LanguageServerLocationPayload>>(
        "declaration",
        RouteDefinition::language_server_feature(declaration),
    ),
    Route::new::<LanguageServerPositionParams, Vec<LanguageServerLocationPayload>>(
        "typeDefinition",
        RouteDefinition::language_server_feature(type_definition),
    ),
    Route::new::<LanguageServerPositionParams, Vec<LanguageServerLocationPayload>>(
        "implementation",
        RouteDefinition::language_server_feature(implementation),
    ),
    Route::new::<LanguageServerReferencesParams, Vec<LanguageServerLocationPayload>>(
        "references",
        RouteDefinition::language_server_feature(references),
    ),
    Route::new::<LanguageServerDiagnosticsParams, Vec<LanguageServerDocumentSymbolPayload>>(
        "documentSymbols",
        RouteDefinition::language_server_feature(document_symbols),
    ),
    Route::new::<LanguageServerWorkspaceSymbolsParams, Vec<LanguageServerWorkspaceSymbolPayload>>(
        "workspaceSymbols",
        RouteDefinition::language_server_feature(workspace_symbols),
    ),
    Route::new::<ResolveLanguageServerWorkspaceSymbolParams, LanguageServerWorkspaceSymbolPayload>(
        "resolveWorkspaceSymbol",
        RouteDefinition::language_server_feature(resolve_workspace_symbol),
    ),
    Route::new::<
        LanguageServerDiagnosticsParams,
        Option<Vec<LanguageServerDiagnosticSnapshotPayload>>,
    >(
        "diagnostics",
        RouteDefinition::language_server_feature(diagnostics),
    ),
    Route::new::<LanguageServerCompletionParams, LanguageServerCompletionListPayload>(
        "completion",
        RouteDefinition::language_server_feature(completion),
    ),
    Route::new::<ResolveLanguageServerCompletionParams, LanguageServerCompletionItemPayload>(
        "resolveCompletion",
        RouteDefinition::language_server_feature(resolve_completion),
    ),
    Route::new::<LanguageServerDiagnosticsParams, Vec<LanguageServerColorInformationPayload>>(
        "documentColors",
        RouteDefinition::language_server_feature(document_colors),
    ),
    Route::new::<LanguageServerColorPresentationParams, Vec<LanguageServerColorPresentationPayload>>(
        "colorPresentations",
        RouteDefinition::language_server_feature(color_presentations),
    ),
    Route::new::<LanguageServerFormattingParams, Vec<LanguageServerTextEditPayload>>(
        "formatting",
        RouteDefinition::language_server_feature(formatting),
    ),
    Route::new::<LanguageServerPositionParams, Option<LanguageServerPrepareRenamePayload>>(
        "prepareRename",
        RouteDefinition::language_server_feature(prepare_rename),
    ),
    Route::new::<LanguageServerRenameParams, StagedWorkspaceEditPayload>(
        "rename",
        RouteDefinition::language_server_feature(rename),
    ),
    Route::new::<LanguageServerCodeActionsParams, Vec<LanguageServerCodeActionPayload>>(
        "codeActions",
        RouteDefinition::language_server_feature(code_actions),
    ),
    Route::new::<ResolveLanguageServerCodeActionParams, LanguageServerCodeActionPayload>(
        "resolveCodeAction",
        RouteDefinition::language_server_feature(resolve_code_action),
    ),
    Route::new::<StageLanguageServerCodeActionParams, Option<StagedWorkspaceEditPayload>>(
        "stageCodeAction",
        RouteDefinition::language_server_feature(stage_code_action),
    ),
    Route::new::<ExecuteLanguageServerCommandParams, AnyJson>(
        "executeCommand",
        RouteDefinition::language_server_feature(execute_command),
    ),
    Route::new::<WorkspaceEditTransactionParams, bool>(
        "applyWorkspaceEdit",
        RouteDefinition::live(apply_workspace_edit),
    ),
    Route::new::<WorkspaceEditTransactionParams, bool>(
        "commitWorkspaceEdit",
        RouteDefinition::live_full(commit_workspace_edit),
    ),
    Route::new::<WorkspaceEditTransactionParams, bool>(
        "rollbackWorkspaceEdit",
        RouteDefinition::live_full(rollback_workspace_edit),
    ),
    Route::new::<WorkspaceEditTransactionParams, bool>(
        "finishWorkspaceEdit",
        RouteDefinition::live_full(finish_workspace_edit),
    ),
    Route::new::<WorkspaceEditTransactionParams, WorkspaceEditTransactionStatusPayload>(
        "finalizeWorkspaceEdit",
        RouteDefinition::live_full(finalize_workspace_edit),
    ),
    Route::new::<WorkspaceEditTransactionParams, bool>(
        "acknowledgeWorkspaceEditCompletion",
        RouteDefinition::live_full(acknowledge_workspace_edit_completion),
    ),
    Route::new::<WorkspaceEditTransactionParams, WorkspaceEditTransactionStatusPayload>(
        "workspaceEditStatus",
        RouteDefinition::live(workspace_edit_status),
    ),
    Route::new::<ResolveWorkspaceEditRecoveryParams, WorkspaceEditTransactionStatusPayload>(
        "resolveWorkspaceEditRecovery",
        RouteDefinition::live(resolve_workspace_edit_recovery),
    ),
    Route::new::<EmptyParams, Vec<WorkspaceEditRecoveryPayload>>(
        "listWorkspaceEditRecoveries",
        RouteDefinition::live(list_workspace_edit_recoveries),
    ),
    Route::new::<TrustLanguageServerWorkspaceParams, bool>(
        "trustWorkspace",
        RouteDefinition::language_server(trust_workspace),
    ),
];

pub(super) fn resolve(action: &str) -> Option<RouteDefinition> {
    find_route(ROUTES, action)
}

fn prepare_rename(
    state: &mut core::State,
    request: &RequestEnvelope,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    let params = match parse_params::<LanguageServerPositionParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.language_server_prepare_rename(
        params.workspace_id.into(),
        &params.path,
        params.generation,
        params.version,
        params.position.into_core(),
        cancellation,
    ) {
        Ok(rename) => ServerMessage::ok(
            request.id,
            rename.map(LanguageServerPrepareRenamePayload::from_core),
        ),
        Err(error) => language_server_error(request.id, error),
    }
}

fn rename(
    state: &mut core::State,
    request: &RequestEnvelope,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    let params = match parse_params::<LanguageServerRenameParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.language_server_rename(
        params.workspace_id.into(),
        &params.path,
        params.generation,
        params.version,
        params.position.into_core(),
        &params.new_name,
        params.server_id.as_deref(),
        cancellation,
    ) {
        Ok(edit) => ServerMessage::ok(request.id, StagedWorkspaceEditPayload::from_core(edit)),
        Err(error) => language_server_error(request.id, error),
    }
}

fn code_actions(
    state: &mut core::State,
    request: &RequestEnvelope,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    let params = match parse_params::<LanguageServerCodeActionsParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.language_server_code_actions(
        params.workspace_id.into(),
        &params.path,
        params.generation,
        params.version,
        &core::language_servers::LanguageServerCodeActionRequest {
            range: params.range.into_core(),
            context: params.context.into_inner(),
        },
        cancellation,
    ) {
        Ok(actions) => ServerMessage::ok(
            request.id,
            actions
                .into_iter()
                .map(LanguageServerCodeActionPayload::from_core)
                .collect::<Vec<_>>(),
        ),
        Err(error) => language_server_error(request.id, error),
    }
}

fn resolve_code_action(
    state: &mut core::State,
    request: &RequestEnvelope,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    let params = match parse_params::<ResolveLanguageServerCodeActionParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.resolve_language_server_code_action(
        params.workspace_id.into(),
        &params.path,
        params.generation,
        params.version,
        core::language_servers::LanguageServerCodeActionResolveRequest {
            action_id: params.action_id,
            server_id: params.server_id,
            raw: params.raw.into_inner(),
        },
        cancellation,
    ) {
        Ok(action) => ServerMessage::ok(
            request.id,
            LanguageServerCodeActionPayload::from_core(action),
        ),
        Err(error) => language_server_error(request.id, error),
    }
}

fn stage_code_action(
    state: &mut core::State,
    request: &RequestEnvelope,
    _cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    let params = match parse_params::<StageLanguageServerCodeActionParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.stage_language_server_code_action(&params.action.into_core()) {
        Ok(edit) => ServerMessage::ok(request.id, edit.map(StagedWorkspaceEditPayload::from_core)),
        Err(error) => workspace_edit_error(request.id, error),
    }
}

fn execute_command(
    state: &mut core::State,
    request: &RequestEnvelope,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    let params = match parse_params::<ExecuteLanguageServerCommandParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.execute_language_server_command(
        core::language_servers::LanguageServerExecuteCommandRequest {
            workspace_id: params.workspace_id.into(),
            path: params.path,
            generation: params.generation,
            version: params.version,
            server_id: params.server_id,
            authorization: params.authorization,
        },
        cancellation,
    ) {
        Ok(result) => ServerMessage::ok(request.id, result),
        Err(error) => language_server_error(request.id, error),
    }
}

fn commit_workspace_edit(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    let params = match parse_params::<WorkspaceEditTransactionParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.commit_workspace_edit(params.transaction_id, &params.authorization) {
        Ok(()) => ServerMessage::ok(request.id, true),
        Err(error) => workspace_edit_error(request.id, error),
    }
}

fn apply_workspace_edit(_state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    // The dispatcher routes this action through the delivery hub so it never waits for Monaco
    // while holding the application mutex.
    ServerMessage::error(
        request.id,
        "workspace_edit.delivery_unavailable",
        "workspace edit delivery was not initialized",
    )
}

fn rollback_workspace_edit(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    let params = match parse_params::<WorkspaceEditTransactionParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.rollback_workspace_edit(params.transaction_id, &params.authorization) {
        Ok(()) => ServerMessage::ok(request.id, true),
        Err(error) => workspace_edit_error(request.id, error),
    }
}

fn finish_workspace_edit(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    let params = match parse_params::<WorkspaceEditTransactionParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.finish_workspace_edit(params.transaction_id, &params.authorization) {
        Ok(finished) => ServerMessage::ok(request.id, finished),
        Err(error) => workspace_edit_error(request.id, error),
    }
}

fn acknowledge_workspace_edit_completion(
    state: &mut core::State,
    request: &RequestEnvelope,
) -> ServerMessage {
    let params = match parse_params::<WorkspaceEditTransactionParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.acknowledge_workspace_edit_completion(params.transaction_id, &params.authorization)
    {
        Ok(acknowledged) => ServerMessage::ok(request.id, acknowledged),
        Err(error) => workspace_edit_error(request.id, error),
    }
}

fn finalize_workspace_edit(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    let params = match parse_params::<WorkspaceEditTransactionParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.finalize_workspace_edit(params.transaction_id, &params.authorization) {
        Ok(status) => ServerMessage::ok(
            request.id,
            WorkspaceEditTransactionStatusPayload::from_core(status),
        ),
        Err(error) => workspace_edit_error(request.id, error),
    }
}

fn workspace_edit_status(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    let params = match parse_params::<WorkspaceEditTransactionParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.workspace_edit_status(params.transaction_id, &params.authorization) {
        Ok(status) => ServerMessage::ok(
            request.id,
            WorkspaceEditTransactionStatusPayload::from_core(status),
        ),
        Err(error) => workspace_edit_error(request.id, error),
    }
}

fn resolve_workspace_edit_recovery(
    _state: &mut core::State,
    request: &RequestEnvelope,
) -> ServerMessage {
    let params = match parse_params::<ResolveWorkspaceEditRecoveryParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    let _ = (params.transaction_id, params.authorization, params.intent);
    ServerMessage::error(
        request.id,
        "workspace_edit.delivery_unavailable",
        "workspace edit recovery coordinator was not initialized",
    )
}

fn list_workspace_edit_recoveries(
    state: &mut core::State,
    request: &RequestEnvelope,
) -> ServerMessage {
    match state.workspace_edit_recoveries() {
        Ok(recoveries) => ServerMessage::ok(
            request.id,
            recoveries
                .into_iter()
                .map(WorkspaceEditRecoveryPayload::from_core)
                .collect::<Vec<_>>(),
        ),
        Err(error) => workspace_edit_error(request.id, error),
    }
}

fn workspace_edit_error(
    id: u64,
    error: core::language_servers::WorkspaceEditError,
) -> ServerMessage {
    let code = match &error {
        core::language_servers::WorkspaceEditError::Stale(_) => "workspace_edit.stale",
        core::language_servers::WorkspaceEditError::Unsupported(_) => "workspace_edit.unsupported",
        core::language_servers::WorkspaceEditError::Limit(_) => "workspace_edit.limit_exceeded",
        core::language_servers::WorkspaceEditError::Expired => "workspace_edit.expired",
        core::language_servers::WorkspaceEditError::Invalid(_) => "workspace_edit.invalid",
        core::language_servers::WorkspaceEditError::Io(_) => "workspace_edit.io_failed",
        core::language_servers::WorkspaceEditError::Recovery(_) => {
            "workspace_edit.recovery_required"
        }
    };
    ServerMessage::error(id, code, error.to_string())
}

fn signature_help(
    state: &mut core::State,
    request: &RequestEnvelope,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    let params = match parse_params::<LanguageServerPositionParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.language_server_signature_help(
        params.workspace_id.into(),
        &params.path,
        params.generation,
        params.version,
        params.position.into_core(),
        cancellation,
    ) {
        Ok(help) => ServerMessage::ok(
            request.id,
            help.map(LanguageServerSignatureHelpPayload::from_core),
        ),
        Err(error) => language_server_error(request.id, error),
    }
}

#[derive(Clone, Copy)]
enum LocationFeature {
    Definition,
    Declaration,
    TypeDefinition,
    Implementation,
}

fn definition(
    state: &mut core::State,
    request: &RequestEnvelope,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    location_request(state, request, cancellation, LocationFeature::Definition)
}

fn declaration(
    state: &mut core::State,
    request: &RequestEnvelope,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    location_request(state, request, cancellation, LocationFeature::Declaration)
}

fn type_definition(
    state: &mut core::State,
    request: &RequestEnvelope,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    location_request(
        state,
        request,
        cancellation,
        LocationFeature::TypeDefinition,
    )
}

fn implementation(
    state: &mut core::State,
    request: &RequestEnvelope,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    location_request(
        state,
        request,
        cancellation,
        LocationFeature::Implementation,
    )
}

fn location_request(
    state: &mut core::State,
    request: &RequestEnvelope,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
    feature: LocationFeature,
) -> ServerMessage {
    let params = match parse_params::<LanguageServerPositionParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    let result = match feature {
        LocationFeature::Definition => state.language_server_definition(
            params.workspace_id.into(),
            &params.path,
            params.generation,
            params.version,
            params.position.into_core(),
            cancellation,
        ),
        LocationFeature::Declaration => state.language_server_declaration(
            params.workspace_id.into(),
            &params.path,
            params.generation,
            params.version,
            params.position.into_core(),
            cancellation,
        ),
        LocationFeature::TypeDefinition => state.language_server_type_definition(
            params.workspace_id.into(),
            &params.path,
            params.generation,
            params.version,
            params.position.into_core(),
            cancellation,
        ),
        LocationFeature::Implementation => state.language_server_implementation(
            params.workspace_id.into(),
            &params.path,
            params.generation,
            params.version,
            params.position.into_core(),
            cancellation,
        ),
    };
    match result {
        Ok(locations) => ServerMessage::ok(
            request.id,
            locations
                .into_iter()
                .map(LanguageServerLocationPayload::from_core)
                .collect::<Vec<_>>(),
        ),
        Err(error) => language_server_error(request.id, error),
    }
}

fn references(
    state: &mut core::State,
    request: &RequestEnvelope,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    let params = match parse_params::<LanguageServerReferencesParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.language_server_references(
        params.workspace_id.into(),
        &params.path,
        params.generation,
        params.version,
        params.position.into_core(),
        params.include_declaration,
        cancellation,
    ) {
        Ok(locations) => ServerMessage::ok(
            request.id,
            locations
                .into_iter()
                .map(LanguageServerLocationPayload::from_core)
                .collect::<Vec<_>>(),
        ),
        Err(error) => language_server_error(request.id, error),
    }
}

fn document_symbols(
    state: &mut core::State,
    request: &RequestEnvelope,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    let params = match parse_params::<LanguageServerDiagnosticsParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.language_server_document_symbols(
        params.workspace_id.into(),
        &params.path,
        params.generation,
        params.version,
        cancellation,
    ) {
        Ok(symbols) => ServerMessage::ok(
            request.id,
            symbols
                .into_iter()
                .map(LanguageServerDocumentSymbolPayload::from_core)
                .collect::<Vec<_>>(),
        ),
        Err(error) => language_server_error(request.id, error),
    }
}

fn workspace_symbols(
    state: &mut core::State,
    request: &RequestEnvelope,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    let params = match parse_params::<LanguageServerWorkspaceSymbolsParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.language_server_workspace_symbols(&params.query, cancellation) {
        Ok(symbols) => ServerMessage::ok(
            request.id,
            symbols
                .into_iter()
                .map(LanguageServerWorkspaceSymbolPayload::from_core)
                .collect::<Vec<_>>(),
        ),
        Err(error) => language_server_error(request.id, error),
    }
}

fn resolve_workspace_symbol(
    state: &mut core::State,
    request: &RequestEnvelope,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    let params = match parse_params::<ResolveLanguageServerWorkspaceSymbolParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.resolve_language_server_workspace_symbol(
        core::language_servers::LanguageServerWorkspaceSymbolResolveRequest {
            server_id: params.server_id,
            workspace_id: params.workspace_id.into(),
            raw: params.raw.into_inner(),
        },
        cancellation,
    ) {
        Ok(symbol) => ServerMessage::ok(
            request.id,
            LanguageServerWorkspaceSymbolPayload::from_core(symbol),
        ),
        Err(error) => language_server_error(request.id, error),
    }
}

fn trust_workspace(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    let params = match parse_params::<TrustLanguageServerWorkspaceParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.trust_language_server_workspace(params.workspace_id.into()) {
        Ok(()) => ServerMessage::ok(request.id, true),
        Err(error) => language_server_error(request.id, error),
    }
}

fn open_document(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    let params = match parse_params::<OpenLanguageServerDocumentParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.open_language_server_document(
        params.workspace_id.into(),
        params.tab_id.into(),
        &params.language_id,
        params.generation,
        params.version,
        &params.text,
    ) {
        Ok(complete) => ServerMessage::ok(request.id, complete),
        Err(error) => language_server_error(request.id, error),
    }
}

fn change_document(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    let params = match parse_params::<ChangeLanguageServerDocumentParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    let changes = params
        .changes
        .into_iter()
        .map(|change| change.into_core())
        .collect::<Vec<_>>();
    match state.change_language_server_document(
        params.workspace_id.into(),
        &params.path,
        params.generation,
        params.version,
        &changes,
        &params.text,
    ) {
        Ok(()) => ServerMessage::ok(request.id, true),
        Err(error) => language_server_error(request.id, error),
    }
}

fn close_document(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    let params = match parse_params::<CloseLanguageServerDocumentParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.close_language_server_document(
        params.workspace_id.into(),
        &params.path,
        params.generation,
    ) {
        Ok(()) => ServerMessage::ok(request.id, true),
        Err(error) => language_server_error(request.id, error),
    }
}

fn save_document(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    let params = match parse_params::<SaveLanguageServerDocumentParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.save_language_server_document(
        params.workspace_id.into(),
        &params.path,
        params.generation,
        params.version,
        &params.text,
    ) {
        Ok(()) => ServerMessage::ok(request.id, true),
        Err(error) => language_server_error(request.id, error),
    }
}

fn hover(
    state: &mut core::State,
    request: &RequestEnvelope,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    let params = match parse_params::<LanguageServerHoverParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.language_server_hover(
        params.workspace_id.into(),
        &params.path,
        params.generation,
        params.version,
        params.position.into_core(),
        cancellation,
    ) {
        Ok(hover) => {
            ServerMessage::ok(request.id, hover.map(LanguageServerHoverPayload::from_core))
        }
        Err(error) => language_server_error(request.id, error),
    }
}

fn diagnostics(
    state: &mut core::State,
    request: &RequestEnvelope,
    _cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    let params = match parse_params::<LanguageServerDiagnosticsParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.language_server_diagnostics(
        params.workspace_id.into(),
        &params.path,
        params.generation,
        params.version,
    ) {
        Ok(diagnostics) => ServerMessage::ok(
            request.id,
            diagnostics.map(|diagnostics| {
                diagnostics
                    .into_iter()
                    .map(LanguageServerDiagnosticSnapshotPayload::from_core)
                    .collect::<Vec<_>>()
            }),
        ),
        Err(error) => language_server_error(request.id, error),
    }
}

fn completion(
    state: &mut core::State,
    request: &RequestEnvelope,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    let params = match parse_params::<LanguageServerCompletionParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.language_server_completion(
        params.workspace_id.into(),
        &params.path,
        params.generation,
        params.version,
        &core::language_servers::LanguageServerCompletionRequest {
            position: params.position.into_core(),
            trigger_kind: params.trigger_kind,
            trigger_character: params.trigger_character,
            filter: params.filter,
        },
        cancellation,
    ) {
        Ok(completion) => ServerMessage::ok(
            request.id,
            LanguageServerCompletionListPayload::from_core(completion),
        ),
        Err(error) => language_server_error(request.id, error),
    }
}

fn resolve_completion(
    state: &mut core::State,
    request: &RequestEnvelope,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    let params = match parse_params::<ResolveLanguageServerCompletionParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.resolve_language_server_completion(
        params.workspace_id.into(),
        &params.path,
        params.generation,
        params.version,
        core::language_servers::LanguageServerCompletionResolveRequest {
            server_id: params.server_id,
            raw: params.raw.into_inner(),
        },
        cancellation,
    ) {
        Ok(item) => ServerMessage::ok(
            request.id,
            LanguageServerCompletionItemPayload::from_core(item),
        ),
        Err(error) => language_server_error(request.id, error),
    }
}

fn document_colors(
    state: &mut core::State,
    request: &RequestEnvelope,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    let params = match parse_params::<LanguageServerDiagnosticsParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.language_server_document_colors(
        params.workspace_id.into(),
        &params.path,
        params.generation,
        params.version,
        cancellation,
    ) {
        Ok(colors) => ServerMessage::ok(
            request.id,
            colors
                .into_iter()
                .map(LanguageServerColorInformationPayload::from_core)
                .collect::<Vec<_>>(),
        ),
        Err(error) => language_server_error(request.id, error),
    }
}

fn color_presentations(
    state: &mut core::State,
    request: &RequestEnvelope,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    let params = match parse_params::<LanguageServerColorPresentationParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.language_server_color_presentations(
        params.workspace_id.into(),
        &params.path,
        params.generation,
        params.version,
        &core::language_servers::LanguageServerColorPresentationRequest {
            server_id: params.server_id,
            range: params.range.into_core(),
            color: params.color.into_core(),
        },
        cancellation,
    ) {
        Ok(presentations) => ServerMessage::ok(
            request.id,
            presentations
                .into_iter()
                .map(LanguageServerColorPresentationPayload::from_core)
                .collect::<Vec<_>>(),
        ),
        Err(error) => language_server_error(request.id, error),
    }
}

fn formatting(
    state: &mut core::State,
    request: &RequestEnvelope,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    let params = match parse_params::<LanguageServerFormattingParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.format_document(
        core::formatters::DocumentFormattingRequest {
            workspace_id: params.workspace_id.into(),
            path: &params.path,
            language_id: &params.language_id,
            generation: params.generation,
            version: params.version,
            text: &params.text,
            options: core::language_servers::LanguageServerFormattingOptions {
                tab_size: params.tab_size,
                insert_spaces: params.insert_spaces,
            },
        },
        cancellation,
    ) {
        Ok(edits) => ServerMessage::ok(
            request.id,
            edits
                .into_iter()
                .map(LanguageServerTextEditPayload::from_core)
                .collect::<Vec<_>>(),
        ),
        Err(core::formatters::FormattingError::Formatter(
            core::formatters::FormatterError::InvalidOptions(message),
        )) => ServerMessage::error(request.id, "ipc.invalid_params", message),
        Err(error) => ServerMessage::error(request.id, error.code(), error.to_string()),
    }
}

fn list(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match state.language_servers() {
        Ok(statuses) => ServerMessage::ok(request.id, LanguageServerListSnapshot::new(statuses)),
        Err(error) => language_server_error(request.id, error),
    }
}

fn tooling_capabilities(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    let params = match parse_params::<ResolvedToolingCapabilitiesParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.resolved_tooling_capabilities(&params.into_core()) {
        Ok(snapshot) => ServerMessage::ok(
            request.id,
            ResolvedToolingSnapshotPayload::from_core(snapshot),
        ),
        Err(error) => language_server_error(request.id, error),
    }
}

fn status(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    let params = match parse_params::<LanguageServerParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.language_server_status(&params.server_id) {
        Ok(status) => ServerMessage::ok(request.id, LanguageServerSnapshot::from_status(status)),
        Err(error) => language_server_error(request.id, error),
    }
}

fn install(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    let params = match parse_params::<LanguageServerParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.install_language_server(&params.server_id) {
        Ok(status) => ServerMessage::ok(request.id, LanguageServerSnapshot::from_status(status)),
        Err(error) => language_server_error(request.id, error),
    }
}

fn uninstall(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    let params = match parse_params::<LanguageServerParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.uninstall_language_server(&params.server_id) {
        Ok(status) => ServerMessage::ok(request.id, LanguageServerSnapshot::from_status(status)),
        Err(error) => language_server_error(request.id, error),
    }
}

fn restart(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    let params = match parse_params::<LanguageServerParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.restart_language_server(&params.server_id) {
        Ok(status) => ServerMessage::ok(request.id, LanguageServerSnapshot::from_status(status)),
        Err(error) => language_server_error(request.id, error),
    }
}

fn language_server_error(id: u64, error: LanguageServerError) -> ServerMessage {
    ServerMessage::error(id, error.code(), error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::messages::envelope::Domain;

    #[test]
    fn unavailable_manager_returns_a_typed_error() {
        let mut state = core::State::new();
        let response = list(
            &mut state,
            &RequestEnvelope {
                id: 7,
                domain: Domain::LanguageServers,
                action: "list".to_owned(),
                params: serde_json::Value::Null,
            },
        );
        let response = serde_json::to_value(response).expect("response should serialize");

        assert_eq!(response["id"], 7);
        assert_eq!(response["error"]["code"], "language_servers.unavailable");
    }

    #[test]
    fn tooling_capabilities_serializes_an_empty_snapshot_without_managers() {
        let mut state = core::State::new();
        let response = tooling_capabilities(
            &mut state,
            &RequestEnvelope {
                id: 11,
                domain: Domain::LanguageServers,
                action: "toolingCapabilities".to_owned(),
                params: serde_json::json!({ "documents": [] }),
            },
        );
        let response = serde_json::to_value(response).expect("response should serialize");

        assert_eq!(response["result"]["revision"], 0);
        assert_eq!(response["result"]["documents"], serde_json::json!([]));
    }

    #[test]
    fn workspace_trust_requirement_returns_a_typed_error() {
        let response = serde_json::to_value(language_server_error(
            10,
            LanguageServerError::WorkspaceNotTrusted,
        ))
        .expect("response should serialize");

        assert_eq!(
            response["error"]["code"],
            "language_servers.workspace_not_trusted"
        );
    }

    #[test]
    fn status_requires_a_server_id() {
        let mut state = core::State::new();
        let response = status(
            &mut state,
            &RequestEnvelope {
                id: 8,
                domain: Domain::LanguageServers,
                action: "status".to_owned(),
                params: serde_json::json!({}),
            },
        );
        let response = serde_json::to_value(response).expect("response should serialize");

        assert_eq!(response["error"]["code"], "ipc.invalid_params");
    }

    #[test]
    fn formatting_rejects_zero_tab_size() {
        let mut state = core::State::new();
        let cancellation = core::language_servers::LanguageServerRequestCancellation::new();
        let response = formatting(
            &mut state,
            &RequestEnvelope {
                id: 9,
                domain: Domain::LanguageServers,
                action: "formatting".to_owned(),
                params: serde_json::json!({
                    "workspaceId": 1,
                    "path": "missing.rs",
                    "languageId": "rust",
                    "generation": 1,
                    "version": 1,
                    "text": "",
                    "tabSize": 0,
                    "insertSpaces": true
                }),
            },
            &cancellation,
        );
        let response = serde_json::to_value(response).expect("response should serialize");

        assert_eq!(response["error"]["code"], "ipc.invalid_params");
    }
}
