use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use crate::State;
use crate::events::CoreEventSink;
use crate::language_servers::{WorkspaceEditError, WorkspaceEditTransactionStatus};
use crate::persistence::{PersistenceError, StateStore};
use crate::settings::{SettingValue, SettingsError};
use crate::state::PersistentStateCandidate;
use crate::window::WindowState;

/// Owns the mutable application state and its durable backing store.
///
/// Callers prepare work while holding their application mutex, run the prepared
/// operation without that mutex, and complete it after durable storage succeeds.
pub struct Application {
    state: State,
    store: StateStore,
    durability_lease_active: bool,
    next_workspace_edit_owner: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct WorkspaceEditOwnerToken(u64);

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
}

impl Application {
    pub fn new(state: State, store: StateStore) -> Self {
        Self {
            state,
            store,
            durability_lease_active: false,
            next_workspace_edit_owner: 1,
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

    pub fn workspace_edit_owner_token(&mut self) -> WorkspaceEditOwnerToken {
        let token = WorkspaceEditOwnerToken(self.next_workspace_edit_owner);
        self.next_workspace_edit_owner = self.next_workspace_edit_owner.wrapping_add(1).max(1);
        token
    }

    pub fn claim_workspace_edit_owner(
        &mut self,
        transaction_id: u64,
        authorization: &str,
        owner: WorkspaceEditOwnerToken,
    ) -> Result<(), WorkspaceEditError> {
        self.state
            .claim_workspace_edit_owner(transaction_id, authorization, owner.0)
    }

    pub fn cancel_owned_workspace_edit(
        &mut self,
        transaction_id: u64,
        authorization: &str,
        owner: WorkspaceEditOwnerToken,
    ) -> Result<WorkspaceEditTransactionStatus, WorkspaceEditError> {
        self.state
            .cancel_owned_workspace_edit(transaction_id, authorization, owner.0)
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

    pub fn disconnect_workspace_edit_owner(
        &mut self,
        owner: WorkspaceEditOwnerToken,
    ) -> Vec<WorkspaceEditTransactionStatus> {
        self.state.disconnect_workspace_edit_owner(owner.0)
    }

    pub fn finish_disconnected_workspace_edit_rollbacks(&self, transaction_ids: &[u64]) {
        self.state
            .finish_disconnected_workspace_edit_rollbacks(transaction_ids);
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
        }
    }
}

impl std::error::Error for ApplicationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Persistence(error) => Some(error),
            Self::DurabilityInFlight | Self::StalePreparedOperation => None,
        }
    }
}

impl From<PersistenceError> for ApplicationError {
    fn from(error: PersistenceError) -> Self {
        Self::Persistence(error)
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
