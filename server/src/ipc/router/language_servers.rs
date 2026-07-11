use core::language_servers::LanguageServerError;

use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::language_servers::{
    ChangeLanguageServerDocumentParams, CloseLanguageServerDocumentParams,
    LanguageServerColorInformationPayload, LanguageServerColorPresentationParams,
    LanguageServerColorPresentationPayload, LanguageServerCompletionItemPayload,
    LanguageServerCompletionListPayload, LanguageServerCompletionParams,
    LanguageServerDiagnosticPayload, LanguageServerDiagnosticsParams,
    LanguageServerFormattingParams, LanguageServerHoverParams, LanguageServerHoverPayload,
    LanguageServerListSnapshot, LanguageServerParams, LanguageServerSnapshot,
    LanguageServerTextEditPayload, OpenLanguageServerDocumentParams,
    ResolveLanguageServerCompletionParams, SaveLanguageServerDocumentParams,
    TrustLanguageServerWorkspaceParams,
};
use super::{RouteDefinition, parse_params};

pub(super) fn resolve(action: &str) -> Option<RouteDefinition> {
    match action {
        "list" => Some(RouteDefinition::snapshot(list)),
        "status" => Some(RouteDefinition::snapshot(status)),
        "install" => Some(RouteDefinition::live(install)),
        "uninstall" => Some(RouteDefinition::live(uninstall)),
        "restart" => Some(RouteDefinition::language_server(restart)),
        "openDocument" => Some(RouteDefinition::language_server(open_document)),
        "changeDocument" => Some(RouteDefinition::language_server(change_document)),
        "closeDocument" => Some(RouteDefinition::language_server(close_document)),
        "saveDocument" => Some(RouteDefinition::language_server(save_document)),
        "hover" => Some(RouteDefinition::language_server_feature(hover)),
        "diagnostics" => Some(RouteDefinition::language_server_feature(diagnostics)),
        "completion" => Some(RouteDefinition::language_server_feature(completion)),
        "resolveCompletion" => Some(RouteDefinition::language_server_feature(resolve_completion)),
        "documentColors" => Some(RouteDefinition::language_server_feature(document_colors)),
        "colorPresentations" => Some(RouteDefinition::language_server_feature(
            color_presentations,
        )),
        "formatting" => Some(RouteDefinition::language_server_feature(formatting)),
        "trustWorkspace" => Some(RouteDefinition::language_server(trust_workspace)),
        _ => None,
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

fn hover(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
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
    ) {
        Ok(hover) => {
            ServerMessage::ok(request.id, hover.map(LanguageServerHoverPayload::from_core))
        }
        Err(error) => language_server_error(request.id, error),
    }
}

fn diagnostics(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
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
                    .map(LanguageServerDiagnosticPayload::from_core)
                    .collect::<Vec<_>>()
            }),
        ),
        Err(error) => language_server_error(request.id, error),
    }
}

fn completion(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
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
    ) {
        Ok(completion) => ServerMessage::ok(
            request.id,
            LanguageServerCompletionListPayload::from_core(completion),
        ),
        Err(error) => language_server_error(request.id, error),
    }
}

fn resolve_completion(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    let params = match parse_params::<ResolveLanguageServerCompletionParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.resolve_language_server_completion(
        params.workspace_id.into(),
        &params.path,
        params.generation,
        params.version,
        &params.server_id,
        params.raw,
    ) {
        Ok(item) => ServerMessage::ok(
            request.id,
            LanguageServerCompletionItemPayload::from_core(item),
        ),
        Err(error) => language_server_error(request.id, error),
    }
}

fn document_colors(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    let params = match parse_params::<LanguageServerDiagnosticsParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.language_server_document_colors(
        params.workspace_id.into(),
        &params.path,
        params.generation,
        params.version,
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

fn color_presentations(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
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

fn formatting(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    let params = match parse_params::<LanguageServerFormattingParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    if params.tab_size == 0 {
        return ServerMessage::error(
            request.id,
            "ipc.invalid_params",
            "formatting tab size must be greater than zero",
        );
    }
    match state.format_document(core::formatters::DocumentFormattingRequest {
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
    }) {
        Ok(edits) => ServerMessage::ok(
            request.id,
            edits
                .into_iter()
                .map(LanguageServerTextEditPayload::from_core)
                .collect::<Vec<_>>(),
        ),
        Err(error) => ServerMessage::error(request.id, error.code(), error.to_string()),
    }
}

fn list(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match state.language_servers() {
        Ok(statuses) => ServerMessage::ok(request.id, LanguageServerListSnapshot::new(statuses)),
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
}
