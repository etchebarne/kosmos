use crate::language_servers::{
    LanguageServerError, StagedWorkspaceEdit, StagedWorkspaceEditOperation, WorkspaceEditError,
    WorkspaceEditRoot,
};
use crate::tabs::editor::EditorViewState;
use crate::tree::{TabId, TabKind, WorkspaceId};

use super::{
    State, WorkspaceEditEditorRecovery, path_is_at_or_below, remap_workspace_path,
    tab_pane_id_in_workspace_list, tab_title_in_node,
};

impl State {
    pub fn commit_workspace_edit(
        &mut self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<(), WorkspaceEditError> {
        let manager = self.language_server_manager.as_ref().ok_or_else(|| {
            WorkspaceEditError::Invalid("language server manager is unavailable".to_owned())
        })?;
        let operations = manager.workspace_edit_operations(transaction_id, authorization)?;
        manager.commit_workspace_edit(transaction_id, authorization)?;
        self.reconcile_workspace_edit_resources(transaction_id, &operations, false);
        Ok(())
    }

    pub fn commit_workspace_edit_with_open_documents(
        &mut self,
        transaction_id: u64,
        authorization: &str,
        documents: &[crate::language_servers::WorkspaceEditOpenDocument],
    ) -> Result<(), WorkspaceEditError> {
        let manager = self.language_server_manager.as_ref().ok_or_else(|| {
            WorkspaceEditError::Invalid("language server manager is unavailable".to_owned())
        })?;
        let operations = manager.workspace_edit_operations(transaction_id, authorization)?;
        manager.commit_workspace_edit_with_documents(transaction_id, authorization, documents)?;
        self.reconcile_workspace_edit_resources(transaction_id, &operations, false);
        Ok(())
    }

    pub(crate) fn staged_workspace_edit(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<StagedWorkspaceEdit, WorkspaceEditError> {
        self.language_server_manager
            .as_ref()
            .ok_or_else(|| {
                WorkspaceEditError::Invalid("language server manager is unavailable".to_owned())
            })?
            .staged_workspace_edit(transaction_id, authorization)
    }

    pub(crate) fn workspace_edit_model_directives(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<Vec<crate::language_servers::WorkspaceEditModelDirective>, WorkspaceEditError> {
        self.language_server_manager
            .as_ref()
            .ok_or_else(|| {
                WorkspaceEditError::Invalid("language server manager is unavailable".to_owned())
            })?
            .workspace_edit_model_directives(transaction_id, authorization)
    }

    pub fn rollback_workspace_edit(
        &mut self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<(), WorkspaceEditError> {
        let manager = self.language_server_manager.as_ref().ok_or_else(|| {
            WorkspaceEditError::Invalid("language server manager is unavailable".to_owned())
        })?;
        let operations = manager.workspace_edit_operations(transaction_id, authorization)?;
        manager.rollback_workspace_edit(transaction_id, authorization)?;
        self.reconcile_workspace_edit_resources(transaction_id, &operations, true);
        Ok(())
    }

    pub(super) fn reconcile_workspace_edit_resources(
        &mut self,
        transaction_id: u64,
        operations: &[StagedWorkspaceEditOperation],
        reverse: bool,
    ) {
        if reverse {
            if self.restore_workspace_edit_editor_recovery(transaction_id) {
                self.mark_persistent_change();
            }
            return;
        }
        let mut changed = false;
        for operation in operations {
            if let StagedWorkspaceEditOperation::CreateFile {
                workspace_id, path, ..
            }
            | StagedWorkspaceEditOperation::DeleteFile {
                workspace_id, path, ..
            } = operation
            {
                changed |= self.remove_deleted_editor_states(transaction_id, *workspace_id, path);
                continue;
            }
            let StagedWorkspaceEditOperation::RenameFile {
                workspace_id,
                old_path,
                new_path,
            } = operation
            else {
                continue;
            };
            changed |= self.remove_deleted_editor_states(transaction_id, *workspace_id, new_path);
            let renamed = self
                .editor_view_states
                .iter()
                .filter(|state| state.workspace_id() == *workspace_id)
                .filter_map(|state| {
                    remap_workspace_path(state.path(), old_path, new_path)
                        .map(|path| (state.clone(), path))
                })
                .collect::<Vec<_>>();
            for (state, path) in renamed {
                let title = self
                    .tab_title(state.workspace_id(), state.tab_id())
                    .unwrap_or_else(|| {
                        state
                            .path()
                            .rsplit('/')
                            .next()
                            .unwrap_or(state.path())
                            .to_owned()
                    });
                self.record_workspace_edit_editor_recovery(transaction_id, state.clone(), title);
                if let Some(current) = self.editor_view_states.iter_mut().find(|current| {
                    current.workspace_id() == state.workspace_id()
                        && current.tab_id() == state.tab_id()
                }) {
                    current.set_path(path.clone());
                }
                self.update_workspace_edit_editor_recovery(
                    transaction_id,
                    state.workspace_id(),
                    state.tab_id(),
                    path.clone(),
                    true,
                );
                self.set_editor_tab_state(
                    state.workspace_id(),
                    state.tab_id(),
                    TabKind::Editor,
                    path.rsplit('/').next().unwrap_or(&path),
                );
                changed = true;
            }
        }
        if !changed {
            return;
        }
        self.mark_persistent_change();
    }

    fn remove_deleted_editor_states(
        &mut self,
        transaction_id: u64,
        workspace_id: WorkspaceId,
        path: &str,
    ) -> bool {
        let removed = self
            .editor_view_states
            .iter()
            .filter(|state| {
                state.workspace_id() == workspace_id && path_is_at_or_below(state.path(), path)
            })
            .cloned()
            .collect::<Vec<_>>();
        if removed.is_empty() {
            return false;
        }
        for state in &removed {
            let title = self
                .tab_title(workspace_id, state.tab_id())
                .unwrap_or_else(|| {
                    state
                        .path()
                        .rsplit('/')
                        .next()
                        .unwrap_or(state.path())
                        .to_owned()
                });
            self.record_workspace_edit_editor_recovery(transaction_id, state.clone(), title);
            self.update_workspace_edit_editor_recovery(
                transaction_id,
                workspace_id,
                state.tab_id(),
                state.path().to_owned(),
                false,
            );
            self.set_editor_tab_state(workspace_id, state.tab_id(), TabKind::Blank, "Blank");
        }
        self.editor_view_states
            .retain(|state| !removed.contains(state));
        true
    }

    fn record_workspace_edit_editor_recovery(
        &mut self,
        transaction_id: u64,
        state: EditorViewState,
        original_title: String,
    ) {
        let states = self
            .workspace_edit_editor_recovery
            .entry(transaction_id)
            .or_default();
        if states.iter().any(|current| {
            current.original.workspace_id() == state.workspace_id()
                && current.original.tab_id() == state.tab_id()
        }) {
            return;
        }
        states.push(WorkspaceEditEditorRecovery {
            virtual_path: state.path().to_owned(),
            original: state,
            original_title,
            present: true,
        });
    }

    fn update_workspace_edit_editor_recovery(
        &mut self,
        transaction_id: u64,
        workspace_id: WorkspaceId,
        tab_id: TabId,
        virtual_path: String,
        present: bool,
    ) {
        let Some(recovery) = self
            .workspace_edit_editor_recovery
            .get_mut(&transaction_id)
            .and_then(|states| {
                states.iter_mut().find(|state| {
                    state.original.workspace_id() == workspace_id
                        && state.original.tab_id() == tab_id
                })
            })
        else {
            return;
        };
        recovery.virtual_path = virtual_path;
        recovery.present = present;
    }

    fn restore_workspace_edit_editor_recovery(&mut self, transaction_id: u64) -> bool {
        let Some(recoveries) = self.workspace_edit_editor_recovery.remove(&transaction_id) else {
            return false;
        };
        for recovery in recoveries {
            let workspace_id = recovery.original.workspace_id();
            let tab_id = recovery.original.tab_id();
            self.editor_view_states
                .retain(|state| state.workspace_id() != workspace_id || state.tab_id() != tab_id);
            self.editor_view_states.push(recovery.original);
            self.set_editor_tab_state(
                workspace_id,
                tab_id,
                TabKind::Editor,
                &recovery.original_title,
            );
        }
        true
    }

    fn tab_title(&self, workspace_id: WorkspaceId, tab_id: TabId) -> Option<String> {
        let workspace = self.workspaces.workspace(workspace_id)?;
        tab_title_in_node(workspace.root(), tab_id).map(str::to_owned)
    }

    fn set_editor_tab_state(
        &mut self,
        workspace_id: WorkspaceId,
        tab_id: TabId,
        kind: TabKind,
        title: &str,
    ) {
        let Some(pane_id) = tab_pane_id_in_workspace_list(&self.workspaces, workspace_id, tab_id)
        else {
            return;
        };
        if let Some(workspace) = self.workspace_mut(workspace_id)
            && let Some(pane) = workspace.root_mut().find_pane_mut(pane_id)
        {
            pane.set_tab_kind(tab_id, kind);
            pane.rename_tab(tab_id, title);
        }
    }

    pub fn finish_workspace_edit(
        &mut self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<bool, WorkspaceEditError> {
        let finished = self
            .language_server_manager
            .as_ref()
            .ok_or_else(|| {
                WorkspaceEditError::Invalid("language server manager is unavailable".to_owned())
            })?
            .finish_workspace_edit(transaction_id, authorization)?;
        if self
            .workspace_edit_editor_recovery
            .remove(&transaction_id)
            .is_some()
        {
            self.mark_persistent_change();
        }
        Ok(finished)
    }

    pub fn acknowledge_workspace_edit_completion(
        &mut self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<bool, WorkspaceEditError> {
        let acknowledged = self
            .language_server_manager
            .as_ref()
            .ok_or_else(|| {
                WorkspaceEditError::Invalid("language server manager is unavailable".to_owned())
            })?
            .acknowledge_workspace_edit_completion(transaction_id, authorization)?;
        if self
            .workspace_edit_editor_recovery
            .remove(&transaction_id)
            .is_some()
        {
            self.mark_persistent_change();
        }
        Ok(acknowledged)
    }

    pub fn finalize_workspace_edit(
        &mut self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<crate::language_servers::WorkspaceEditTransactionStatus, WorkspaceEditError> {
        let manager = self.language_server_manager.as_ref().ok_or_else(|| {
            WorkspaceEditError::Invalid("language server manager is unavailable".to_owned())
        })?;
        let operations = manager.workspace_edit_operations(transaction_id, authorization)?;
        let status = manager.finalize_workspace_edit(transaction_id, authorization)?;
        match status.phase {
            crate::language_servers::WorkspaceEditTransactionPhase::FinishedCommitted => {
                self.reconcile_workspace_edit_resources(transaction_id, &operations, false);
                if self
                    .workspace_edit_editor_recovery
                    .remove(&transaction_id)
                    .is_some()
                {
                    self.mark_persistent_change();
                }
            }
            crate::language_servers::WorkspaceEditTransactionPhase::FinishedRolledBack => {
                self.reconcile_workspace_edit_resources(transaction_id, &operations, true);
            }
            _ => {}
        }
        Ok(status)
    }

    pub fn workspace_edit_status(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<crate::language_servers::WorkspaceEditTransactionStatus, WorkspaceEditError> {
        self.language_server_manager
            .as_ref()
            .ok_or_else(|| {
                WorkspaceEditError::Invalid("language server manager is unavailable".to_owned())
            })?
            .workspace_edit_status(transaction_id, authorization)
    }

    pub fn workspace_edit_recoveries(
        &self,
    ) -> Result<Vec<crate::language_servers::WorkspaceEditRecovery>, WorkspaceEditError> {
        Ok(self
            .language_server_manager
            .as_ref()
            .ok_or_else(|| {
                WorkspaceEditError::Invalid("language server manager is unavailable".to_owned())
            })?
            .workspace_edit_recoveries())
    }

    pub(super) fn workspace_edit_roots(
        &self,
    ) -> Result<Vec<WorkspaceEditRoot>, LanguageServerError> {
        self.workspaces
            .workspaces()
            .iter()
            .map(|workspace| {
                Ok(WorkspaceEditRoot {
                    workspace_id: workspace.id(),
                    path: std::fs::canonicalize(workspace.directory())
                        .map_err(|error| LanguageServerError::InvalidDocument(error.to_string()))?,
                })
            })
            .collect()
    }

    pub(super) fn workspace_edit_root(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<WorkspaceEditRoot, LanguageServerError> {
        let workspace = self.workspaces.workspace(workspace_id).ok_or_else(|| {
            LanguageServerError::InvalidDocument("workspace does not exist".to_owned())
        })?;
        Ok(WorkspaceEditRoot {
            workspace_id,
            path: std::fs::canonicalize(workspace.directory())
                .map_err(|error| LanguageServerError::InvalidDocument(error.to_string()))?,
        })
    }
}
