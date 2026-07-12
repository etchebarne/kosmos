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
    state: Arc<Mutex<Option<Weak<Mutex<core::State>>>>>,
}

impl NotificationHub {
    pub(crate) fn attach_state(&self, state: Weak<Mutex<core::State>>) {
        *self.state.lock().unwrap_or_else(|error| error.into_inner()) = Some(state);
    }

    pub(crate) fn subscribe(&self, responses: ResponseSender) -> NotificationSubscription {
        let mut subscribers = self
            .subscribers
            .lock()
            .expect("notification subscribers should lock");
        let id = subscribers.next_id;

        subscribers.next_id = subscribers.next_id.wrapping_add(1);
        subscribers.responses.insert(id, responses);

        NotificationSubscription {
            id,
            subscribers: Arc::clone(&self.subscribers),
            state: Arc::clone(&self.state),
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
        self.disconnect_renderers(&disconnected, pending);
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
        self.disconnect_renderers(&disconnected, pending);
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
        owner: u64,
        edit: &core::language_servers::StagedWorkspaceEdit,
    ) -> Result<(), core::language_servers::WorkspaceEditError> {
        self.with_state(|state| {
            state.claim_workspace_edit_owner(edit.transaction_id, &edit.authorization, owner)
        })
        .unwrap_or(Ok(()))
    }

    fn resolve_acknowledgement(
        &self,
        pending: &PendingApplyEdit,
        applied: bool,
        failure_reason: Option<String>,
    ) -> core::events::WorkspaceEditApplication {
        if self.state().is_none() {
            return core::events::WorkspaceEditApplication {
                applied: applied && !pending.cancelled,
                failure_reason: if pending.cancelled {
                    Some("workspace edit application was cancelled".to_owned())
                } else {
                    failure_reason
                },
            };
        }
        if self.transaction_finished_committed(pending) {
            return core::events::WorkspaceEditApplication {
                applied: true,
                failure_reason: None,
            };
        }
        if pending.cancelled || !applied {
            let _ = self.cancel_transaction(pending);
        }
        core::events::WorkspaceEditApplication {
            applied: false,
            failure_reason: Some(if pending.cancelled {
                "workspace edit application was cancelled".to_owned()
            } else if applied {
                "workspace edit was not durably completed before acknowledgement".to_owned()
            } else {
                failure_reason.unwrap_or_else(|| "renderer rejected the workspace edit".to_owned())
            }),
        }
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
        let result = if self.state().is_none() {
            apply_edit_failure(
                "workspace edit acknowledgement timed out and application was cancelled",
            )
        } else {
            match self.cancel_transaction(&pending) {
                Ok(status)
                    if status.phase
                        == core::language_servers::WorkspaceEditTransactionPhase::FinishedCommitted =>
                {
                    core::events::WorkspaceEditApplication {
                        applied: true,
                        failure_reason: None,
                    }
                }
                Ok(_) => apply_edit_failure(
                    "workspace edit acknowledgement timed out and application was cancelled",
                ),
                Err(error) => apply_edit_failure(&format!(
                    "workspace edit acknowledgement timed out; rollback requires recovery: {error}"
                )),
            }
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

    fn transaction_finished_committed(&self, pending: &PendingApplyEdit) -> bool {
        self.with_state(|state| {
            state.workspace_edit_status(pending.transaction_id, &pending.authorization)
        })
        .and_then(Result::ok)
        .is_some_and(|status| {
            status.phase == core::language_servers::WorkspaceEditTransactionPhase::FinishedCommitted
        })
    }

    fn cancel_transaction(
        &self,
        pending: &PendingApplyEdit,
    ) -> Result<
        core::language_servers::WorkspaceEditTransactionStatus,
        core::language_servers::WorkspaceEditError,
    > {
        self.with_state(|state| {
            state.cancel_owned_workspace_edit(
                pending.transaction_id,
                &pending.authorization,
                pending.renderer_id,
            )
        })
        .unwrap_or(Err(core::language_servers::WorkspaceEditError::Invalid(
            "workspace edit state is unavailable".to_owned(),
        )))
    }

    fn disconnect_renderers(&self, renderer_ids: &[u64], pending: Vec<PendingApplyEdit>) {
        if renderer_ids.is_empty() {
            debug_assert!(pending.is_empty());
            return;
        }
        if let Some(state) = self.state() {
            let state = state.lock().unwrap_or_else(|error| error.into_inner());
            for renderer_id in renderer_ids {
                state.disconnect_workspace_edit_owner(*renderer_id);
            }
        }
        fail_pending_apply_edits(
            pending,
            "renderer disconnected while applying workspace edit",
        );
    }

    fn state(&self) -> Option<Arc<Mutex<core::State>>> {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.as_ref()?.upgrade())
    }

    fn with_state<T>(&self, operation: impl FnOnce(&core::State) -> T) -> Option<T> {
        let state = self.state()?;
        let state = state.lock().ok()?;
        Some(operation(&state))
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

fn fail_pending_apply_edits(pending: Vec<PendingApplyEdit>, reason: &str) {
    for pending in pending {
        let _ = pending.result.send(apply_edit_failure(reason));
    }
}

fn apply_edit_failure(reason: &str) -> core::events::WorkspaceEditApplication {
    core::events::WorkspaceEditApplication {
        applied: false,
        failure_reason: Some(reason.to_owned()),
    }
}

pub(crate) struct NotificationSubscription {
    id: u64,
    subscribers: Arc<Mutex<Subscribers>>,
    state: Arc<Mutex<Option<Weak<Mutex<core::State>>>>>,
}

impl NotificationSubscription {
    pub(crate) fn id(&self) -> u64 {
        self.id
    }
}

impl Drop for NotificationSubscription {
    fn drop(&mut self) {
        let pending = self
            .subscribers
            .lock()
            .map(|mut subscribers| subscribers.remove_renderers(&[self.id]))
            .unwrap_or_default();
        if let Some(state) = self
            .state
            .lock()
            .ok()
            .and_then(|state| state.as_ref()?.upgrade())
        {
            state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .disconnect_workspace_edit_owner(self.id);
        }
        fail_pending_apply_edits(
            pending,
            "renderer disconnected while applying workspace edit",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::messages::envelope::ServerMessage;
    use crate::ipc::transport::response;
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

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

    fn response_channel() -> (ResponseSender, response::ResponseReceiver) {
        let (stream, _peer) = UnixStream::pair().expect("socket pair should open");
        let (responses, receiver, _) =
            response::channel(&stream).expect("response channel should open");

        (responses, receiver)
    }
}
