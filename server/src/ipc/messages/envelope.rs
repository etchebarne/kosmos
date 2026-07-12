use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub(crate) enum ClientMessage {
    Request(RequestEnvelope),
    Cancel {
        id: u64,
    },
    ApplyEditAck {
        id: u64,
        token: String,
        applied: bool,
        #[serde(default)]
        failure_reason: Option<String>,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub(crate) enum ServerMessage {
    Response(ResponseEnvelope),
    Notification(NotificationEnvelope),
}

impl ServerMessage {
    pub(crate) fn ok<T>(id: u64, result: T) -> Self
    where
        T: Serialize,
    {
        let result = match serde_json::to_value(result) {
            Ok(result) => result,
            Err(error) => {
                return Self::error(id, "ipc.serialization_failed", error.to_string());
            }
        };

        Self::Response(ResponseEnvelope {
            id,
            ok: true,
            result: Some(result),
            error: None,
        })
    }

    pub(crate) fn error(id: u64, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Response(ResponseEnvelope {
            id,
            ok: false,
            result: None,
            error: Some(ErrorEnvelope {
                code: code.into(),
                message: message.into(),
            }),
        })
    }

    pub(crate) fn workspace_changed(workspace_ids: Vec<u64>) -> Self {
        Self::Notification(NotificationEnvelope {
            event: NotificationEvent::WorkspaceChanged,
            payload: serde_json::json!({ "workspaceIds": workspace_ids }),
        })
    }

    pub(crate) fn language_server_diagnostics_changed(
        diagnostics: core::events::LanguageServerDiagnosticsChanged,
    ) -> Self {
        let workspace_id = diagnostics.workspace_id.value();
        let path = diagnostics.path;
        let server_id = diagnostics.server_id;
        let generation = diagnostics.generation;
        let version = diagnostics.version;
        let diagnostics = diagnostics
            .diagnostics
            .into_iter()
            .map(crate::ipc::messages::language_servers::LanguageServerDiagnosticPayload::from_core)
            .collect::<Vec<_>>();
        Self::Notification(NotificationEnvelope {
            event: NotificationEvent::LanguageServerDiagnosticsChanged,
            payload: serde_json::json!({
                "workspaceId": workspace_id,
                "path": path,
                "serverId": server_id,
                "generation": generation,
                "version": version,
                "diagnostics": diagnostics,
            }),
        })
    }

    pub(crate) fn language_server_diagnostics_resync() -> Self {
        Self::Notification(NotificationEnvelope {
            event: NotificationEvent::LanguageServerDiagnosticsResync,
            payload: serde_json::json!({}),
        })
    }

    pub(crate) fn language_server_status_changed(server_id: String) -> Self {
        Self::Notification(NotificationEnvelope {
            event: NotificationEvent::LanguageServerStatusChanged,
            payload: serde_json::json!({ "serverId": server_id }),
        })
    }

    pub(crate) fn language_server_log_available(server_id: String) -> Self {
        Self::Notification(NotificationEnvelope {
            event: NotificationEvent::LanguageServerLogAvailable,
            payload: serde_json::json!({ "serverId": server_id }),
        })
    }

    pub(crate) fn language_server_apply_edit(
        id: u64,
        token: String,
        edit: core::language_servers::StagedWorkspaceEdit,
    ) -> Self {
        Self::Notification(NotificationEnvelope {
            event: NotificationEvent::LanguageServerApplyEdit,
            payload: serde_json::json!({
                "id": id,
                "token": token,
                "edit": crate::ipc::messages::language_servers::StagedWorkspaceEditPayload::from_core(edit)
            }),
        })
    }

    pub(crate) fn language_server_apply_edit_cancelled(id: u64, token: String) -> Self {
        Self::Notification(NotificationEnvelope {
            event: NotificationEvent::LanguageServerApplyEditCancelled,
            payload: serde_json::json!({ "id": id, "token": token }),
        })
    }

    pub(crate) fn is_ok(&self) -> bool {
        match self {
            Self::Response(response) => response.ok,
            Self::Notification(_) => false,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequestEnvelope {
    pub(crate) id: u64,
    pub(crate) domain: Domain,
    pub(crate) action: String,
    #[serde(default)]
    pub(crate) params: serde_json::Value,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Domain {
    Workspace,
    Pane,
    Tab,
    FileTree,
    Formatters,
    Editor,
    Git,
    Search,
    Terminal,
    Settings,
    LanguageServers,
    Window,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResponseEnvelope {
    id: u64,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ErrorEnvelope>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorEnvelope {
    code: String,
    message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NotificationEnvelope {
    event: NotificationEvent,
    #[serde(flatten)]
    payload: serde_json::Value,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
enum NotificationEvent {
    WorkspaceChanged,
    LanguageServerDiagnosticsChanged,
    LanguageServerDiagnosticsResync,
    LanguageServerStatusChanged,
    LanguageServerLogAvailable,
    LanguageServerApplyEdit,
    LanguageServerApplyEditCancelled,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn response_serialization_failures_return_errors_instead_of_panicking() {
        let response = ServerMessage::ok(1, HashMap::from([(vec![1, 2], true)]));

        assert!(!response.is_ok());
    }

    #[test]
    fn workspace_change_notifications_serialize_without_a_request_id() {
        let notification = serde_json::to_value(ServerMessage::workspace_changed(vec![1, 2]))
            .expect("notification should serialize");

        assert_eq!(
            notification,
            serde_json::json!({
                "type": "notification",
                "event": "workspaceChanged",
                "workspaceIds": [1, 2]
            })
        );
    }

    #[test]
    fn language_server_diagnostics_notifications_are_typed_and_versioned() {
        let notification = ServerMessage::language_server_diagnostics_changed(
            core::events::LanguageServerDiagnosticsChanged {
                workspace_id: core::tree::WorkspaceId::new(3),
                path: "src/main.rs".to_owned(),
                server_id: "rust-analyzer".to_owned(),
                generation: 7,
                version: 11,
                diagnostics: vec![core::language_servers::LanguageServerDiagnostic {
                    range: core::language_servers::LanguageServerRange {
                        start: core::language_servers::LanguageServerPosition {
                            line: 1,
                            character: 2,
                        },
                        end: core::language_servers::LanguageServerPosition {
                            line: 1,
                            character: 4,
                        },
                    },
                    severity: Some(
                        core::language_servers::LanguageServerDiagnosticSeverity::Warning,
                    ),
                    message: "test warning".to_owned(),
                    source: Some("test".to_owned()),
                    code: None,
                }],
            },
        );
        let value = serde_json::to_value(notification).expect("notification should serialize");

        assert_eq!(value["type"], "notification");
        assert_eq!(value["event"], "languageServerDiagnosticsChanged");
        assert_eq!(value["workspaceId"], 3);
        assert_eq!(value["serverId"], "rust-analyzer");
        assert_eq!(value["generation"], 7);
        assert_eq!(value["version"], 11);
        assert_eq!(value["diagnostics"][0]["severity"], "warning");
    }

    #[test]
    fn language_server_diagnostics_resync_serializes_as_a_coalesced_signal() {
        assert_eq!(
            serde_json::to_value(ServerMessage::language_server_diagnostics_resync()).unwrap(),
            serde_json::json!({
                "type": "notification",
                "event": "languageServerDiagnosticsResync"
            })
        );
    }

    #[test]
    fn cancellation_messages_target_a_transport_request_id() {
        let message = serde_json::from_str::<ClientMessage>(r#"{"type":"cancel","id":42}"#)
            .expect("cancellation should deserialize");

        assert!(matches!(message, ClientMessage::Cancel { id: 42 }));
    }
}
