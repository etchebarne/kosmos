pub(crate) mod editor;
pub(crate) mod envelope;
pub(crate) mod file_tree;
pub(crate) mod formatters;
pub(crate) mod git;
mod ids;
pub(crate) mod language_servers;
pub(crate) mod pane;
pub(crate) mod search;
pub(crate) mod settings;
pub(crate) mod tab;
pub(crate) mod terminal;
pub(crate) mod window;
pub(crate) mod workspace;

#[cfg(test)]
mod contract_fixtures;

use schemars::JsonSchema;
use schemars::r#gen::SchemaGenerator;
use schemars::schema::{Schema, SchemaObject};
use serde::{Deserialize, Serialize};

/// A reviewed escape hatch for values defined by the upstream LSP protocol.
///
/// LSP `data` and command payloads intentionally have no application-level shape.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub(crate) struct AnyJson(pub(crate) serde_json::Value);

impl AnyJson {
    pub(crate) fn into_inner(self) -> serde_json::Value {
        self.0
    }
}

impl From<serde_json::Value> for AnyJson {
    fn from(value: serde_json::Value) -> Self {
        Self(value)
    }
}

impl From<AnyJson> for serde_json::Value {
    fn from(value: AnyJson) -> Self {
        value.0
    }
}

impl JsonSchema for AnyJson {
    fn schema_name() -> String {
        "AnyJson".to_owned()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        Schema::Object(SchemaObject::default())
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EmptyParams {}
