use super::envelope::{ClientMessage, ServerMessage};

#[test]
fn client_envelope_fixtures_accept_every_transport_variant() {
    for fixture in [
        r#"{"type":"request","id":1,"domain":"workspace","action":"list","params":{}}"#,
        r#"{"type":"cancel","id":1}"#,
        r#"{"type":"applyEditAck","id":1,"token":"token","applied":true,"failureReason":null}"#,
    ] {
        serde_json::from_str::<ClientMessage>(fixture)
            .expect("synthetic fixture should deserialize");
    }
}

#[test]
fn server_envelope_fixtures_preserve_response_and_notification_wire_shapes() {
    let response = serde_json::to_value(ServerMessage::ok(1, true)).unwrap();
    assert_eq!(
        response,
        serde_json::json!({ "type": "response", "id": 1, "ok": true, "result": true })
    );
    let error =
        serde_json::to_value(ServerMessage::error(1, "fixture.error", "synthetic")).unwrap();
    assert_eq!(error["error"]["code"], "fixture.error");

    let notifications = vec![
        ServerMessage::workspace_changed(vec![1]),
        ServerMessage::language_server_diagnostics_changed(diagnostics()),
        ServerMessage::language_server_diagnostics_resync(),
        ServerMessage::language_server_status_changed("rust-analyzer".to_owned()),
        ServerMessage::language_server_log_available("rust-analyzer".to_owned()),
        ServerMessage::language_server_apply_edit(
            1,
            "token".to_owned(),
            staged_edit(),
            directive(),
        ),
        ServerMessage::language_server_apply_edit_cancelled(1, "token".to_owned()),
    ];
    let expected_events = [
        "workspaceChanged",
        "languageServerDiagnosticsChanged",
        "languageServerDiagnosticsResync",
        "languageServerStatusChanged",
        "languageServerLogAvailable",
        "languageServerApplyEdit",
        "languageServerApplyEditCancelled",
    ];

    for (notification, event) in notifications.into_iter().zip(expected_events) {
        let value = serde_json::to_value(notification).unwrap();
        assert_eq!(value["type"], "notification");
        assert_eq!(value["event"], event);
    }

    let apply_edit = serde_json::to_value(ServerMessage::language_server_apply_edit(
        1,
        "token".to_owned(),
        staged_edit(),
        directive(),
    ))
    .unwrap();
    assert_eq!(apply_edit["edit"]["operations"][0]["workspaceId"], 1);
    assert_eq!(apply_edit["edit"]["operations"][0]["oldPath"], "old.rs");
    assert_eq!(apply_edit["edit"]["operations"][0]["newPath"], "new.rs");
}

fn directive() -> core::language_servers::WorkspaceEditDirective {
    core::language_servers::WorkspaceEditDirective::ApplyOpenModels {
        transaction_id: 1,
        models: Vec::new(),
    }
}

fn diagnostics() -> core::events::LanguageServerDiagnosticsChanged {
    core::events::LanguageServerDiagnosticsChanged {
        workspace_id: core::tree::WorkspaceId::new(1),
        path: "src/main.rs".to_owned(),
        server_id: "rust-analyzer".to_owned(),
        generation: 1,
        version: 1,
        diagnostics: vec![core::language_servers::LanguageServerDiagnostic {
            range: core::language_servers::LanguageServerRange {
                start: core::language_servers::LanguageServerPosition {
                    line: 0,
                    character: 0,
                },
                end: core::language_servers::LanguageServerPosition {
                    line: 0,
                    character: 1,
                },
            },
            severity: None,
            message: "synthetic fixture".to_owned(),
            source: None,
            code: None,
        }],
    }
}

fn staged_edit() -> core::language_servers::StagedWorkspaceEdit {
    core::language_servers::StagedWorkspaceEdit {
        transaction_id: 1,
        authorization: "token".to_owned(),
        documents: Vec::new(),
        operations: vec![
            core::language_servers::StagedWorkspaceEditOperation::RenameFile {
                workspace_id: core::tree::WorkspaceId::new(1),
                old_path: "old.rs".to_owned(),
                new_path: "new.rs".to_owned(),
            },
        ],
    }
}
