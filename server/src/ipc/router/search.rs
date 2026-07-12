use core::tabs::search::SearchError;

use super::super::messages::editor::EditorDocumentPayload;
use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::search::{
    SearchDocumentParams, SearchWorkspaceParams, WorkspaceSearchResultsPayload,
};
use super::{Route, RouteDefinition, find_route, parse_params};

pub(super) const ROUTES: &[Route] = &[
    Route {
        action: "query",
        definition: RouteDefinition::external(query),
    },
    Route {
        action: "document",
        definition: RouteDefinition::external(document),
    },
];

pub(super) fn resolve(action: &str) -> Option<RouteDefinition> {
    find_route(ROUTES, action)
}

fn query(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<SearchWorkspaceParams>(request) {
        Ok(params) => match state.search_workspace(
            params.workspace_id.map(Into::into),
            params.tab_id.into(),
            &params.query,
            params.mode.into(),
        ) {
            Ok(results) => ServerMessage::ok(
                request.id,
                WorkspaceSearchResultsPayload::from_results(&results),
            ),
            Err(error) => search_error(request.id, error),
        },
        Err(response) => response,
    }
}

fn document(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<SearchDocumentParams>(request) {
        Ok(params) => match state.search_document(
            params.workspace_id.map(Into::into),
            params.tab_id.into(),
            &params.path,
        ) {
            Ok(document) => {
                ServerMessage::ok(request.id, EditorDocumentPayload::from_document(&document))
            }
            Err(error) => search_error(request.id, error),
        },
        Err(response) => response,
    }
}

fn search_error(id: u64, error: SearchError) -> ServerMessage {
    let code = match error {
        SearchError::WorkspaceNotFound => "search.workspace_not_found",
        SearchError::TabNotFound => "search.tab_not_found",
        SearchError::WorkspaceNotDirectory(_) => "search.workspace_not_directory",
        SearchError::QueryTooLong { .. } => "search.query_too_long",
        SearchError::Document(_) => "search.document_failed",
        SearchError::Io { .. } => "search.access_failed",
    };

    ServerMessage::error(id, code, error.to_string())
}
