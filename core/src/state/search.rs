use crate::tabs::editor::EditorDocument;
use crate::tabs::search::{SearchError, SearchMode, WorkspaceSearch, WorkspaceSearchResults};
use crate::tree::{TabId, WorkspaceId};

use super::State;

impl State {
    pub fn search_workspace(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        query: &str,
        mode: SearchMode,
    ) -> Result<WorkspaceSearchResults, SearchError> {
        let directory = self.search_workspace_directory(workspace_id, tab_id)?;

        WorkspaceSearch::query(directory, query, mode)
    }

    pub fn search_document(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        path: &str,
    ) -> Result<EditorDocument, SearchError> {
        let directory = self.search_workspace_directory(workspace_id, tab_id)?;

        WorkspaceSearch::document(directory, path)
    }
}
