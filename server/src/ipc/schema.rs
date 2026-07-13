use std::fs::File;
use std::io::{self, BufWriter};
use std::path::Path;

use schemars::JsonSchema;
use schemars::schema::RootSchema;
use serde_json::{Map, Value, json};

use super::messages::envelope::{
    LanguageServerApplyEditCancelledNotification, LanguageServerApplyEditNotification,
    LanguageServerDiagnosticsChangedNotification, LanguageServerDiagnosticsResyncNotification,
    LanguageServerLogAvailableNotification, LanguageServerStatusChangedNotification,
    ToolingCapabilitiesChangedNotification, WorkspaceChangedNotification,
};
use super::router;

const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

/// Reviewed, deliberately unconstrained values passed through from the LSP protocol.
pub const ANY_JSON_ALLOWLIST: &[&str] = &[
    "languageServers.codeActions.context",
    "languageServers.resolveCodeAction.raw",
    "languageServers.resolveCompletion.raw",
    "languageServers.resolveWorkspaceSymbol.raw",
    "languageServers.completion.result.items[].raw",
    "languageServers.codeActions.result[].raw",
    "languageServers.workspaceSymbols.result[].raw",
    "languageServers.executeCommand.result",
];

#[derive(Clone, Copy)]
struct NotificationContract {
    event: &'static str,
    schema: fn() -> RootSchema,
}

impl NotificationContract {
    const fn of<Payload: JsonSchema>(event: &'static str) -> Self {
        Self {
            event,
            schema: schema_for::<Payload>,
        }
    }
}

const NOTIFICATIONS: &[NotificationContract] = &[
    NotificationContract::of::<WorkspaceChangedNotification>("workspaceChanged"),
    NotificationContract::of::<LanguageServerDiagnosticsChangedNotification>(
        "languageServerDiagnosticsChanged",
    ),
    NotificationContract::of::<LanguageServerDiagnosticsResyncNotification>(
        "languageServerDiagnosticsResync",
    ),
    NotificationContract::of::<LanguageServerStatusChangedNotification>(
        "languageServerStatusChanged",
    ),
    NotificationContract::of::<LanguageServerLogAvailableNotification>(
        "languageServerLogAvailable",
    ),
    NotificationContract::of::<ToolingCapabilitiesChangedNotification>(
        "toolingCapabilitiesChanged",
    ),
    NotificationContract::of::<LanguageServerApplyEditNotification>("languageServerApplyEdit"),
    NotificationContract::of::<LanguageServerApplyEditCancelledNotification>(
        "languageServerApplyEditCancelled",
    ),
];

pub fn export(path: impl AsRef<Path>) -> io::Result<()> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, &document()).map_err(io::Error::other)
}

pub fn document() -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "Kosmos IPC contract",
        "requestEnvelope": request_envelope_schema(),
        "responseEnvelope": response_envelope_schema(),
        "error": error_schema(),
        "actions": action_schemas(),
        "notifications": notification_schemas(),
        "anyJsonAllowlist": ANY_JSON_ALLOWLIST,
    })
}

fn action_schemas() -> Value {
    let mut domains = Map::new();

    for domain in router::DOMAINS {
        let mut actions = Map::new();
        for route in router::routes_for(*domain) {
            actions.insert(
                route.action.to_owned(),
                json!({
                    "params": normalized_schema((route.contract.params_schema)(), false),
                    "result": normalized_schema((route.contract.result_schema)(), true),
                }),
            );
        }
        domains.insert(domain.as_str().to_owned(), Value::Object(actions));
    }

    Value::Object(domains)
}

fn notification_schemas() -> Value {
    let mut notifications = Map::new();

    for contract in NOTIFICATIONS {
        notifications.insert(
            contract.event.to_owned(),
            notification_schema(contract.event, (contract.schema)()),
        );
    }

    Value::Object(notifications)
}

fn notification_schema(event: &str, schema: RootSchema) -> Value {
    let mut schema = normalized_schema(schema, true);
    let object = schema
        .as_object_mut()
        .expect("notification payload schema must be an object");
    let properties = object
        .entry("properties")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .expect("notification properties must be an object");
    properties.insert("type".to_owned(), json!({ "const": "notification" }));
    properties.insert("event".to_owned(), json!({ "const": event }));

    let required = object
        .entry("required")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .expect("notification required fields must be an array");
    required.push(Value::String("type".to_owned()));
    required.push(Value::String("event".to_owned()));

    schema
}

fn request_envelope_schema() -> Value {
    json!({
        "type": "object",
        "required": ["type", "id", "domain", "action"],
        "properties": {
            "type": { "const": "request" },
            "id": safe_integer_schema(0, MAX_SAFE_INTEGER),
            "domain": { "enum": router::DOMAINS.iter().map(|domain| domain.as_str()).collect::<Vec<_>>() },
            "action": { "type": "string" },
            "params": {}
        }
    })
}

fn response_envelope_schema() -> Value {
    json!({
        "oneOf": [
            {
                "type": "object",
                "required": ["type", "id", "ok", "result"],
                "properties": {
                    "type": { "const": "response" },
                    "id": safe_integer_schema(0, MAX_SAFE_INTEGER),
                    "ok": { "const": true },
                    "result": {}
                }
            },
            {
                "type": "object",
                "required": ["type", "id", "ok", "error"],
                "properties": {
                    "type": { "const": "response" },
                    "id": safe_integer_schema(0, MAX_SAFE_INTEGER),
                    "ok": { "const": false },
                    "error": error_schema()
                }
            }
        ]
    })
}

fn error_schema() -> Value {
    json!({
        "type": "object",
        "required": ["code", "message"],
        "properties": {
            "code": { "type": "string" },
            "message": { "type": "string" }
        }
    })
}

fn safe_integer_schema(minimum: i64, maximum: i64) -> Value {
    json!({ "type": "integer", "minimum": minimum, "maximum": maximum })
}

fn schema_for<T: JsonSchema>() -> RootSchema {
    schemars::schema_for!(T)
}

fn normalized_schema(schema: RootSchema, require_nullable_fields: bool) -> Value {
    let mut schema = serde_json::to_value(schema).expect("schema should serialize");
    constrain_safe_integers(&mut schema);
    if require_nullable_fields {
        require_nullable_properties(&mut schema);
    }
    schema
}

fn constrain_safe_integers(schema: &mut Value) {
    let Some(object) = schema.as_object_mut() else {
        return;
    };

    match object.get("format").and_then(Value::as_str) {
        Some("uint") | Some("uint64") | Some("uint128") => {
            object.insert("maximum".to_owned(), Value::from(MAX_SAFE_INTEGER));
        }
        Some("int") | Some("int64") | Some("int128") => {
            object.insert("minimum".to_owned(), Value::from(-MAX_SAFE_INTEGER));
            object.insert("maximum".to_owned(), Value::from(MAX_SAFE_INTEGER));
        }
        _ => {}
    }

    for value in object.values_mut() {
        constrain_safe_integers(value);
    }
}

fn require_nullable_properties(schema: &mut Value) {
    let Some(object) = schema.as_object_mut() else {
        return;
    };

    let nullable_properties = object
        .get("properties")
        .and_then(Value::as_object)
        .map(|properties| {
            properties
                .iter()
                .filter(|(_, property)| permits_null(property))
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !nullable_properties.is_empty() {
        let required = object
            .entry("required")
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .expect("schema required fields must be an array");
        for name in nullable_properties {
            if !required.iter().any(|required_name| required_name == &name) {
                required.push(Value::String(name));
            }
        }
    }

    for value in object.values_mut() {
        require_nullable_properties(value);
    }
}

fn permits_null(schema: &Value) -> bool {
    let any_of_permits_null =
        schema
            .get("anyOf")
            .and_then(Value::as_array)
            .is_some_and(|variants| {
                variants
                    .iter()
                    .any(|variant| variant.get("type") == Some(&Value::String("null".to_owned())))
            });
    let type_permits_null = schema
        .get("type")
        .and_then(Value::as_array)
        .is_some_and(|types| types.iter().any(|kind| kind == "null"));

    any_of_permits_null || type_permits_null
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exported_contract_is_deterministic_and_has_a_schema_for_every_route() {
        let first = serde_json::to_vec(&document()).expect("contract should serialize");
        let second = serde_json::to_vec(&document()).expect("contract should serialize");
        assert_eq!(first, second);

        let contract = document();
        let actions = contract["actions"]
            .as_object()
            .expect("actions should be an object");
        assert_eq!(
            actions
                .values()
                .map(|actions| actions.as_object().unwrap().len())
                .sum::<usize>(),
            router::DOMAINS
                .iter()
                .map(|domain| router::routes_for(*domain).len())
                .sum::<usize>(),
        );
        assert_eq!(
            contract["notifications"].as_object().unwrap().len(),
            NOTIFICATIONS.len(),
        );
    }

    #[test]
    fn safe_integer_constraints_are_added_to_64_bit_schema_fields() {
        let schema = normalized_schema(schemars::schema_for!(u64), false);
        assert_eq!(schema["maximum"], MAX_SAFE_INTEGER);
    }

    #[test]
    fn unconstrained_values_are_limited_to_reviewed_lsp_protocol_fields() {
        assert_eq!(
            ANY_JSON_ALLOWLIST,
            [
                "languageServers.codeActions.context",
                "languageServers.resolveCodeAction.raw",
                "languageServers.resolveCompletion.raw",
                "languageServers.resolveWorkspaceSymbol.raw",
                "languageServers.completion.result.items[].raw",
                "languageServers.codeActions.result[].raw",
                "languageServers.workspaceSymbols.result[].raw",
                "languageServers.executeCommand.result",
            ],
        );
        assert!(!include_str!("messages/language_servers.rs").contains("serde_json::Value"));
        assert!(!include_str!("messages/settings.rs").contains("serde_json::Value"));
    }
}
