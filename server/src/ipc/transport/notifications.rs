use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::time::Duration;

use super::response::ResponseSender;
use crate::ipc::messages::envelope::ServerMessage;

const MAX_PENDING_APPLY_EDITS: usize = 16;
const APPLY_EDIT_ACK_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Default)]
pub(crate) struct NotificationHub {
    subscribers: Arc<(Mutex<Subscribers>, std::sync::Condvar)>,
    application: Arc<Mutex<Option<Weak<Mutex<core::Application>>>>>,
}

impl NotificationHub {
    pub(crate) fn attach_application(&self, application: Weak<Mutex<core::Application>>) {
        *self
            .application
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(application);
    }

    pub(crate) fn subscribe(&self, responses: ResponseSender) -> NotificationSubscription {
        let mut subscribers = self
            .subscribers
            .0
            .lock()
            .expect("notification subscribers should lock");
        let id = subscribers.next_id;
        subscribers.next_id = subscribers.next_id.wrapping_add(1).max(1);
        subscribers.responses.insert(id, responses);
        self.subscribers.1.notify_all();
        NotificationSubscription {
            id,
            hub: self.clone(),
        }
    }

    pub(crate) fn workspace_changed(&self, workspace_ids: Vec<u64>) {
        let disconnected = self.subscribers.0.lock().ok().map(|mut subscribers| {
            let disconnected = subscribers
                .responses
                .iter()
                .filter(|(_, responses)| !responses.notify_workspace_changed(&workspace_ids))
                .map(|(id, _)| *id)
                .collect::<Vec<_>>();
            let pending = subscribers.remove_renderers(&disconnected);
            (disconnected, pending)
        });
        if let Some((renderers, pending)) = disconnected {
            self.resolve_disconnected(renderers, pending);
        }
    }

    pub(crate) fn acknowledge_apply_edit(
        &self,
        renderer_id: u64,
        id: u64,
        token: &str,
        applied: bool,
        failure_reason: Option<String>,
    ) {
        let pending = self.subscribers.0.lock().ok().and_then(|mut subscribers| {
            let pending = subscribers.pending_apply_edits.get(&id)?;
            if pending.renderer_id != renderer_id || pending.token != token {
                return None;
            }
            let pending = subscribers.pending_apply_edits.remove(&id)?;
            subscribers.release_lease(renderer_id, pending.delivery.lease);
            Some(pending)
        });
        if let Some(pending) = pending {
            let outcome = if applied {
                core::language_servers::WorkspaceEditDeliveryOutcome::Applied
            } else {
                core::language_servers::WorkspaceEditDeliveryOutcome::Rejected(
                    failure_reason
                        .unwrap_or_else(|| "renderer rejected the workspace edit".to_owned()),
                )
            };
            let _ = pending.result.send(outcome);
        }
    }

    pub(crate) fn request_apply_edit(
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
        let Some(application) = self.application() else {
            return apply_edit_failure("workspace edit application is unavailable");
        };
        let first = application
            .lock()
            .map(|mut application| application.prepare_workspace_edit_delivery(edit))
            .unwrap_or_else(|_| apply_edit_complete("workspace edit application is unavailable"));
        self.drive_delivery(first, timeout)
    }

    pub(crate) fn request_staged_workspace_edit(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<core::events::WorkspaceEditApplication, core::language_servers::WorkspaceEditError>
    {
        let application = self.application().ok_or_else(|| {
            core::language_servers::WorkspaceEditError::Invalid(
                "workspace edit application is unavailable".to_owned(),
            )
        })?;
        let first = application
            .lock()
            .map_err(|_| {
                core::language_servers::WorkspaceEditError::Invalid(
                    "workspace edit application is unavailable".to_owned(),
                )
            })?
            .prepare_staged_workspace_edit_delivery(transaction_id, authorization)?;
        Ok(self.drive_delivery(first, APPLY_EDIT_ACK_TIMEOUT))
    }

    fn drive_delivery(
        &self,
        mut step: core::language_servers::WorkspaceEditDeliveryStep,
        timeout: Duration,
    ) -> core::events::WorkspaceEditApplication {
        loop {
            let delivery = match step {
                core::language_servers::WorkspaceEditDeliveryStep::Complete(result) => {
                    return result;
                }
                core::language_servers::WorkspaceEditDeliveryStep::Deliver(delivery) => delivery,
            };
            let outcome = self.deliver(delivery.clone(), timeout);
            let Some(application) = self.application() else {
                return apply_edit_failure("workspace edit application is unavailable");
            };
            step = match application.lock() {
                Ok(mut application) => application.complete_workspace_edit_delivery(
                    delivery.lease,
                    delivery.step,
                    outcome,
                ),
                Err(_) => return apply_edit_failure("workspace edit application is unavailable"),
            }
            .unwrap_or_else(|error| apply_edit_complete(&error.to_string()));
        }
    }

    fn deliver(
        &self,
        delivery: core::language_servers::PendingWorkspaceEditDelivery,
        timeout: Duration,
    ) -> core::language_servers::WorkspaceEditDeliveryOutcome {
        let (result, receiver) = mpsc::sync_channel(1);
        let token = delivery.lease.token();
        let selected = {
            let Ok(mut subscribers) = self.subscribers.0.lock() else {
                return core::language_servers::WorkspaceEditDeliveryOutcome::NotDelivered(
                    "workspace edit notification hub is unavailable".to_owned(),
                );
            };
            if subscribers.responses.is_empty() {
                let (next, wait) = self
                    .subscribers
                    .1
                    .wait_timeout_while(subscribers, timeout, |subscribers| {
                        subscribers.responses.is_empty()
                    })
                    .unwrap_or_else(|error| error.into_inner());
                subscribers = next;
                if wait.timed_out() && subscribers.responses.is_empty() {
                    return core::language_servers::WorkspaceEditDeliveryOutcome::NotDelivered(
                        "no renderer is connected to apply the workspace edit".to_owned(),
                    );
                }
            }
            if subscribers.pending_apply_edits.len() >= MAX_PENDING_APPLY_EDITS {
                return core::language_servers::WorkspaceEditDeliveryOutcome::NotDelivered(
                    "too many workspace edit acknowledgements are pending".to_owned(),
                );
            }
            let Some((renderer_id, response)) = subscribers
                .responses
                .iter()
                .next()
                .map(|(id, response)| (*id, response.clone()))
            else {
                return core::language_servers::WorkspaceEditDeliveryOutcome::NotDelivered(
                    "no renderer is connected to apply the workspace edit".to_owned(),
                );
            };
            let id = subscribers.next_apply_edit_id;
            subscribers.next_apply_edit_id = id.wrapping_add(1).max(1);
            subscribers.pending_apply_edits.insert(
                id,
                PendingApplyEdit {
                    renderer_id,
                    token: token.clone(),
                    delivery: delivery.clone(),
                    result,
                },
            );
            subscribers
                .renderer_leases
                .entry(renderer_id)
                .or_default()
                .insert(delivery.lease);
            (id, renderer_id, response)
        };
        let (id, renderer_id, response) = selected;
        if !response.try_send(ServerMessage::language_server_apply_edit(
            id,
            token.clone(),
            delivery.edit,
            delivery.directive,
        )) {
            return self.remove_pending(
                renderer_id,
                id,
                core::language_servers::WorkspaceEditDeliveryOutcome::NotDelivered(
                    "workspace edit notification could not be delivered".to_owned(),
                ),
            );
        }
        match receiver.recv_timeout(timeout) {
            Ok(outcome) => outcome,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let outcome = core::language_servers::WorkspaceEditDeliveryOutcome::TimedOut;
                let outcome = self.remove_pending(renderer_id, id, outcome);
                let _ = response.try_send(ServerMessage::language_server_apply_edit_cancelled(
                    id, token,
                ));
                outcome
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                core::language_servers::WorkspaceEditDeliveryOutcome::RendererDisconnected
            }
        }
    }

    fn remove_pending(
        &self,
        renderer_id: u64,
        id: u64,
        fallback: core::language_servers::WorkspaceEditDeliveryOutcome,
    ) -> core::language_servers::WorkspaceEditDeliveryOutcome {
        self.subscribers
            .0
            .lock()
            .ok()
            .and_then(|mut subscribers| {
                let pending = subscribers.pending_apply_edits.remove(&id)?;
                if pending.renderer_id != renderer_id {
                    subscribers.pending_apply_edits.insert(id, pending);
                    return None;
                }
                subscribers.release_lease(renderer_id, pending.delivery.lease);
                Some(fallback.clone())
            })
            .unwrap_or(fallback)
    }

    fn resolve_disconnected(&self, _renderers: Vec<u64>, pending: Vec<PendingApplyEdit>) {
        for pending in pending {
            let _ = pending
                .result
                .send(core::language_servers::WorkspaceEditDeliveryOutcome::RendererDisconnected);
        }
    }

    fn core_event(&self, event: core::events::CoreEvent) {
        let disconnected = self.subscribers.0.lock().ok().map(|mut subscribers| {
            let disconnected = subscribers
                .responses
                .iter()
                .filter(|(_, responses)| !responses.notify_core_event(event.clone()))
                .map(|(id, _)| *id)
                .collect::<Vec<_>>();
            let pending = subscribers.remove_renderers(&disconnected);
            (disconnected, pending)
        });
        if let Some((renderers, pending)) = disconnected {
            self.resolve_disconnected(renderers, pending);
        }
    }

    fn application(&self) -> Option<Arc<Mutex<core::Application>>> {
        self.application
            .lock()
            .ok()
            .and_then(|application| application.as_ref()?.upgrade())
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
    renderer_leases: HashMap<u64, HashSet<core::language_servers::WorkspaceEditDeliveryLease>>,
}

struct PendingApplyEdit {
    renderer_id: u64,
    token: String,
    delivery: core::language_servers::PendingWorkspaceEditDelivery,
    result: mpsc::SyncSender<core::language_servers::WorkspaceEditDeliveryOutcome>,
}

impl Subscribers {
    fn remove_renderers(&mut self, renderer_ids: &[u64]) -> Vec<PendingApplyEdit> {
        for renderer_id in renderer_ids {
            self.responses.remove(renderer_id);
            self.renderer_leases.remove(renderer_id);
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

    fn release_lease(
        &mut self,
        renderer_id: u64,
        lease: core::language_servers::WorkspaceEditDeliveryLease,
    ) {
        let Some(leases) = self.renderer_leases.get_mut(&renderer_id) else {
            return;
        };
        leases.remove(&lease);
        if leases.is_empty() {
            self.renderer_leases.remove(&renderer_id);
        }
    }
}

fn apply_edit_failure(reason: &str) -> core::events::WorkspaceEditApplication {
    core::events::WorkspaceEditApplication {
        applied: false,
        failure_reason: Some(reason.to_owned()),
    }
}

fn apply_edit_complete(reason: &str) -> core::language_servers::WorkspaceEditDeliveryStep {
    core::language_servers::WorkspaceEditDeliveryStep::Complete(apply_edit_failure(reason))
}

pub(crate) struct NotificationSubscription {
    id: u64,
    hub: NotificationHub,
}

impl NotificationSubscription {
    pub(crate) fn id(&self) -> u64 {
        self.id
    }
}

impl Drop for NotificationSubscription {
    fn drop(&mut self) {
        let pending = self
            .hub
            .subscribers
            .0
            .lock()
            .map(|mut subscribers| subscribers.remove_renderers(&[self.id]))
            .unwrap_or_default();
        self.hub.resolve_disconnected(vec![self.id], pending);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::transport::response;
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn deliveries_use_an_opaque_lease_and_progress_through_reconciliation() {
        let fixture = Fixture::new("success");
        let hub = NotificationHub::default();
        hub.attach_application(Arc::downgrade(&fixture.application));
        let (responses, receiver) = response_channel();
        let subscription = hub.subscribe(responses);
        let renderer_id = subscription.id();
        let requested = fixture.staged.clone();
        let requested_hub = hub.clone();
        let request = std::thread::spawn(move || requested_hub.request_apply_edit(requested));

        let apply = notification(&receiver);
        assert_eq!(apply["edit"]["directive"]["kind"], "applyOpenModels");
        assert_ne!(apply["token"], fixture.staged.authorization);
        acknowledge(&hub, renderer_id, &apply, true);

        let reconcile = notification(&receiver);
        assert_eq!(
            reconcile["edit"]["directive"]["kind"],
            "reconcileCommittedModels"
        );
        acknowledge(&hub, renderer_id, &reconcile, true);

        assert!(request.join().unwrap().applied);
    }

    #[test]
    fn rejected_apply_delivers_undo_before_a_rolled_back_reconciliation() {
        let fixture = Fixture::new("undo");
        let hub = NotificationHub::default();
        hub.attach_application(Arc::downgrade(&fixture.application));
        let (responses, receiver) = response_channel();
        let subscription = hub.subscribe(responses);
        let renderer_id = subscription.id();
        let requested_hub = hub.clone();
        let staged = fixture.staged.clone();
        let request = std::thread::spawn(move || requested_hub.request_apply_edit(staged));

        let apply = notification(&receiver);
        acknowledge(&hub, renderer_id, &apply, false);
        let undo = notification(&receiver);
        assert_eq!(undo["edit"]["directive"]["kind"], "undoOpenModels");
        acknowledge(&hub, renderer_id, &undo, true);
        let reconcile = notification(&receiver);
        assert_eq!(
            reconcile["edit"]["directive"]["kind"],
            "reconcileRolledBackModels"
        );
        acknowledge(&hub, renderer_id, &reconcile, true);

        assert!(!request.join().unwrap().applied);
        assert_eq!(std::fs::read_to_string(fixture.path()).unwrap(), "before");
    }

    #[test]
    fn timeout_is_reported_to_core_and_never_waits_with_the_application_mutex_held() {
        let fixture = Fixture::new("timeout");
        let hub = NotificationHub::default();
        hub.attach_application(Arc::downgrade(&fixture.application));
        let (responses, receiver) = response_channel();
        let _subscription = hub.subscribe(responses);
        let requested_hub = hub.clone();
        let staged = fixture.staged.clone();
        let request = std::thread::spawn(move || {
            requested_hub.request_apply_edit_with_timeout(staged, Duration::from_millis(10))
        });

        let _apply = notification(&receiver);
        assert!(fixture.application.try_lock().is_ok());
        let result = request.join().unwrap();
        assert!(!result.applied);
        assert!(result.failure_reason.unwrap().contains("timed out"));
    }

    #[test]
    fn renderer_disconnect_is_delivered_as_a_transport_fact() {
        let fixture = Fixture::new("disconnect");
        let hub = NotificationHub::default();
        hub.attach_application(Arc::downgrade(&fixture.application));
        let (responses, receiver) = response_channel();
        let subscription = hub.subscribe(responses);
        let requested_hub = hub.clone();
        let staged = fixture.staged.clone();
        let request = std::thread::spawn(move || requested_hub.request_apply_edit(staged));

        let _apply = notification(&receiver);
        drop(subscription);

        let result = request.join().unwrap();
        assert!(!result.applied);
        assert_eq!(std::fs::read_to_string(fixture.path()).unwrap(), "before");
    }

    fn acknowledge(
        hub: &NotificationHub,
        renderer_id: u64,
        notification: &serde_json::Value,
        applied: bool,
    ) {
        hub.acknowledge_apply_edit(
            renderer_id,
            notification["id"].as_u64().unwrap(),
            notification["token"].as_str().unwrap(),
            applied,
            (!applied).then(|| "renderer rejected directive".to_owned()),
        );
    }

    fn notification(receiver: &response::ResponseReceiver) -> serde_json::Value {
        serde_json::to_value(
            receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("workspace edit notification should arrive"),
        )
        .unwrap()
    }

    fn response_channel() -> (ResponseSender, response::ResponseReceiver) {
        let (stream, _peer) = UnixStream::pair().unwrap();
        let (responses, receiver, _) = response::channel(&stream).unwrap();
        (responses, receiver)
    }

    struct Fixture {
        root: std::path::PathBuf,
        application: Arc<Mutex<core::Application>>,
        staged: core::language_servers::StagedWorkspaceEdit,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "kosmos-server-workspace-edit-{name}-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed),
            ));
            std::fs::create_dir_all(&root).unwrap();
            let path = root.join("target.txt");
            std::fs::write(&path, "before").unwrap();
            let store = core::DurableStore::open(root.join("state.sqlite3")).unwrap();
            let manager = core::language_servers::LanguageServerManager::open(
                core::language_servers::LanguageServerPaths::new(
                    root.join("language-servers"),
                    root.join("cache"),
                ),
                store.clone(),
            )
            .unwrap();
            let mut state = core::State::new();
            let workspace_id = state.open_workspace(&root);
            let staged = manager
                .stage_workspace_edit(
                    &serde_json::json!({ "changes": {
                        format!("file://{}", path.display()): [{
                            "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 6 } },
                            "newText": "after"
                        }]
                    }}),
                    &[core::language_servers::WorkspaceEditRoot {
                        workspace_id,
                        path: root.clone(),
                    }],
                )
                .unwrap();
            state.attach_language_server_manager(manager);
            Self {
                root,
                application: Arc::new(Mutex::new(core::Application::new(state, store))),
                staged,
            }
        }

        fn path(&self) -> std::path::PathBuf {
            self.root.join("target.txt")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}
