use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub(crate) enum ClientMessage {
    Request(RequestEnvelope),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub(crate) enum ServerMessage {
    Response(ResponseEnvelope),
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

    pub(crate) fn is_ok(&self) -> bool {
        match self {
            Self::Response(response) => response.ok,
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
    Editor,
    Git,
    Terminal,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResponseEnvelope {
    id: u64,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ErrorEnvelope>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorEnvelope {
    code: String,
    message: String,
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
}
