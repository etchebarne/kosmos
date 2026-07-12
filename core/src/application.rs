use std::collections::HashMap;
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use crate::State;
use crate::editor_sessions::{
    EditorSessionError, EditorSessionId, EditorSessionRegistry, EditorSessionSnapshot,
    EditorSessionUpdate,
};
use crate::events::CoreEventSink;
use crate::language_servers::{
    StagedWorkspaceEdit, WorkspaceEditCoordinator, WorkspaceEditDeliveryOutcome,
    WorkspaceEditDeliveryStep, WorkspaceEditError, WorkspaceEditRecoveryIntent,
    WorkspaceEditTransactionStatus,
};
use crate::persistence::{PersistenceError, StateStore};
use crate::settings::{SettingValue, SettingsError};
use crate::state::{FileTreeGitDecorationsError, OpenEditorLocation, PersistentStateCandidate};
use crate::tabs::editor::EditorError;
use crate::tabs::git::{FileTreeGitDecorations, GitError, GitLineHunk};
use crate::tree::{PaneId, TabId, WorkspaceId};
use crate::window::WindowState;

/// Owns the mutable application state and its durable backing store.
///
/// Callers prepare work while holding their application mutex, run the prepared
/// operation without that mutex, and complete it after durable storage succeeds.
pub struct Application {
    state: State,
    store: StateStore,
    durability_lease_active: bool,
    workspace_edit_coordinator: WorkspaceEditCoordinator,
    editor_sessions: EditorSessionRegistry,
    workspace_edit_session_recovery:
        HashMap<crate::language_servers::WorkspaceEditDeliveryLease, EditorSessionRegistry>,
    pending_closes: Vec<PendingClose>,
    next_close_id: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseTarget {
    Tab {
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        tab_id: TabId,
    },
    Workspace {
        workspace_id: WorkspaceId,
    },
    Application,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CloseIntent {
    pub target: CloseTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CloseIntentResult {
    Completed,
    RequiresDocumentDecision {
        close_id: u64,
        documents: Vec<EditorSessionSnapshot>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseDocumentDecision {
    Save,
    Discard,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CloseDocumentDecisionRequest {
    pub id: EditorSessionId,
    pub revision: u64,
    pub decision: CloseDocumentDecision,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CloseDecision {
    Cancel {
        close_id: u64,
    },
    Resolve {
        close_id: u64,
        documents: Vec<CloseDocumentDecisionRequest>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingClose {
    id: u64,
    target: CloseTarget,
}

pub struct PreparedPersistentOperation {
    candidate: PersistentStateCandidate,
    store: StateStore,
}

pub struct PreparedExternalOperation {
    state: State,
}

#[derive(Debug)]
pub enum ApplicationError {
    DurabilityInFlight,
    Persistence(PersistenceError),
    StalePreparedOperation,
    Editor(EditorError),
    EditorSession(EditorSessionError),
    CloseNotFound,
    InvalidCloseDecision,
}

impl Application {
    pub fn new(state: State, store: StateStore) -> Self {
        Self {
            state,
            store,
            durability_lease_active: false,
            workspace_edit_coordinator: WorkspaceEditCoordinator::default(),
            editor_sessions: EditorSessionRegistry::default(),
            workspace_edit_session_recovery: HashMap::new(),
            pending_closes: Vec::new(),
            next_close_id: 1,
        }
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut State {
        &mut self.state
    }

    pub fn set_event_sink(&self, sink: Arc<dyn CoreEventSink>) {
        self.state.set_event_sink(sink);
    }

    pub fn file_tree_git_decorations(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<FileTreeGitDecorations, FileTreeGitDecorationsError> {
        self.state.file_tree_git_decorations(workspace_id, tab_id)
    }

    pub fn editor_git_line_hunks(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<Vec<GitLineHunk>, GitError> {
        self.state.editor_git_line_hunks(workspace_id, tab_id)
    }

    pub fn prepare_persistent_operation(
        &mut self,
    ) -> Result<PreparedPersistentOperation, ApplicationError> {
        if self.durability_lease_active {
            return Err(ApplicationError::DurabilityInFlight);
        }

        self.durability_lease_active = true;
        Ok(PreparedPersistentOperation {
            candidate: self.state.persistent_candidate(),
            store: self.store.clone(),
        })
    }

    pub fn prepare_external_operation(&self) -> PreparedExternalOperation {
        PreparedExternalOperation {
            state: self.state.persistent_candidate().into_state(),
        }
    }

    pub fn complete_persistent_operation(
        &mut self,
        operation: PreparedPersistentOperation,
    ) -> Result<(), ApplicationError> {
        let committed = self.state.commit_persistent_candidate(operation.candidate);
        self.durability_lease_active = false;
        committed
            .then_some(())
            .ok_or(ApplicationError::StalePreparedOperation)
    }

    pub fn abandon_persistent_operation(&mut self) {
        self.durability_lease_active = false;
    }

    pub fn update_setting(&mut self, id: &str, value: SettingValue) -> Result<(), SettingsError> {
        self.state.update_setting(id, value)
    }

    pub fn update_window_state(&mut self, window_state: WindowState) {
        self.state.update_window_state(window_state);
    }

    pub fn open_editor_session(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        path: &str,
        content: String,
        revision: u64,
    ) -> Result<EditorSessionUpdate, ApplicationError> {
        let (workspace_id, expected_path) =
            self.state.editor_session_target(workspace_id, tab_id)?;
        if expected_path != path {
            return Err(EditorSessionError::PathMismatch {
                expected: expected_path,
                received: path.to_owned(),
            }
            .into());
        }
        self.editor_sessions
            .open(
                EditorSessionId::new(workspace_id, tab_id),
                path,
                content,
                revision,
            )
            .map_err(ApplicationError::from)
    }

    pub fn change_editor_session(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        content: String,
        revision: u64,
    ) -> Result<EditorSessionUpdate, ApplicationError> {
        let (workspace_id, _) = self.state.editor_session_target(workspace_id, tab_id)?;
        self.editor_sessions
            .change(
                EditorSessionId::new(workspace_id, tab_id),
                content,
                revision,
            )
            .map_err(ApplicationError::from)
    }

    pub fn editor_session_document(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<EditorSessionSnapshot, ApplicationError> {
        let (workspace_id, path) = self.state.editor_session_target(workspace_id, tab_id)?;
        if let Some(session) = self
            .editor_sessions
            .snapshot(EditorSessionId::new(workspace_id, tab_id))
        {
            return Ok(session);
        }
        let document = self.state.editor_document(Some(workspace_id), tab_id)?;
        Ok(EditorSessionSnapshot {
            id: EditorSessionId::new(workspace_id, tab_id),
            path,
            content: document.content().to_owned(),
            saved_content: document.content().to_owned(),
            revision: 0,
        })
    }

    /// Saves the current session text without invoking format-on-save policy.
    pub fn save_editor_session_unformatted(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        revision: u64,
    ) -> Result<EditorSessionSnapshot, ApplicationError> {
        let (workspace_id, _) = self.state.editor_session_target(workspace_id, tab_id)?;
        let id = EditorSessionId::new(workspace_id, tab_id);
        let session = self
            .editor_sessions
            .snapshot(id)
            .ok_or(EditorSessionError::Missing(id))?;
        if session.revision != revision {
            return Err(EditorSessionError::StaleRevision {
                expected: session.revision,
                received: revision,
            }
            .into());
        }
        self.state
            .save_editor_document(Some(workspace_id), tab_id, &session.content)?;
        self.editor_sessions
            .mark_saved(id, revision)
            .map_err(ApplicationError::from)
    }

    pub fn begin_close(
        &mut self,
        intent: CloseIntent,
    ) -> Result<CloseIntentResult, ApplicationError> {
        let session_ids = self.session_ids_for_target(intent.target);
        let dirty = self.editor_sessions.dirty_for_ids(&session_ids);
        if dirty.is_empty() {
            self.persist_close(intent.target)?;
            self.remove_sessions_for_target(intent.target);
            return Ok(CloseIntentResult::Completed);
        }

        if let Some(pending) = self
            .pending_closes
            .iter()
            .find(|pending| pending.target == intent.target)
        {
            return Ok(CloseIntentResult::RequiresDocumentDecision {
                close_id: pending.id,
                documents: dirty,
            });
        }
        let close_id = self.next_close_id;
        self.next_close_id = self.next_close_id.saturating_add(1).max(1);
        self.pending_closes.push(PendingClose {
            id: close_id,
            target: intent.target,
        });
        Ok(CloseIntentResult::RequiresDocumentDecision {
            close_id,
            documents: dirty,
        })
    }

    pub fn resolve_close(
        &mut self,
        decision: CloseDecision,
    ) -> Result<CloseIntentResult, ApplicationError> {
        let close_id = match &decision {
            CloseDecision::Cancel { close_id } | CloseDecision::Resolve { close_id, .. } => {
                *close_id
            }
        };
        let position = self
            .pending_closes
            .iter()
            .position(|pending| pending.id == close_id)
            .ok_or(ApplicationError::CloseNotFound)?;
        let target = self.pending_closes[position].target;
        if matches!(decision, CloseDecision::Cancel { .. }) {
            self.pending_closes.remove(position);
            return Ok(CloseIntentResult::Completed);
        }

        let CloseDecision::Resolve { documents, .. } = decision else {
            unreachable!("cancel decisions returned before document validation")
        };
        self.validate_close_documents(target, &documents)?;
        for document in &documents {
            if document.decision == CloseDocumentDecision::Save {
                self.save_editor_session_unformatted(
                    Some(document.id.workspace_id),
                    document.id.tab_id,
                    document.revision,
                )?;
            }
        }
        self.persist_close(target)?;
        self.remove_sessions_for_target(target);
        self.pending_closes.remove(position);
        Ok(CloseIntentResult::Completed)
    }

    pub fn editor_session_observations(
        &self,
    ) -> Vec<crate::language_servers::WorkspaceEditOpenDocument> {
        self.editor_sessions.workspace_edit_observations()
    }

    pub fn prepare_workspace_edit_delivery(
        &mut self,
        edit: StagedWorkspaceEdit,
    ) -> WorkspaceEditDeliveryStep {
        let before = self.editor_sessions.clone();
        let step = self.workspace_edit_coordinator.start_with_editor_sessions(
            &mut self.state,
            &self.store,
            &self.editor_sessions,
            edit.clone(),
        );
        if let WorkspaceEditDeliveryStep::Deliver(delivery) = &step {
            self.editor_sessions.apply_workspace_edit(&edit);
            self.workspace_edit_session_recovery
                .insert(delivery.lease, before);
        }
        step
    }

    pub fn prepare_staged_workspace_edit_delivery(
        &mut self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<WorkspaceEditDeliveryStep, WorkspaceEditError> {
        let edit = self
            .state
            .staged_workspace_edit(transaction_id, authorization)?;
        Ok(self.prepare_workspace_edit_delivery(edit))
    }

    pub fn complete_workspace_edit_delivery(
        &mut self,
        lease: crate::language_servers::WorkspaceEditDeliveryLease,
        step: u64,
        outcome: WorkspaceEditDeliveryOutcome,
    ) -> Result<WorkspaceEditDeliveryStep, WorkspaceEditError> {
        let result = self.workspace_edit_coordinator.complete(
            &mut self.state,
            &self.store,
            lease,
            step,
            outcome,
        )?;
        let restore = matches!(
            &result,
            WorkspaceEditDeliveryStep::Deliver(delivery)
                if matches!(delivery.directive, crate::language_servers::WorkspaceEditDirective::UndoOpenModels { .. })
        ) || matches!(
            &result,
            WorkspaceEditDeliveryStep::Complete(application) if !application.applied
        );
        if restore && let Some(sessions) = self.workspace_edit_session_recovery.get(&lease) {
            self.editor_sessions = sessions.clone();
        }
        if matches!(&result, WorkspaceEditDeliveryStep::Complete(_)) {
            self.workspace_edit_session_recovery.remove(&lease);
        }
        Ok(result)
    }

    pub fn resolve_workspace_edit_recovery(
        &mut self,
        transaction_id: u64,
        authorization: &str,
        intent: WorkspaceEditRecoveryIntent,
    ) -> Result<WorkspaceEditTransactionStatus, WorkspaceEditError> {
        self.workspace_edit_coordinator.recover(
            &mut self.state,
            &self.store,
            transaction_id,
            authorization,
            intent,
        )
    }

    pub fn workspace_edit_status(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<WorkspaceEditTransactionStatus, WorkspaceEditError> {
        self.state
            .workspace_edit_status(transaction_id, authorization)
    }

    pub fn finish_workspace_edit(
        &mut self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<bool, WorkspaceEditError> {
        self.state
            .finish_workspace_edit(transaction_id, authorization)
    }

    pub fn acknowledge_workspace_edit_completion(
        &mut self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<bool, WorkspaceEditError> {
        self.state
            .acknowledge_workspace_edit_completion(transaction_id, authorization)
    }

    fn session_ids_for_target(&self, target: CloseTarget) -> Vec<EditorSessionId> {
        match target {
            CloseTarget::Tab {
                workspace_id,
                tab_id,
                ..
            } => vec![EditorSessionId::new(workspace_id, tab_id)],
            CloseTarget::Workspace { workspace_id } => {
                self.editor_sessions.ids_for_workspace(workspace_id)
            }
            CloseTarget::Application => self.editor_sessions.ids(),
        }
    }

    fn validate_close_documents(
        &self,
        target: CloseTarget,
        documents: &[CloseDocumentDecisionRequest],
    ) -> Result<(), ApplicationError> {
        let target_ids = self.session_ids_for_target(target);
        if documents
            .iter()
            .any(|document| !target_ids.contains(&document.id))
        {
            return Err(ApplicationError::InvalidCloseDecision);
        }
        if documents.iter().enumerate().any(|(index, document)| {
            documents[..index]
                .iter()
                .any(|previous| previous.id == document.id)
        }) {
            return Err(ApplicationError::InvalidCloseDecision);
        }
        let dirty = self.editor_sessions.dirty_for_ids(&target_ids);
        for session in dirty {
            let Some(decision) = documents.iter().find(|decision| decision.id == session.id) else {
                return Err(ApplicationError::InvalidCloseDecision);
            };
            if decision.revision != session.revision {
                return Err(EditorSessionError::StaleRevision {
                    expected: session.revision,
                    received: decision.revision,
                }
                .into());
            }
        }
        for document in documents {
            if let Some(session) = self.editor_sessions.snapshot(document.id)
                && document.revision != session.revision
            {
                return Err(EditorSessionError::StaleRevision {
                    expected: session.revision,
                    received: document.revision,
                }
                .into());
            }
        }
        Ok(())
    }

    fn persist_close(&mut self, target: CloseTarget) -> Result<(), ApplicationError> {
        let mut operation = self.prepare_persistent_operation()?;
        let closed = match target {
            CloseTarget::Tab {
                workspace_id,
                pane_id,
                tab_id,
            } => operation
                .state_mut()
                .close_tab(Some(workspace_id), pane_id, tab_id)
                .is_some(),
            CloseTarget::Workspace { workspace_id } => operation
                .state_mut()
                .close_workspace(Some(workspace_id))
                .is_some(),
            CloseTarget::Application => {
                let workspace_ids = operation
                    .state()
                    .workspaces()
                    .workspaces()
                    .iter()
                    .map(|workspace| workspace.id())
                    .collect::<Vec<_>>();
                for workspace_id in workspace_ids {
                    operation.state_mut().close_workspace(Some(workspace_id));
                }
                true
            }
        };
        if !closed {
            self.abandon_persistent_operation();
            return Err(ApplicationError::InvalidCloseDecision);
        }
        if let Err(error) = operation.persist() {
            self.abandon_persistent_operation();
            return Err(error);
        }
        self.complete_persistent_operation(operation)
    }

    fn remove_sessions_for_target(&mut self, target: CloseTarget) {
        match target {
            CloseTarget::Tab {
                workspace_id,
                tab_id,
                ..
            } => self
                .editor_sessions
                .remove(EditorSessionId::new(workspace_id, tab_id)),
            CloseTarget::Workspace { workspace_id } => {
                self.editor_sessions.remove_workspace(workspace_id)
            }
            CloseTarget::Application => self.editor_sessions = EditorSessionRegistry::default(),
        }
    }

    pub fn persist_current_state(&self) -> Result<(), ApplicationError> {
        self.store.save(&self.state).map_err(ApplicationError::from)
    }
}

impl Deref for Application {
    type Target = State;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

impl DerefMut for Application {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.state
    }
}

impl PreparedPersistentOperation {
    pub fn state(&self) -> &State {
        self.candidate.state()
    }

    pub fn state_mut(&mut self) -> &mut State {
        self.candidate.state_mut()
    }

    pub fn persist(&self) -> Result<(), ApplicationError> {
        self.candidate
            .persistence_scope()
            .save(&self.store, self.candidate.state())
            .map_err(ApplicationError::from)
    }

    pub fn open_editor_location(
        &mut self,
        workspace_id: WorkspaceId,
        path: &str,
    ) -> Result<OpenEditorLocation, ApplicationError> {
        self.state_mut()
            .open_editor_location(workspace_id, path)
            .map_err(ApplicationError::from)
    }
}

impl PreparedExternalOperation {
    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut State {
        &mut self.state
    }
}

impl fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DurabilityInFlight => {
                formatter.write_str("a durable operation is already in flight")
            }
            Self::Persistence(error) => error.fmt(formatter),
            Self::StalePreparedOperation => formatter
                .write_str("persistent state changed before the prepared operation completed"),
            Self::Editor(error) => error.fmt(formatter),
            Self::EditorSession(error) => error.fmt(formatter),
            Self::CloseNotFound => formatter.write_str("pending close decision does not exist"),
            Self::InvalidCloseDecision => {
                formatter.write_str("close decision no longer matches application state")
            }
        }
    }
}

impl std::error::Error for ApplicationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Persistence(error) => Some(error),
            Self::Editor(error) => Some(error),
            Self::EditorSession(error) => Some(error),
            Self::DurabilityInFlight
            | Self::StalePreparedOperation
            | Self::CloseNotFound
            | Self::InvalidCloseDecision => None,
        }
    }
}

impl From<PersistenceError> for ApplicationError {
    fn from(error: PersistenceError) -> Self {
        Self::Persistence(error)
    }
}

impl From<EditorError> for ApplicationError {
    fn from(error: EditorError) -> Self {
        Self::Editor(error)
    }
}

impl From<EditorSessionError> for ApplicationError {
    fn from(error: EditorSessionError) -> Self {
        Self::EditorSession(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn persistence_failure_does_not_publish_candidate() {
        let (mut application, path) = test_application("persistence-failure");
        let mut operation = application.prepare_persistent_operation().unwrap();
        operation.state_mut().open_workspace("/workspaces/main");
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();

        assert!(matches!(
            operation.persist(),
            Err(ApplicationError::Persistence(_))
        ));
        application.abandon_persistent_operation();
        assert!(application.state().workspaces().is_empty());

        let _ = std::fs::remove_dir(path);
    }

    #[test]
    fn stale_prepared_operation_is_rejected() {
        let (mut application, path) = test_application("stale-operation");
        let mut operation = application.prepare_persistent_operation().unwrap();
        operation.state_mut().open_workspace("/workspaces/main");
        operation.persist().unwrap();
        application.state_mut().open_workspace("/workspaces/other");

        assert!(matches!(
            application.complete_persistent_operation(operation),
            Err(ApplicationError::StalePreparedOperation)
        ));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn database_commit_precedes_memory_publication() {
        let (mut application, path) = test_application("commit-order");
        let mut operation = application.prepare_persistent_operation().unwrap();
        operation.state_mut().open_workspace("/workspaces/main");
        operation.persist().unwrap();

        assert!(application.state().workspaces().is_empty());
        application
            .complete_persistent_operation(operation)
            .unwrap();
        assert_eq!(application.state().workspaces().workspaces().len(), 1);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn terminal_sessions_survive_persistent_commit() {
        let (mut application, path) = test_application("terminal-session");
        let workspace_id = application.state_mut().open_workspace("/workspaces/main");
        application.state_mut().set_tab_kind(
            Some(workspace_id),
            crate::tree::PaneId::new(1),
            crate::tree::TabId::new(1),
            crate::tree::TabKind::Terminal,
        );
        let mut operation = application.prepare_persistent_operation().unwrap();
        operation.state_mut().open_workspace("/workspaces/other");
        operation.persist().unwrap();
        application
            .complete_persistent_operation(operation)
            .unwrap();

        assert_eq!(application.state().workspaces().workspaces().len(), 2);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn formatter_priority_failure_preserves_previous_state() {
        let (application, path) = test_application("formatter-priority");
        assert!(application.state().formatters().is_err());
        assert!(application.state().formatters().is_err());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn full_state_active_workspace_settings_and_window_scopes_persist() {
        let (mut application, path) = test_application("scopes");
        let mut full = application.prepare_persistent_operation().unwrap();
        full.state_mut().open_workspace("/workspaces/main");
        full.persist().unwrap();
        application.complete_persistent_operation(full).unwrap();

        let workspace_id = application
            .state()
            .workspaces()
            .active_workspace_id()
            .unwrap();
        let mut active = application.prepare_persistent_operation().unwrap();
        active.state_mut().activate_workspace(workspace_id);
        active.persist().unwrap();
        application.complete_persistent_operation(active).unwrap();

        let mut settings = application.prepare_persistent_operation().unwrap();
        settings
            .state_mut()
            .update_setting(
                crate::settings::EDITOR_SOFT_WRAP,
                SettingValue::Boolean(true),
            )
            .unwrap();
        settings.persist().unwrap();
        application.complete_persistent_operation(settings).unwrap();

        let mut window = application.prepare_persistent_operation().unwrap();
        window
            .state_mut()
            .update_window_state(WindowState::new(1, 2, 800, 600, false, false).unwrap());
        window.persist().unwrap();
        application.complete_persistent_operation(window).unwrap();

        let loaded = StateStore::open(&path).unwrap().load().unwrap();
        assert_eq!(
            loaded.settings().boolean(crate::settings::EDITOR_SOFT_WRAP),
            Some(true)
        );
        assert_eq!(loaded.window_state().unwrap().width(), 800);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_durable_lease_rejects_overlapping_prepares() {
        let (mut application, path) = test_application("lease");
        let operation = application.prepare_persistent_operation().unwrap();
        assert!(matches!(
            application.prepare_persistent_operation(),
            Err(ApplicationError::DurabilityInFlight)
        ));
        drop(operation);
        application.abandon_persistent_operation();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn prepared_external_work_does_not_publish_state() {
        let (application, path) = test_application("external");
        let mut operation = application.prepare_external_operation();
        operation.state_mut().open_workspace("/workspaces/main");

        assert!(application.state().workspaces().is_empty());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn stale_close_decisions_cannot_discard_newer_editor_content() {
        let (mut application, root, database, workspace_id, tab_id) =
            editor_application("stale-close");
        application
            .open_editor_session(
                Some(workspace_id),
                tab_id,
                "document.txt",
                "before".to_owned(),
                1,
            )
            .unwrap();
        application
            .change_editor_session(Some(workspace_id), tab_id, "first".to_owned(), 2)
            .unwrap();
        let CloseIntentResult::RequiresDocumentDecision { close_id, .. } = application
            .begin_close(CloseIntent {
                target: CloseTarget::Tab {
                    workspace_id,
                    pane_id: crate::tree::PaneId::new(1),
                    tab_id,
                },
            })
            .unwrap()
        else {
            panic!("dirty editor close should require a decision");
        };
        application
            .change_editor_session(Some(workspace_id), tab_id, "newer".to_owned(), 3)
            .unwrap();

        assert!(matches!(
            application.resolve_close(CloseDecision::Resolve {
                close_id,
                documents: vec![CloseDocumentDecisionRequest {
                    id: EditorSessionId::new(workspace_id, tab_id),
                    revision: 2,
                    decision: CloseDocumentDecision::Discard,
                }],
            }),
            Err(ApplicationError::EditorSession(
                EditorSessionError::StaleRevision { .. }
            ))
        ));
        assert!(
            application
                .state()
                .editor_session_target(Some(workspace_id), tab_id)
                .is_ok()
        );

        application
            .resolve_close(CloseDecision::Resolve {
                close_id,
                documents: vec![CloseDocumentDecisionRequest {
                    id: EditorSessionId::new(workspace_id, tab_id),
                    revision: 3,
                    decision: CloseDocumentDecision::Discard,
                }],
            })
            .unwrap();
        assert!(
            application
                .state()
                .editor_session_target(Some(workspace_id), tab_id)
                .is_err()
        );

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(database);
    }

    #[test]
    fn cancelled_close_leaves_the_dirty_session_and_tab_intact() {
        let (mut application, root, database, workspace_id, tab_id) =
            editor_application("cancel-close");
        application
            .open_editor_session(
                Some(workspace_id),
                tab_id,
                "document.txt",
                "before".to_owned(),
                1,
            )
            .unwrap();
        application
            .change_editor_session(Some(workspace_id), tab_id, "changed".to_owned(), 2)
            .unwrap();
        let CloseIntentResult::RequiresDocumentDecision { close_id, .. } = application
            .begin_close(CloseIntent {
                target: CloseTarget::Tab {
                    workspace_id,
                    pane_id: crate::tree::PaneId::new(1),
                    tab_id,
                },
            })
            .unwrap()
        else {
            panic!("dirty editor close should require a decision");
        };

        application
            .resolve_close(CloseDecision::Cancel { close_id })
            .unwrap();
        assert_eq!(
            application
                .editor_session_document(Some(workspace_id), tab_id)
                .unwrap()
                .content,
            "changed"
        );

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(database);
    }

    #[test]
    fn save_close_updates_the_baseline_before_confirmed_tab_removal() {
        let (mut application, root, database, workspace_id, tab_id) =
            editor_application("save-close");
        application
            .open_editor_session(
                Some(workspace_id),
                tab_id,
                "document.txt",
                "before".to_owned(),
                1,
            )
            .unwrap();
        application
            .change_editor_session(Some(workspace_id), tab_id, "saved".to_owned(), 2)
            .unwrap();
        let CloseIntentResult::RequiresDocumentDecision { close_id, .. } = application
            .begin_close(CloseIntent {
                target: CloseTarget::Tab {
                    workspace_id,
                    pane_id: crate::tree::PaneId::new(1),
                    tab_id,
                },
            })
            .unwrap()
        else {
            panic!("dirty editor close should require a decision");
        };

        application
            .resolve_close(CloseDecision::Resolve {
                close_id,
                documents: vec![CloseDocumentDecisionRequest {
                    id: EditorSessionId::new(workspace_id, tab_id),
                    revision: 2,
                    decision: CloseDocumentDecision::Save,
                }],
            })
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("document.txt")).unwrap(),
            "saved"
        );
        assert!(
            application
                .state()
                .editor_session_target(Some(workspace_id), tab_id)
                .is_err()
        );

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(database);
    }

    #[test]
    fn failed_save_keeps_the_pending_close_and_editor_session() {
        let (mut application, root, database, workspace_id, tab_id) =
            editor_application("save-failure");
        application
            .open_editor_session(
                Some(workspace_id),
                tab_id,
                "document.txt",
                "before".to_owned(),
                1,
            )
            .unwrap();
        application
            .change_editor_session(Some(workspace_id), tab_id, "saved".to_owned(), 2)
            .unwrap();
        std::fs::remove_file(root.join("document.txt")).unwrap();
        std::fs::create_dir(root.join("document.txt")).unwrap();
        let CloseIntentResult::RequiresDocumentDecision { close_id, .. } = application
            .begin_close(CloseIntent {
                target: CloseTarget::Tab {
                    workspace_id,
                    pane_id: crate::tree::PaneId::new(1),
                    tab_id,
                },
            })
            .unwrap()
        else {
            panic!("dirty editor close should require a decision");
        };

        assert!(matches!(
            application.resolve_close(CloseDecision::Resolve {
                close_id,
                documents: vec![CloseDocumentDecisionRequest {
                    id: EditorSessionId::new(workspace_id, tab_id),
                    revision: 2,
                    decision: CloseDocumentDecision::Save,
                }],
            }),
            Err(ApplicationError::Editor(_))
        ));
        assert_eq!(
            application
                .editor_session_document(Some(workspace_id), tab_id)
                .unwrap()
                .content,
            "saved"
        );
        application
            .resolve_close(CloseDecision::Cancel { close_id })
            .unwrap();

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(database);
    }

    fn editor_application(name: &str) -> (Application, PathBuf, PathBuf, WorkspaceId, TabId) {
        let (mut application, database) = test_application(name);
        let root = std::env::temp_dir().join(format!(
            "kosmos-application-editor-{name}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("document.txt"), "before").unwrap();
        let workspace_id = application.state_mut().open_workspace(&root);
        assert!(application.state_mut().set_tab_kind(
            Some(workspace_id),
            crate::tree::PaneId::new(1),
            TabId::new(1),
            crate::tree::TabKind::FileTree,
        ));
        application
            .state_mut()
            .open_editor_tab(Some(workspace_id), TabId::new(1), "document.txt")
            .unwrap();
        let tab_id = application.state().editor_view_states()[0].tab_id();
        (application, root, database, workspace_id, tab_id)
    }

    fn test_application(name: &str) -> (Application, PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "kosmos-application-{}-{name}-{}.sqlite3",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = StateStore::open(&path).unwrap();
        (Application::new(State::new(), store), path)
    }
}
