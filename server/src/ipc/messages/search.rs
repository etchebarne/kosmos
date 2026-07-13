use core::tabs::search::{SearchMode, WorkspaceSearchResults};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ids::{TabIdParam, WorkspaceIdParam};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchWorkspaceParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) query: String,
    pub(crate) mode: SearchModeParam,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchDocumentParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) path: String,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SearchModeParam {
    Name,
    Content,
}

impl From<SearchModeParam> for SearchMode {
    fn from(mode: SearchModeParam) -> Self {
        match mode {
            SearchModeParam::Name => Self::Name,
            SearchModeParam::Content => Self::Content,
        }
    }
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceSearchResultsPayload {
    matches: Vec<SearchMatchPayload>,
    limit_reached: bool,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchMatchPayload {
    path: String,
    line_number: Option<u32>,
    preview: Option<String>,
}

impl WorkspaceSearchResultsPayload {
    pub(crate) fn from_results(results: &WorkspaceSearchResults) -> Self {
        Self {
            matches: results
                .matches()
                .iter()
                .map(|search_match| SearchMatchPayload {
                    path: search_match.path().to_owned(),
                    line_number: search_match.line_number(),
                    preview: search_match.preview().map(str::to_owned),
                })
                .collect(),
            limit_reached: results.limit_reached(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_params_use_camel_case_and_named_modes() {
        let params: SearchWorkspaceParams = serde_json::from_value(serde_json::json!({
            "workspaceId": 1,
            "tabId": 2,
            "query": "state",
            "mode": "content"
        }))
        .unwrap();

        assert!(matches!(params.mode, SearchModeParam::Content));
        assert_eq!(params.query, "state");
    }
}
