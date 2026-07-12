use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::time::Duration;

use super::response::ResponseSender;
use crate::ipc::messages::envelope::ServerMessage;

const MAX_PENDING_APPLY_EDITS: usize = 16;
const APPLY_EDIT_ACK_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Default)]
pub(crate) struct NotificationHub {
    subscribers: Arc<Mutex<Subscribers>>,
    application: Arc<Mutex<Option<Weak<Mutex<core::Application>>>>>,
    workspace_edit_publication: Arc<Mutex<()>>,
}

impl NotificationHub {
    pub(crate) fn workspace_edit_publication_gate(&self) -> Arc<Mutex<()>> {
        Arc::clone(&self.workspace_edit_publication)
    }

    pub(crate) fn attach_application(&self, application: Weak<Mutex<core::Application>>) {
        *self
            .application
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(application);
    }

    pub(crate) fn subscribe(&self, responses: ResponseSender) -> NotificationSubscription {
        let mut subscribers = self
            .subscribers
            .lock()
            .expect("notification subscribers should lock");
        let id = subscribers.next_id;

        subscribers.next_id = subscribers.next_id.wrapping_add(1);
        let owner = self.application().and_then(|application| {
            application
                .lock()
                .ok()
                .map(|mut application| application.workspace_edit_owner_token())
        });
        subscribers.responses.insert(id, responses);
        if let Some(owner) = owner {
            subscribers.workspace_edit_owners.insert(id, owner);
        }

        NotificationSubscription {
            id,
            workspace_edit_owner: owner,
            hub: self.clone(),
        }
    }

    pub(crate) fn workspace_changed(&self, workspace_ids: Vec<u64>) {
        let Ok(mut subscribers) = self.subscribers.lock() else {
            return;
        };
        let disconnected = subscribers
            .responses
            .iter()
            .filter(|(_, responses)| !responses.notify_workspace_changed(&workspace_ids))
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        let pending = subscribers.remove_renderers(&disconnected);
        drop(subscribers);
        self.disconnect_renderers_async(disconnected, pending);
    }

    fn core_event(&self, event: core::events::CoreEvent) {
        let Ok(mut subscribers) = self.subscribers.lock() else {
            return;
        };
        let disconnected = subscribers
            .responses
            .iter()
            .filter(|(_, responses)| !responses.notify_core_event(event.clone()))
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        let pending = subscribers.remove_renderers(&disconnected);
        drop(subscribers);
        self.disconnect_renderers_async(disconnected, pending);
    }

    pub(crate) fn acknowledge_apply_edit(
        &self,
        renderer_id: u64,
        id: u64,
        token: &str,
        applied: bool,
        failure_reason: Option<String>,
    ) {
        let pending = self.subscribers.lock().ok().and_then(|mut subscribers| {
            let pending = subscribers.pending_apply_edits.get(&id)?;
            if pending.renderer_id != renderer_id || pending.token != token {
                return None;
            }
            subscribers.pending_apply_edits.remove(&id)
        });
        if let Some(pending) = pending {
            let result = self.resolve_acknowledgement(&pending, applied, failure_reason);
            let _ = pending.result.send(result);
        }
    }

    fn request_apply_edit(
        &self,
        edit: core::language_servers::StagedWorkspaceEdit,
    ) -> core::events::WorkspaceEditApplication {
        self.request_apply_edit_with_timeout(edit, APPLY_EDIT_ACK_TIMEOUT)
    }

    fn request_apply_edit_with_timeout(
        &self,
        edit: core::language_servers::StagedWorkspaceEdit,
        timeout: Duration,
    ) -> core::events::WorkspaceEditApplication {
        let (result, receiver) = mpsc::sync_channel(1);
        let token = edit.authorization.clone();
        let (id, renderer_id, response) = {
            let Ok(mut subscribers) = self.subscribers.lock() else {
                return apply_edit_failure("workspace edit notification hub is unavailable");
            };
            if subscribers.pending_apply_edits.len() >= MAX_PENDING_APPLY_EDITS {
                return apply_edit_failure("too many workspace edit acknowledgements are pending");
            }
            let Some((&renderer_id, response)) = subscribers.responses.iter().next() else {
                return apply_edit_failure("no renderer is connected to apply the workspace edit");
            };
            let response = response.clone();
            let id = subscribers.next_apply_edit_id;
            subscribers.next_apply_edit_id = id.wrapping_add(1).max(1);
            subscribers.pending_apply_edits.insert(
                id,
                PendingApplyEdit {
                    renderer_id,
                    token: token.clone(),
                    cancelled: false,
                    transaction_id: edit.transaction_id,
                    authorization: edit.authorization.clone(),
                    result,
                },
            );
            (id, renderer_id, response)
        };
        if let Err(error) = self.claim_transaction(renderer_id, &edit) {
            self.acknowledge_apply_edit(renderer_id, id, &token, false, Some(error.to_string()));
            return receiver.recv().unwrap_or_else(|_| {
                apply_edit_failure("workspace edit ownership could not be established")
            });
        }
        if !response.try_send(ServerMessage::language_server_apply_edit(
            id,
            token.clone(),
            edit,
        )) {
            self.acknowledge_apply_edit(
                renderer_id,
                id,
                &token,
                false,
                Some("workspace edit notification could not be delivered".to_owned()),
            );
        }
        match receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(_) => self.cancel_timed_out_request(id, &receiver),
        }
    }

    fn claim_transaction(
        &self,
        renderer_id: u64,
        edit: &core::language_servers::StagedWorkspaceEdit,
    ) -> Result<(), core::language_servers::WorkspaceEditError> {
        self.workspace_edit_owner(renderer_id)
            .zip(self.application())
            .and_then(|(owner, application)| {
                application.lock().ok().map(|mut application| {
                    application.claim_workspace_edit_owner(
                        edit.transaction_id,
                        &edit.authorization,
                        owner,
                    )
                })
            })
            .unwrap_or(Ok(()))
    }

    fn resolve_acknowledgement(
        &self,
        pending: &PendingApplyEdit,
        applied: bool,
        failure_reason: Option<String>,
    ) -> core::events::WorkspaceEditApplication {
        if self.application().is_none() {
            return core::events::WorkspaceEditApplication {
                applied: applied && !pending.cancelled,
                failure_reason: if pending.cancelled {
                    Some("workspace edit application was cancelled".to_owned())
                } else {
                    failure_reason
                },
            };
        }
        let reason = if pending.cancelled {
            "workspace edit application was cancelled".to_owned()
        } else if applied {
            "workspace edit was not durably completed before acknowledgement".to_owned()
        } else {
            failure_reason.unwrap_or_else(|| "renderer rejected the workspace edit".to_owned())
        };
        self.resolve_interrupted_transaction(pending, &reason)
    }

    fn cancel_timed_out_request(
        &self,
        id: u64,
        receiver: &mpsc::Receiver<core::events::WorkspaceEditApplication>,
    ) -> core::events::WorkspaceEditApplication {
        let cancelled = self.subscribers.lock().ok().and_then(|mut subscribers| {
            let mut pending = subscribers.pending_apply_edits.remove(&id)?;
            pending.cancelled = true;
            let response = subscribers.responses.get(&pending.renderer_id).cloned();
            Some((pending, response))
        });
        let Some((pending, response)) = cancelled else {
            return receiver.recv().unwrap_or_else(|_| {
                apply_edit_failure("workspace edit acknowledgement outcome was unavailable")
            });
        };
        let result = if self.application().is_none() {
            apply_edit_failure(
                "workspace edit acknowledgement timed out and application was cancelled",
            )
        } else {
            self.resolve_interrupted_transaction(
                &pending,
                "workspace edit acknowledgement timed out and application was cancelled",
            )
        };
        if !result.applied
            && let Some(response) = response
        {
            let _ = response.try_send(ServerMessage::language_server_apply_edit_cancelled(
                id,
                pending.token.clone(),
            ));
        }
        let _ = pending.result.send(result.clone());
        result
    }

    fn resolve_interrupted_transaction(
        &self,
        pending: &PendingApplyEdit,
        reason: &str,
    ) -> core::events::WorkspaceEditApplication {
        let _publication = self
            .workspace_edit_publication
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.resolve_interrupted_transaction_under_gate(pending, reason)
    }

    fn resolve_interrupted_transaction_under_gate(
        &self,
        pending: &PendingApplyEdit,
        reason: &str,
    ) -> core::events::WorkspaceEditApplication {
        let owner = self.workspace_edit_owner(pending.renderer_id);
        let Some(application) = self.application() else {
            return apply_edit_failure(reason);
        };
        let mut application = application
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let status = match application
            .workspace_edit_status(pending.transaction_id, &pending.authorization)
        {
            Ok(status) => status,
            Err(error) => {
                return apply_edit_recovery_required(&format!(
                    "{reason}; durable transaction outcome is unavailable: {error}"
                ));
            }
        };
        match status.phase {
            core::language_servers::WorkspaceEditTransactionPhase::FinishedCommitted => {
                self.acknowledge_terminal_outcome(&mut application, pending);
                return workspace_edit_phase_application(
                    status.phase,
                    reason,
                    pending.transaction_id,
                )
                .expect("finished committed is a resolved application phase");
            }
            core::language_servers::WorkspaceEditTransactionPhase::FinishedRolledBack
            | core::language_servers::WorkspaceEditTransactionPhase::FinishedUncommitted => {
                self.acknowledge_terminal_outcome(&mut application, pending);
                return workspace_edit_phase_application(
                    status.phase,
                    reason,
                    pending.transaction_id,
                )
                .expect("finished rollback is a resolved application phase");
            }
            phase
                @ (core::language_servers::WorkspaceEditTransactionPhase::FinishingCommitted
                | core::language_servers::WorkspaceEditTransactionPhase::CommittedCleanupRequired
                | core::language_servers::WorkspaceEditTransactionPhase::RecoveryRequired) => {
                    return workspace_edit_phase_application(
                        phase,
                        reason,
                        pending.transaction_id,
                    )
                    .expect("recovery phases have an application response");
                }
            _ => {}
        }

        let Some(owner) = owner else {
            return apply_edit_recovery_required(&format!(
                "{reason}; workspace edit owner is unavailable"
            ));
        };
        let status = match application.cancel_owned_workspace_edit(
            pending.transaction_id,
            &pending.authorization,
            owner,
        ) {
            Ok(status) => status,
            Err(error) => {
                return apply_edit_recovery_required(&format!(
                    "{reason}; rollback requires recovery: {error}"
                ));
            }
        };
        if let Err(error) = self.persist_locked_application(&application) {
            return apply_edit_recovery_required(&format!("{reason}; {error}"));
        }
        match status.phase {
            core::language_servers::WorkspaceEditTransactionPhase::FinishedCommitted => {
                self.acknowledge_terminal_outcome(&mut application, pending);
                core::events::WorkspaceEditApplication {
                    applied: true,
                    failure_reason: None,
                }
            }
            core::language_servers::WorkspaceEditTransactionPhase::RolledBack => {
                if let Err(error) = application
                    .finish_workspace_edit(pending.transaction_id, &pending.authorization)
                    .and_then(|_| self.persist_locked_application(&application))
                {
                    return apply_edit_failure(&format!(
                        "{reason}; rollback completion requires recovery: {error}"
                    ));
                }
                self.acknowledge_terminal_outcome(&mut application, pending);
                apply_edit_failure(reason)
            }
            core::language_servers::WorkspaceEditTransactionPhase::FinishedRolledBack
            | core::language_servers::WorkspaceEditTransactionPhase::FinishedUncommitted => {
                self.acknowledge_terminal_outcome(&mut application, pending);
                apply_edit_failure(reason)
            }
            phase
                @ (core::language_servers::WorkspaceEditTransactionPhase::FinishingCommitted
                | core::language_servers::WorkspaceEditTransactionPhase::CommittedCleanupRequired
                | core::language_servers::WorkspaceEditTransactionPhase::RecoveryRequired) => {
                    workspace_edit_phase_application(phase, reason, pending.transaction_id)
                        .expect("recovery phases have an application response")
                }
            phase => apply_edit_failure(&format!(
                "{reason}; rollback did not reach a durable terminal outcome ({phase:?})"
            )),
        }
    }

    fn disconnect_renderers(&self, renderer_ids: &[u64], pending: Vec<PendingApplyEdit>) {
        if renderer_ids.is_empty() {
            debug_assert!(pending.is_empty());
            return;
        }
        let _publication = self
            .workspace_edit_publication
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for pending in pending {
            let result = self.resolve_interrupted_transaction_under_gate(
                &pending,
                "renderer disconnected while applying workspace edit",
            );
            let _ = pending.result.send(result);
        }
        let owners = renderer_ids
            .iter()
            .filter_map(|renderer_id| self.workspace_edit_owner(*renderer_id))
            .collect::<Vec<_>>();
        if let Some(application) = self.application() {
            let mut application = application
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let mut rolled_back = Vec::new();
            for owner in owners {
                rolled_back.extend(
                    application
                        .disconnect_workspace_edit_owner(owner)
                        .into_iter()
                        .filter(|status| {
                            status.phase
                                == core::language_servers::WorkspaceEditTransactionPhase::RolledBack
                        })
                        .map(|status| status.transaction_id),
                );
            }
            match application.persist_current_state() {
                Ok(()) => application.finish_disconnected_workspace_edit_rollbacks(&rolled_back),
                Err(error) => {
                    eprintln!("workspace edit disconnect rollback persistence failed: {error}");
                }
            }
        }
    }

    fn disconnect_renderers_async(&self, renderer_ids: Vec<u64>, pending: Vec<PendingApplyEdit>) {
        if renderer_ids.is_empty() {
            debug_assert!(pending.is_empty());
            return;
        }
        let hub = self.clone();
        if std::thread::Builder::new()
            .name("kosmos-workspace-edit-disconnect".to_owned())
            .spawn(move || hub.disconnect_renderers(&renderer_ids, pending))
            .is_err()
        {
            eprintln!("workspace edit disconnect worker failed to start");
        }
    }

    fn application(&self) -> Option<Arc<Mutex<core::Application>>> {
        self.application
            .lock()
            .ok()
            .and_then(|application| application.as_ref()?.upgrade())
    }

    fn workspace_edit_owner(&self, renderer_id: u64) -> Option<core::WorkspaceEditOwnerToken> {
        self.subscribers
            .lock()
            .ok()
            .and_then(|subscribers| subscribers.workspace_edit_owners.get(&renderer_id).copied())
    }

    fn acknowledge_terminal_outcome(
        &self,
        application: &mut core::Application,
        pending: &PendingApplyEdit,
    ) {
        if let Err(error) = application
            .acknowledge_workspace_edit_completion(pending.transaction_id, &pending.authorization)
        {
            eprintln!(
                "workspace edit {} completion cleanup failed: {error}",
                pending.transaction_id
            );
            return;
        }
        if let Err(error) = self.persist_locked_application(application) {
            eprintln!(
                "workspace edit {} completion state persistence failed: {error}",
                pending.transaction_id
            );
        }
    }

    fn persist_locked_application(
        &self,
        application: &core::Application,
    ) -> Result<(), core::language_servers::WorkspaceEditError> {
        application.persist_current_state().map_err(|error| {
            core::language_servers::WorkspaceEditError::Recovery(format!(
                "workspace edit rollback persistence failed: {error}"
            ))
        })
    }
}

impl core::events::CoreEventSink for NotificationHub {
    fn emit(&self, event: core::events::CoreEvent) {
        self.core_event(event);
    }

    fn apply_workspace_edit(
        &self,
        edit: core::language_servers::StagedWorkspaceEdit,
    ) -> core::events::WorkspaceEditApplication {
        self.request_apply_edit(edit)
    }
}

#[derive(Default)]
struct Subscribers {
    next_id: u64,
    responses: HashMap<u64, ResponseSender>,
    workspace_edit_owners: HashMap<u64, core::WorkspaceEditOwnerToken>,
    next_apply_edit_id: u64,
    pending_apply_edits: HashMap<u64, PendingApplyEdit>,
}

struct PendingApplyEdit {
    renderer_id: u64,
    token: String,
    cancelled: bool,
    transaction_id: u64,
    authorization: String,
    result: mpsc::SyncSender<core::events::WorkspaceEditApplication>,
}

impl Subscribers {
    fn remove_renderers(&mut self, renderer_ids: &[u64]) -> Vec<PendingApplyEdit> {
        for renderer_id in renderer_ids {
            self.responses.remove(renderer_id);
        }
        let pending_ids = self
            .pending_apply_edits
            .iter()
            .filter(|(_, pending)| renderer_ids.contains(&pending.renderer_id))
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        pending_ids
            .into_iter()
            .filter_map(|id| self.pending_apply_edits.remove(&id))
            .collect()
    }
}

fn apply_edit_failure(reason: &str) -> core::events::WorkspaceEditApplication {
    core::events::WorkspaceEditApplication {
        applied: false,
        failure_reason: Some(reason.to_owned()),
    }
}

fn apply_edit_recovery_required(reason: &str) -> core::events::WorkspaceEditApplication {
    core::events::WorkspaceEditApplication {
        applied: false,
        failure_reason: Some(reason.to_owned()),
    }
}

fn workspace_edit_phase_application(
    phase: core::language_servers::WorkspaceEditTransactionPhase,
    reason: &str,
    transaction_id: u64,
) -> Option<core::events::WorkspaceEditApplication> {
    use core::language_servers::WorkspaceEditTransactionPhase as Phase;

    match phase {
        Phase::FinishedCommitted => Some(core::events::WorkspaceEditApplication {
            applied: true,
            failure_reason: None,
        }),
        Phase::FinishingCommitted | Phase::CommittedCleanupRequired => {
            Some(core::events::WorkspaceEditApplication {
                applied: true,
                failure_reason: Some(committed_cleanup_failure_reason(reason, transaction_id)),
            })
        }
        Phase::FinishedRolledBack | Phase::FinishedUncommitted => Some(apply_edit_failure(reason)),
        Phase::RecoveryRequired => Some(apply_edit_recovery_required(&recovery_failure_reason(
            reason,
            transaction_id,
        ))),
        Phase::Staged | Phase::Committed | Phase::RolledBack => None,
    }
}

fn recovery_failure_reason(reason: &str, transaction_id: u64) -> String {
    format!(
        "{reason}; workspace edit transaction {transaction_id} requires recovery; retry rollback or explicitly finalize it"
    )
}

fn committed_cleanup_failure_reason(reason: &str, transaction_id: u64) -> String {
    format!(
        "{reason}; workspace edit transaction {transaction_id} is durably committed but cleanup is incomplete; retry finalize"
    )
}

pub(crate) struct NotificationSubscription {
    id: u64,
    workspace_edit_owner: Option<core::WorkspaceEditOwnerToken>,
    hub: NotificationHub,
}

impl NotificationSubscription {
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn workspace_edit_owner(&self) -> Option<core::WorkspaceEditOwnerToken> {
        self.workspace_edit_owner
    }
}

impl Drop for NotificationSubscription {
    fn drop(&mut self) {
        let pending = self
            .hub
            .subscribers
            .lock()
            .map(|mut subscribers| subscribers.remove_renderers(&[self.id]))
            .unwrap_or_default();
        self.hub.disconnect_renderers(&[self.id], pending);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::messages::envelope::ServerMessage;
    use crate::ipc::transport::response;
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[test]
    fn subscriptions_receive_workspace_changes_until_dropped() {
        let hub = NotificationHub::default();
        let (responses, receiver) = response_channel();
        let subscription = hub.subscribe(responses);

        hub.workspace_changed(vec![1]);
        assert!(matches!(
            receiver.recv_timeout(Duration::from_secs(1)),
            Ok(ServerMessage::Notification(_))
        ));

        drop(subscription);
        hub.workspace_changed(vec![2]);
        assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
    }

    #[test]
    fn apply_edit_notifications_wait_for_the_matching_acknowledgement() {
        let hub = NotificationHub::default();
        let (responses, receiver) = response_channel();
        let subscription = hub.subscribe(responses);
        let renderer_id = subscription.id();
        let request_hub = hub.clone();
        let request = std::thread::spawn(move || {
            request_hub.request_apply_edit(core::language_servers::StagedWorkspaceEdit {
                transaction_id: 9,
                authorization: "a".repeat(64),
                documents: Vec::new(),
                operations: Vec::new(),
            })
        });
        let notification = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("apply edit notification should arrive");
        let value = serde_json::to_value(notification).expect("notification should serialize");
        assert_eq!(value["event"], "languageServerApplyEdit");
        assert_eq!(value["edit"]["transactionId"], 9);
        let id = value["id"]
            .as_u64()
            .expect("notification should have an ID");
        let token = value["token"]
            .as_str()
            .expect("notification should have a token");

        hub.acknowledge_apply_edit(renderer_id + 1, id, token, true, None);
        hub.acknowledge_apply_edit(renderer_id, id, "wrong-token", true, None);
        assert!(!request.is_finished());
        hub.acknowledge_apply_edit(
            renderer_id,
            id,
            token,
            false,
            Some("renderer rejected it".to_owned()),
        );
        let result = request.join().expect("apply edit request should finish");
        assert!(!result.applied);
        assert_eq!(
            result.failure_reason.as_deref(),
            Some("renderer rejected it")
        );
    }

    #[test]
    fn renderer_disconnect_immediately_fails_pending_apply_edit() {
        let hub = NotificationHub::default();
        let (responses, receiver) = response_channel();
        let subscription = hub.subscribe(responses);
        let request_hub = hub.clone();
        let request = std::thread::spawn(move || {
            request_hub.request_apply_edit(core::language_servers::StagedWorkspaceEdit {
                transaction_id: 10,
                authorization: "b".repeat(64),
                documents: Vec::new(),
                operations: Vec::new(),
            })
        });
        receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("apply edit notification should arrive");

        drop(subscription);

        let result = request.join().expect("disconnect should release request");
        assert!(!result.applied);
        assert_eq!(
            result.failure_reason.as_deref(),
            Some("renderer disconnected while applying workspace edit")
        );
    }

    #[test]
    fn timeout_cancels_the_owner_before_reporting_failure() {
        let hub = NotificationHub::default();
        let (responses, receiver) = response_channel();
        let subscription = hub.subscribe(responses);
        let renderer_id = subscription.id();
        let request_hub = hub.clone();
        let request = std::thread::spawn(move || {
            request_hub.request_apply_edit_with_timeout(
                core::language_servers::StagedWorkspaceEdit {
                    transaction_id: 11,
                    authorization: "c".repeat(64),
                    documents: Vec::new(),
                    operations: Vec::new(),
                },
                Duration::from_millis(25),
            )
        });
        let apply = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("apply edit notification should arrive");
        let apply = serde_json::to_value(apply).unwrap();
        let id = apply["id"].as_u64().unwrap();
        let token = apply["token"].as_str().unwrap().to_owned();
        let cancellation = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("cancellation should arrive");
        let cancellation = serde_json::to_value(cancellation).unwrap();
        assert_eq!(cancellation["event"], "languageServerApplyEditCancelled");
        assert_eq!(cancellation["token"], token);
        let result = request.join().unwrap();
        hub.acknowledge_apply_edit(renderer_id, id, &token, false, Some("stopped".to_owned()));
        assert!(!result.applied);
        assert_eq!(
            result.failure_reason.as_deref(),
            Some("workspace edit acknowledgement timed out and application was cancelled")
        );
    }

    #[test]
    fn every_workspace_edit_phase_has_safe_apply_edit_response_semantics() {
        use core::language_servers::WorkspaceEditTransactionPhase as Phase;

        let cases = [
            (Phase::Staged, None),
            (Phase::Committed, None),
            (Phase::RolledBack, None),
            (Phase::FinishingCommitted, Some((true, true))),
            (Phase::CommittedCleanupRequired, Some((true, true))),
            (Phase::RecoveryRequired, Some((false, true))),
            (Phase::FinishedCommitted, Some((true, false))),
            (Phase::FinishedRolledBack, Some((false, true))),
            (Phase::FinishedUncommitted, Some((false, true))),
        ];

        for (phase, expected) in cases {
            let application = workspace_edit_phase_application(phase, "interrupted", 42);
            assert_eq!(
                application
                    .as_ref()
                    .map(|result| (result.applied, result.failure_reason.is_some())),
                expected,
                "unexpected workspace/applyEdit response for {phase:?}"
            );
            if phase == Phase::RecoveryRequired {
                assert!(
                    application
                        .as_ref()
                        .unwrap()
                        .failure_reason
                        .as_ref()
                        .unwrap()
                        .contains("retry rollback")
                );
            }
            if matches!(
                phase,
                Phase::FinishingCommitted | Phase::CommittedCleanupRequired
            ) {
                let application = application.as_ref().unwrap();
                assert!(
                    !application
                        .failure_reason
                        .as_ref()
                        .unwrap()
                        .contains("rollback")
                );
            }
        }
    }

    #[test]
    fn disconnect_after_finished_commit_reports_applied_and_acknowledges_cleanup() {
        let root = std::env::temp_dir().join(format!(
            "kosmos-notification-apply-edit-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should follow the Unix epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("test workspace should be created");
        let database = root.join("state.sqlite");
        let store = core::DurableStore::open(&database).expect("store should open");
        let manager = core::language_servers::LanguageServerManager::open(
            core::language_servers::LanguageServerPaths::new(
                root.join("language-servers"),
                root.join("language-server-cache"),
            ),
            store.clone(),
        )
        .expect("language server manager should open");
        let mut state = core::State::new();
        let workspace_id = state.open_workspace(&root);
        let staged = manager
            .stage_workspace_edit(
                &serde_json::json!({}),
                &[core::language_servers::WorkspaceEditRoot {
                    workspace_id,
                    path: root.clone(),
                }],
            )
            .expect("workspace edit should stage");
        state.attach_language_server_manager(manager);
        let application = Arc::new(Mutex::new(core::Application::new(state, store.clone())));
        let hub = NotificationHub::default();
        hub.attach_application(Arc::downgrade(&application));
        let (responses, receiver) = response_channel();
        let subscription = hub.subscribe(responses);
        let renderer_id = subscription.id();
        let request_hub = hub.clone();
        let requested = staged.clone();
        let request = std::thread::spawn(move || request_hub.request_apply_edit(requested));
        let notification = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("renderer should receive apply-edit notification");
        let notification = serde_json::to_value(notification).expect("notification should encode");
        let id = notification["id"]
            .as_u64()
            .expect("notification should have an ID");
        let token = notification["token"]
            .as_str()
            .expect("notification should have a token");

        {
            let mut state = application.lock().expect("state should lock");
            state
                .commit_workspace_edit(staged.transaction_id, &staged.authorization)
                .expect("renderer commit should succeed");
            store.save(&state).expect("committed state should persist");
            state
                .finish_workspace_edit(staged.transaction_id, &staged.authorization)
                .expect("renderer finish should succeed");
            assert_eq!(
                state
                    .workspace_edit_status(staged.transaction_id, &staged.authorization)
                    .expect("terminal outcome must remain visible through renderer completion")
                    .phase,
                core::language_servers::WorkspaceEditTransactionPhase::FinishedCommitted
            );
        }

        drop(subscription);
        let result = request.join().expect("apply-edit request should finish");
        assert!(result.applied);
        assert_eq!(result.failure_reason, None);
        hub.acknowledge_apply_edit(renderer_id, id, token, true, None);
        let mut state = application
            .lock()
            .expect("state should lock after acknowledgement");
        assert!(state.workspace_edit_recoveries().unwrap().is_empty());
        assert!(matches!(
            state.workspace_edit_status(staged.transaction_id, &staged.authorization),
            Err(core::language_servers::WorkspaceEditError::Expired)
        ));
        assert!(
            state
                .acknowledge_workspace_edit_completion(staged.transaction_id, &staged.authorization)
                .expect("acknowledgement tombstone should make cleanup retries idempotent")
        );
        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn disconnect_before_commit_durably_rolls_back_before_reporting_failure() {
        let (root, store, state, staged) = staged_apply_edit("disconnect-before-commit", false);
        let state = Arc::new(Mutex::new(core::Application::new(state, store.clone())));
        let hub = NotificationHub::default();
        hub.attach_application(Arc::downgrade(&state));
        let (responses, receiver) = response_channel();
        let subscription = hub.subscribe(responses);
        let renderer_id = subscription.id();
        let request_hub = hub.clone();
        let requested = staged.clone();
        let request = std::thread::spawn(move || request_hub.request_apply_edit(requested));
        let notification = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("renderer should receive apply-edit notification");
        let notification = serde_json::to_value(notification).unwrap();
        let id = notification["id"].as_u64().unwrap();
        let token = notification["token"].as_str().unwrap().to_owned();

        drop(subscription);
        let application = request
            .join()
            .expect("disconnect should resolve apply-edit");
        assert!(!application.applied);
        assert_eq!(
            application.failure_reason.as_deref(),
            Some("renderer disconnected while applying workspace edit")
        );
        assert!(matches!(
            state
                .lock()
                .unwrap()
                .workspace_edit_status(staged.transaction_id, &staged.authorization),
            Err(core::language_servers::WorkspaceEditError::Expired)
        ));
        hub.acknowledge_apply_edit(renderer_id, id, &token, true, None);
        assert_eq!(
            std::fs::read_to_string(root.join("target.txt")).unwrap(),
            "before"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn disconnect_during_recovery_reports_recovery_and_retains_transaction() {
        let (root, store, state, staged) = staged_apply_edit("disconnect-during-recovery", true);
        let state = Arc::new(Mutex::new(core::Application::new(state, store)));
        let hub = NotificationHub::default();
        hub.attach_application(Arc::downgrade(&state));
        let (responses, receiver) = response_channel();
        let subscription = hub.subscribe(responses);
        let request_hub = hub.clone();
        let requested = staged.clone();
        let request = std::thread::spawn(move || request_hub.request_apply_edit(requested));
        receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("renderer should receive apply-edit notification");

        {
            let mut state = state.lock().unwrap();
            state
                .commit_workspace_edit(staged.transaction_id, &staged.authorization)
                .expect("closed edit should commit");
            std::fs::write(root.join("target.txt"), "changed after commit").unwrap();
            assert!(matches!(
                state.rollback_workspace_edit(staged.transaction_id, &staged.authorization),
                Err(core::language_servers::WorkspaceEditError::Recovery(_))
            ));
        }

        drop(subscription);
        let application = request
            .join()
            .expect("disconnect should resolve apply-edit");
        assert!(!application.applied);
        assert!(
            application
                .failure_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("requires recovery"))
        );
        let mut state = state.lock().unwrap();
        assert_eq!(
            state
                .workspace_edit_status(staged.transaction_id, &staged.authorization)
                .unwrap()
                .phase,
            core::language_servers::WorkspaceEditTransactionPhase::RecoveryRequired
        );
        let owner = state.workspace_edit_owner_token();
        state
            .claim_workspace_edit_owner(staged.transaction_id, &staged.authorization, owner)
            .expect("disconnect should release recovery ownership without retrying rollback");
        drop(state);
        assert_eq!(
            std::fs::read_to_string(root.join("target.txt")).unwrap(),
            "changed after commit"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    fn staged_apply_edit(
        name: &str,
        overwrite: bool,
    ) -> (
        std::path::PathBuf,
        core::DurableStore,
        core::State,
        core::language_servers::StagedWorkspaceEdit,
    ) {
        let root = std::env::temp_dir().join(format!(
            "kosmos-notification-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("target.txt"), "before").unwrap();
        let store = core::DurableStore::open(root.join("state.sqlite")).unwrap();
        let manager = core::language_servers::LanguageServerManager::open(
            core::language_servers::LanguageServerPaths::new(
                root.join("language-servers"),
                root.join("language-server-cache"),
            ),
            store.clone(),
        )
        .unwrap();
        let mut state = core::State::new();
        let workspace_id = state.open_workspace(&root);
        let edit = if overwrite {
            serde_json::json!({ "documentChanges": [{
                "kind": "create",
                "uri": format!("file://{}", root.join("target.txt").display()),
                "options": { "overwrite": true }
            }]})
        } else {
            serde_json::json!({})
        };
        let staged = manager
            .stage_workspace_edit(
                &edit,
                &[core::language_servers::WorkspaceEditRoot {
                    workspace_id,
                    path: root.clone(),
                }],
            )
            .unwrap();
        state.attach_language_server_manager(manager);
        store.save(&state).unwrap();
        (root, store, state, staged)
    }

    fn response_channel() -> (ResponseSender, response::ResponseReceiver) {
        let (stream, _peer) = UnixStream::pair().expect("socket pair should open");
        let (responses, receiver, _) =
            response::channel(&stream).expect("response channel should open");

        (responses, receiver)
    }
}
