use std::collections::HashMap;

use crate::EditorSessionRegistry;
use crate::State;
use crate::events::WorkspaceEditApplication;
use crate::persistence::StateStore;

use super::{
    StagedWorkspaceEdit, WorkspaceEditError, WorkspaceEditModelDirective,
    WorkspaceEditTransactionPhase,
};

const MAX_RETAINED_DELIVERIES: usize = 64;

/// Core-issued identifier for one renderer delivery sequence.
///
/// It deliberately has no relationship to a renderer, IPC request, or connection identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct WorkspaceEditDeliveryLease(u64);

impl WorkspaceEditDeliveryLease {
    pub fn token(self) -> String {
        format!("workspace-edit-{:016x}", self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceEditDirective {
    ApplyOpenModels {
        transaction_id: u64,
        models: Vec<WorkspaceEditModelDirective>,
    },
    UndoOpenModels {
        transaction_id: u64,
        models: Vec<WorkspaceEditModelDirective>,
    },
    ReconcileCommittedModels {
        transaction_id: u64,
    },
    ReconcileRolledBackModels {
        transaction_id: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingWorkspaceEditDelivery {
    pub lease: WorkspaceEditDeliveryLease,
    pub step: u64,
    /// Retained for protocol compatibility. Adapters must follow `directive`, not infer policy
    /// from this staged description.
    pub edit: StagedWorkspaceEdit,
    pub directive: WorkspaceEditDirective,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceEditDeliveryStep {
    Deliver(PendingWorkspaceEditDelivery),
    Complete(WorkspaceEditApplication),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceEditDeliveryOutcome {
    Applied,
    Rejected(String),
    TimedOut,
    Cancelled,
    RendererDisconnected,
    NotDelivered(String),
}

#[derive(Default)]
pub struct WorkspaceEditCoordinator {
    next_lease: u64,
    deliveries: HashMap<WorkspaceEditDeliveryLease, Delivery>,
    completed: HashMap<(WorkspaceEditDeliveryLease, u64), WorkspaceEditDeliveryStep>,
}

struct Delivery {
    edit: StagedWorkspaceEdit,
    models: Vec<WorkspaceEditModelDirective>,
    current_step: u64,
    directive: WorkspaceEditDirective,
    failure_reason: Option<String>,
}

impl WorkspaceEditCoordinator {
    pub fn start(
        &mut self,
        state: &mut State,
        store: &StateStore,
        edit: StagedWorkspaceEdit,
    ) -> WorkspaceEditDeliveryStep {
        self.start_with_editor_sessions(state, store, &EditorSessionRegistry::default(), edit)
    }

    pub fn start_with_editor_sessions(
        &mut self,
        state: &mut State,
        store: &StateStore,
        sessions: &EditorSessionRegistry,
        edit: StagedWorkspaceEdit,
    ) -> WorkspaceEditDeliveryStep {
        if let Err(error) = state.commit_workspace_edit_with_open_documents(
            edit.transaction_id,
            &edit.authorization,
            &sessions.workspace_edit_observations(),
        ) {
            return self.complete_start_failure(state, store, &edit, error);
        }
        if let Err(error) = persist_state(state, store) {
            return self.complete_start_failure(state, store, &edit, error);
        }
        let models =
            match state.workspace_edit_model_directives(edit.transaction_id, &edit.authorization) {
                Ok(models) => models,
                Err(error) => return self.complete_start_failure(state, store, &edit, error),
            };
        let lease = self.allocate_lease();
        let directive = WorkspaceEditDirective::ApplyOpenModels {
            transaction_id: edit.transaction_id,
            models: models.clone(),
        };
        self.deliveries.insert(
            lease,
            Delivery {
                edit,
                models,
                current_step: 1,
                directive: directive.clone(),
                failure_reason: None,
            },
        );
        WorkspaceEditDeliveryStep::Deliver(self.pending(lease, 1, directive))
    }

    pub fn complete(
        &mut self,
        state: &mut State,
        store: &StateStore,
        lease: WorkspaceEditDeliveryLease,
        step: u64,
        outcome: WorkspaceEditDeliveryOutcome,
    ) -> Result<WorkspaceEditDeliveryStep, WorkspaceEditError> {
        if let Some(recorded) = self.completed.get(&(lease, step)) {
            return Ok(recorded.clone());
        }
        let delivery = self.deliveries.get(&lease).ok_or_else(|| {
            WorkspaceEditError::Invalid("workspace edit delivery lease is unknown".to_owned())
        })?;
        if delivery.current_step != step {
            return Err(WorkspaceEditError::Invalid(
                "workspace edit delivery acknowledgement is stale".to_owned(),
            ));
        }
        let directive = delivery.directive.clone();
        let result = match directive {
            WorkspaceEditDirective::ApplyOpenModels { .. } => {
                self.complete_apply(state, store, lease, outcome)
            }
            WorkspaceEditDirective::UndoOpenModels { .. } => {
                self.complete_undo(state, store, lease, outcome)
            }
            WorkspaceEditDirective::ReconcileCommittedModels { .. } => {
                self.complete_reconciliation(state, store, lease, outcome, true)
            }
            WorkspaceEditDirective::ReconcileRolledBackModels { .. } => {
                self.complete_reconciliation(state, store, lease, outcome, false)
            }
        };
        self.completed.insert((lease, step), result.clone());
        self.trim_completed();
        Ok(result)
    }

    pub fn recover(
        &mut self,
        state: &mut State,
        store: &StateStore,
        transaction_id: u64,
        authorization: &str,
        intent: WorkspaceEditRecoveryIntent,
    ) -> Result<super::WorkspaceEditTransactionStatus, WorkspaceEditError> {
        match intent {
            WorkspaceEditRecoveryIntent::RetryRollback => {
                state.rollback_workspace_edit(transaction_id, authorization)?;
                persist_state(state, store)?;
                state.finish_workspace_edit(transaction_id, authorization)?;
            }
            WorkspaceEditRecoveryIntent::Finalize => {
                state.finalize_workspace_edit(transaction_id, authorization)?;
            }
        }
        persist_state(state, store)?;
        let status = state.workspace_edit_status(transaction_id, authorization)?;
        state.acknowledge_workspace_edit_completion(transaction_id, authorization)?;
        persist_state(state, store)?;
        Ok(status)
    }

    fn complete_apply(
        &mut self,
        state: &mut State,
        store: &StateStore,
        lease: WorkspaceEditDeliveryLease,
        outcome: WorkspaceEditDeliveryOutcome,
    ) -> WorkspaceEditDeliveryStep {
        let edit = self
            .deliveries
            .get(&lease)
            .expect("delivery existence was checked before completion")
            .edit
            .clone();
        match outcome {
            WorkspaceEditDeliveryOutcome::Applied => {
                match finish_workspace_edit(state, store, &edit) {
                    Ok(()) => self.next_reconciliation(lease, true),
                    Err(error) => self.complete_durable_error(state, &edit, error),
                }
            }
            WorkspaceEditDeliveryOutcome::RendererDisconnected
            | WorkspaceEditDeliveryOutcome::NotDelivered(_) => {
                let reason = outcome_reason(outcome);
                self.rollback_without_model_undo(state, store, lease, reason)
            }
            outcome => {
                let reason = outcome_reason(outcome);
                match state.rollback_workspace_edit(edit.transaction_id, &edit.authorization) {
                    Ok(()) => match persist_state(state, store) {
                        Ok(()) => {
                            let (step, directive) = {
                                let delivery = self
                                    .deliveries
                                    .get_mut(&lease)
                                    .expect("delivery existence was checked before completion");
                                delivery.current_step += 1;
                                delivery.failure_reason = Some(reason);
                                delivery.directive = WorkspaceEditDirective::UndoOpenModels {
                                    transaction_id: delivery.edit.transaction_id,
                                    models: delivery.models.clone(),
                                };
                                (delivery.current_step, delivery.directive.clone())
                            };
                            WorkspaceEditDeliveryStep::Deliver(self.pending(lease, step, directive))
                        }
                        Err(error) => self.complete_durable_error(state, &edit, error),
                    },
                    Err(error) => self.complete_durable_error(state, &edit, error),
                }
            }
        }
    }

    fn complete_undo(
        &mut self,
        state: &mut State,
        store: &StateStore,
        lease: WorkspaceEditDeliveryLease,
        outcome: WorkspaceEditDeliveryOutcome,
    ) -> WorkspaceEditDeliveryStep {
        let (edit, _failure_reason) = self
            .deliveries
            .get(&lease)
            .map(|delivery| (delivery.edit.clone(), delivery.failure_reason.clone()))
            .expect("delivery existence was checked before completion");
        match outcome {
            WorkspaceEditDeliveryOutcome::Applied => {
                match finish_workspace_edit(state, store, &edit) {
                    Ok(()) => self.next_reconciliation(lease, false),
                    Err(error) => self.complete_durable_error(state, &edit, error),
                }
            }
            outcome => self.complete_delivery(
                lease,
                WorkspaceEditApplication {
                    applied: false,
                    failure_reason: Some(format!(
                        "{}; open-model rollback requires recovery",
                        outcome_reason(outcome)
                    )),
                },
            ),
        }
    }

    fn complete_start_failure(
        &mut self,
        state: &mut State,
        store: &StateStore,
        edit: &StagedWorkspaceEdit,
        error: WorkspaceEditError,
    ) -> WorkspaceEditDeliveryStep {
        let reason = error.to_string();
        match state.rollback_workspace_edit(edit.transaction_id, &edit.authorization) {
            Ok(()) => match state.finish_workspace_edit(edit.transaction_id, &edit.authorization) {
                Ok(_) => match persist_state(state, store) {
                    Ok(()) => WorkspaceEditDeliveryStep::Complete(WorkspaceEditApplication {
                        applied: false,
                        failure_reason: Some(reason),
                    }),
                    Err(persist) => WorkspaceEditDeliveryStep::Complete(WorkspaceEditApplication {
                        applied: false,
                        failure_reason: Some(format!("{reason}; {persist}")),
                    }),
                },
                Err(finish) => self.complete_durable_error(state, edit, finish),
            },
            Err(rollback) => self.complete_durable_error(state, edit, rollback),
        }
    }

    fn rollback_without_model_undo(
        &mut self,
        state: &mut State,
        store: &StateStore,
        lease: WorkspaceEditDeliveryLease,
        reason: String,
    ) -> WorkspaceEditDeliveryStep {
        let edit = self
            .deliveries
            .get(&lease)
            .expect("delivery existence was checked before completion")
            .edit
            .clone();
        match state.rollback_workspace_edit(edit.transaction_id, &edit.authorization) {
            Ok(()) => match finish_and_acknowledge(state, store, &edit) {
                Ok(()) => self.complete_delivery(
                    lease,
                    WorkspaceEditApplication {
                        applied: false,
                        failure_reason: Some(reason),
                    },
                ),
                Err(error) => self.complete_durable_error(state, &edit, error),
            },
            Err(error) => self.complete_durable_error(state, &edit, error),
        }
    }

    fn complete_durable_error(
        &mut self,
        state: &State,
        edit: &StagedWorkspaceEdit,
        error: WorkspaceEditError,
    ) -> WorkspaceEditDeliveryStep {
        let result = state.workspace_edit_status(edit.transaction_id, &edit.authorization);
        let applied = matches!(
            result.map(|status| status.phase),
            Ok(WorkspaceEditTransactionPhase::FinishingCommitted
                | WorkspaceEditTransactionPhase::CommittedCleanupRequired
                | WorkspaceEditTransactionPhase::FinishedCommitted)
        );
        WorkspaceEditDeliveryStep::Complete(WorkspaceEditApplication {
            applied,
            failure_reason: Some(error.to_string()),
        })
    }

    fn complete_reconciliation(
        &mut self,
        state: &mut State,
        store: &StateStore,
        lease: WorkspaceEditDeliveryLease,
        outcome: WorkspaceEditDeliveryOutcome,
        committed: bool,
    ) -> WorkspaceEditDeliveryStep {
        let (edit, failure_reason) = self
            .deliveries
            .get(&lease)
            .map(|delivery| (delivery.edit.clone(), delivery.failure_reason.clone()))
            .expect("delivery existence was checked before completion");
        match outcome {
            WorkspaceEditDeliveryOutcome::Applied => {
                match acknowledge_workspace_edit(state, store, &edit) {
                    Ok(()) => self.complete_delivery(
                        lease,
                        WorkspaceEditApplication {
                            applied: committed,
                            failure_reason: if committed { None } else { failure_reason },
                        },
                    ),
                    Err(error) => self.complete_durable_error(state, &edit, error),
                }
            }
            outcome => self.complete_delivery(
                lease,
                WorkspaceEditApplication {
                    applied: committed,
                    failure_reason: Some(format!(
                        "{}; workspace edit reconciliation requires recovery",
                        outcome_reason(outcome)
                    )),
                },
            ),
        }
    }

    fn next_reconciliation(
        &mut self,
        lease: WorkspaceEditDeliveryLease,
        committed: bool,
    ) -> WorkspaceEditDeliveryStep {
        let (step, directive) = {
            let delivery = self
                .deliveries
                .get_mut(&lease)
                .expect("delivery existence was checked before completion");
            delivery.current_step += 1;
            let transaction_id = delivery.edit.transaction_id;
            delivery.directive = if committed {
                WorkspaceEditDirective::ReconcileCommittedModels { transaction_id }
            } else {
                WorkspaceEditDirective::ReconcileRolledBackModels { transaction_id }
            };
            (delivery.current_step, delivery.directive.clone())
        };
        WorkspaceEditDeliveryStep::Deliver(self.pending(lease, step, directive))
    }

    fn allocate_lease(&mut self) -> WorkspaceEditDeliveryLease {
        self.next_lease = self.next_lease.wrapping_add(1).max(1);
        WorkspaceEditDeliveryLease(self.next_lease)
    }

    fn pending(
        &self,
        lease: WorkspaceEditDeliveryLease,
        step: u64,
        directive: WorkspaceEditDirective,
    ) -> PendingWorkspaceEditDelivery {
        let delivery = self
            .deliveries
            .get(&lease)
            .expect("delivery is inserted before it is published");
        PendingWorkspaceEditDelivery {
            lease,
            step,
            edit: delivery.edit.clone(),
            directive,
        }
    }

    fn complete_delivery(
        &mut self,
        lease: WorkspaceEditDeliveryLease,
        result: WorkspaceEditApplication,
    ) -> WorkspaceEditDeliveryStep {
        self.deliveries.remove(&lease);
        WorkspaceEditDeliveryStep::Complete(result)
    }

    fn trim_completed(&mut self) {
        if self.completed.len() <= MAX_RETAINED_DELIVERIES {
            return;
        }
        let remove = self.completed.len() - MAX_RETAINED_DELIVERIES;
        let keys = self
            .completed
            .keys()
            .copied()
            .take(remove)
            .collect::<Vec<_>>();
        for key in keys {
            self.completed.remove(&key);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceEditRecoveryIntent {
    RetryRollback,
    Finalize,
}

fn finish_and_acknowledge(
    state: &mut State,
    store: &StateStore,
    edit: &StagedWorkspaceEdit,
) -> Result<(), WorkspaceEditError> {
    finish_workspace_edit(state, store, edit)?;
    acknowledge_workspace_edit(state, store, edit)
}

fn finish_workspace_edit(
    state: &mut State,
    store: &StateStore,
    edit: &StagedWorkspaceEdit,
) -> Result<(), WorkspaceEditError> {
    state.finish_workspace_edit(edit.transaction_id, &edit.authorization)?;
    persist_state(state, store)
}

fn acknowledge_workspace_edit(
    state: &mut State,
    store: &StateStore,
    edit: &StagedWorkspaceEdit,
) -> Result<(), WorkspaceEditError> {
    state.acknowledge_workspace_edit_completion(edit.transaction_id, &edit.authorization)?;
    persist_state(state, store)
}

fn persist_state(state: &State, store: &StateStore) -> Result<(), WorkspaceEditError> {
    store.save(state).map_err(|error| {
        WorkspaceEditError::Recovery(format!("workspace edit state persistence failed: {error}"))
    })
}

fn outcome_reason(outcome: WorkspaceEditDeliveryOutcome) -> String {
    match outcome {
        WorkspaceEditDeliveryOutcome::Applied => "workspace edit was applied".to_owned(),
        WorkspaceEditDeliveryOutcome::Rejected(reason)
        | WorkspaceEditDeliveryOutcome::NotDelivered(reason) => reason,
        WorkspaceEditDeliveryOutcome::TimedOut => {
            "workspace edit acknowledgement timed out".to_owned()
        }
        WorkspaceEditDeliveryOutcome::Cancelled => "workspace edit was cancelled".to_owned(),
        WorkspaceEditDeliveryOutcome::RendererDisconnected => {
            "renderer disconnected while applying workspace edit".to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language_servers::{
        LanguageServerManager, LanguageServerPaths, StagedWorkspaceEditDocument,
        StagedWorkspaceEditOperation, WorkspaceEditOpenDocument, WorkspaceEditRoot,
    };
    use crate::tree::WorkspaceId;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn success_commits_applies_reconciles_and_acknowledges() {
        let fixture = Fixture::new("success");
        let mut coordinator = WorkspaceEditCoordinator::default();
        let first = coordinator.start(&mut fixture.state(), &fixture.store, fixture.staged.clone());
        let apply = pending(first);
        let reconcile = coordinator
            .complete(
                &mut fixture.state(),
                &fixture.store,
                apply.lease,
                apply.step,
                WorkspaceEditDeliveryOutcome::Applied,
            )
            .unwrap();
        let reconcile = pending(reconcile);
        let complete = coordinator
            .complete(
                &mut fixture.state(),
                &fixture.store,
                reconcile.lease,
                reconcile.step,
                WorkspaceEditDeliveryOutcome::Applied,
            )
            .unwrap();

        assert_eq!(
            complete,
            WorkspaceEditDeliveryStep::Complete(WorkspaceEditApplication {
                applied: true,
                failure_reason: None,
            })
        );
        assert_eq!(fs::read_to_string(fixture.path()).unwrap(), "after");
    }

    #[test]
    fn preflight_failure_finishes_without_applying_models() {
        let fixture = Fixture::new("preflight");
        fs::write(fixture.path(), "changed externally").unwrap();
        let result = WorkspaceEditCoordinator::default().start(
            &mut fixture.state(),
            &fixture.store,
            fixture.staged.clone(),
        );

        assert!(matches!(result, WorkspaceEditDeliveryStep::Complete(_)));
        assert_eq!(
            fs::read_to_string(fixture.path()).unwrap(),
            "changed externally"
        );
    }

    #[test]
    fn partial_open_model_failure_requests_undo_before_completion() {
        let fixture = Fixture::new("partial-open-model");
        let mut coordinator = WorkspaceEditCoordinator::default();
        let apply = pending(coordinator.start(
            &mut fixture.state(),
            &fixture.store,
            fixture.staged.clone(),
        ));
        let undo = pending(
            coordinator
                .complete(
                    &mut fixture.state(),
                    &fixture.store,
                    apply.lease,
                    apply.step,
                    WorkspaceEditDeliveryOutcome::Rejected("second Monaco model failed".to_owned()),
                )
                .unwrap(),
        );
        assert!(matches!(
            undo.directive,
            WorkspaceEditDirective::UndoOpenModels { .. }
        ));
        let reconcile = pending(
            coordinator
                .complete(
                    &mut fixture.state(),
                    &fixture.store,
                    undo.lease,
                    undo.step,
                    WorkspaceEditDeliveryOutcome::Applied,
                )
                .unwrap(),
        );
        let complete = coordinator
            .complete(
                &mut fixture.state(),
                &fixture.store,
                reconcile.lease,
                reconcile.step,
                WorkspaceEditDeliveryOutcome::Applied,
            )
            .unwrap();

        assert!(matches!(
            complete,
            WorkspaceEditDeliveryStep::Complete(WorkspaceEditApplication { applied: false, .. })
        ));
        assert_eq!(fs::read_to_string(fixture.path()).unwrap(), "before");
    }

    #[test]
    fn cancellation_and_timeout_follow_the_same_rollback_policy() {
        for outcome in [
            WorkspaceEditDeliveryOutcome::Cancelled,
            WorkspaceEditDeliveryOutcome::TimedOut,
        ] {
            let fixture = Fixture::new("interrupted");
            let mut coordinator = WorkspaceEditCoordinator::default();
            let apply = pending(coordinator.start(
                &mut fixture.state(),
                &fixture.store,
                fixture.staged.clone(),
            ));
            let undo = pending(
                coordinator
                    .complete(
                        &mut fixture.state(),
                        &fixture.store,
                        apply.lease,
                        apply.step,
                        outcome,
                    )
                    .unwrap(),
            );
            assert!(matches!(
                undo.directive,
                WorkspaceEditDirective::UndoOpenModels { .. }
            ));
        }
    }

    #[test]
    fn duplicate_and_stale_delivery_completions_are_idempotent() {
        let fixture = Fixture::new("idempotent");
        let mut coordinator = WorkspaceEditCoordinator::default();
        let apply = pending(coordinator.start(
            &mut fixture.state(),
            &fixture.store,
            fixture.staged.clone(),
        ));
        let first = coordinator
            .complete(
                &mut fixture.state(),
                &fixture.store,
                apply.lease,
                apply.step,
                WorkspaceEditDeliveryOutcome::Rejected("lost response".to_owned()),
            )
            .unwrap();
        let duplicate = coordinator
            .complete(
                &mut fixture.state(),
                &fixture.store,
                apply.lease,
                apply.step,
                WorkspaceEditDeliveryOutcome::Applied,
            )
            .unwrap();
        assert_eq!(first, duplicate);
        assert!(matches!(
            coordinator.complete(
                &mut fixture.state(),
                &fixture.store,
                apply.lease,
                apply.step + 10,
                WorkspaceEditDeliveryOutcome::Applied,
            ),
            Err(WorkspaceEditError::Invalid(_))
        ));
    }

    #[test]
    fn renderer_disconnect_rolls_back_without_storing_connection_identity_in_core() {
        let fixture = Fixture::new("disconnect");
        let mut coordinator = WorkspaceEditCoordinator::default();
        let apply = pending(coordinator.start(
            &mut fixture.state(),
            &fixture.store,
            fixture.staged.clone(),
        ));
        let complete = coordinator
            .complete(
                &mut fixture.state(),
                &fixture.store,
                apply.lease,
                apply.step,
                WorkspaceEditDeliveryOutcome::RendererDisconnected,
            )
            .unwrap();

        assert!(matches!(
            complete,
            WorkspaceEditDeliveryStep::Complete(WorkspaceEditApplication { applied: false, .. })
        ));
        assert_eq!(fs::read_to_string(fixture.path()).unwrap(), "before");
    }

    #[test]
    fn unknown_transaction_is_rejected_without_advancing_a_lease() {
        let fixture = Fixture::new("unknown");
        assert!(matches!(
            WorkspaceEditCoordinator::default().complete(
                &mut fixture.state(),
                &fixture.store,
                WorkspaceEditDeliveryLease(99),
                1,
                WorkspaceEditDeliveryOutcome::Applied,
            ),
            Err(WorkspaceEditError::Invalid(_))
        ));
    }

    #[test]
    fn ordered_resource_operations_and_dirty_overwrite_rejection_are_planned_in_core() {
        let edit = StagedWorkspaceEdit {
            transaction_id: 1,
            authorization: "test".to_owned(),
            documents: vec![StagedWorkspaceEditDocument {
                workspace_id: WorkspaceId::new(1),
                path: "renamed.ts".to_owned(),
                original_path: "old.ts".to_owned(),
                original_text: "before".to_owned(),
                new_text: "after".to_owned(),
                generation: Some(4),
                version: Some(7),
            }],
            operations: vec![
                StagedWorkspaceEditOperation::RenameFile {
                    workspace_id: WorkspaceId::new(1),
                    old_path: "old.ts".to_owned(),
                    new_path: "renamed.ts".to_owned(),
                },
                StagedWorkspaceEditOperation::TextDocument { document: 0 },
            ],
        };
        let observations = vec![WorkspaceEditOpenDocument {
            workspace_id: WorkspaceId::new(1),
            path: "old.ts".to_owned(),
            generation: 4,
            version: 7,
            text: "before".to_owned(),
            saved_text: "before".to_owned(),
        }];
        let directives =
            crate::language_servers::edits::plan_open_model_lineages(&edit, &observations).unwrap();
        assert_eq!(directives[0].path.as_deref(), Some("renamed.ts"));
        assert_eq!(directives[0].text, "after");

        let dirty = WorkspaceEditOpenDocument {
            text: "unsaved".to_owned(),
            ..observations[0].clone()
        };
        let delete = StagedWorkspaceEdit {
            operations: vec![StagedWorkspaceEditOperation::DeleteFile {
                workspace_id: WorkspaceId::new(1),
                path: "old.ts".to_owned(),
                recursive: false,
            }],
            ..edit
        };
        assert!(matches!(
            crate::language_servers::edits::plan_open_model_lineages(&delete, &[dirty]),
            Err(WorkspaceEditError::Stale(_))
        ));
    }

    fn pending(step: WorkspaceEditDeliveryStep) -> PendingWorkspaceEditDelivery {
        match step {
            WorkspaceEditDeliveryStep::Deliver(delivery) => delivery,
            WorkspaceEditDeliveryStep::Complete(result) => {
                panic!("unexpected completion: {result:?}")
            }
        }
    }

    struct Fixture {
        root: PathBuf,
        store: StateStore,
        state: std::cell::RefCell<State>,
        staged: StagedWorkspaceEdit,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "kosmos-workspace-edit-coordinator-{name}-{}-{}",
                std::process::id(),
                NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir_all(&root).unwrap();
            let path = root.join("target.txt");
            fs::write(&path, "before").unwrap();
            let store = StateStore::open(root.join("state.sqlite3")).unwrap();
            let manager = LanguageServerManager::open(
                LanguageServerPaths::new(root.join("language-servers"), root.join("cache")),
                store.clone(),
            )
            .unwrap();
            let mut state = State::new();
            let workspace_id = state.open_workspace(&root);
            let staged = manager
                .stage_workspace_edit(
                    &serde_json::json!({ "changes": {
                        format!("file://{}", path.display()): [{
                            "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 6 } },
                            "newText": "after"
                        }]
                    }}),
                    &[WorkspaceEditRoot {
                        workspace_id,
                        path: root.clone(),
                    }],
                )
                .unwrap();
            state.attach_language_server_manager(manager);
            Self {
                root,
                store,
                state: std::cell::RefCell::new(state),
                staged,
            }
        }

        fn state(&self) -> std::cell::RefMut<'_, State> {
            self.state.borrow_mut()
        }

        fn path(&self) -> PathBuf {
            self.root.join("target.txt")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
