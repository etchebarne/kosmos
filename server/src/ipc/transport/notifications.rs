use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::response::ResponseSender;

#[derive(Clone, Default)]
pub(crate) struct NotificationHub {
    subscribers: Arc<Mutex<Subscribers>>,
}

impl NotificationHub {
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
        }
    }

    pub(crate) fn workspace_changed(&self, workspace_ids: Vec<u64>) {
        let Ok(mut subscribers) = self.subscribers.lock() else {
            return;
        };

        subscribers
            .responses
            .retain(|_, responses| responses.notify_workspace_changed(&workspace_ids));
    }
}

#[derive(Default)]
struct Subscribers {
    next_id: u64,
    responses: HashMap<u64, ResponseSender>,
}

pub(crate) struct NotificationSubscription {
    id: u64,
    subscribers: Arc<Mutex<Subscribers>>,
}

impl Drop for NotificationSubscription {
    fn drop(&mut self) {
        if let Ok(mut subscribers) = self.subscribers.lock() {
            subscribers.responses.remove(&self.id);
        }
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

    fn response_channel() -> (ResponseSender, response::ResponseReceiver) {
        let (stream, _peer) = UnixStream::pair().expect("socket pair should open");
        let (responses, receiver, _) =
            response::channel(&stream).expect("response channel should open");

        (responses, receiver)
    }
}
