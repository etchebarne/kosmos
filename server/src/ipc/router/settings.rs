use core::settings::{SettingValue, SettingsError};

use super::super::messages::EmptyParams;
use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::settings::{SettingValueParam, SettingsSnapshot, UpdateSettingParams};
use super::{Route, RouteDefinition, find_route, parse_params};

pub(super) const ROUTES: &[Route] = &[
    Route::new::<EmptyParams, SettingsSnapshot>("get", RouteDefinition::snapshot(get)),
    Route::new::<UpdateSettingParams, SettingsSnapshot>(
        "update",
        RouteDefinition::settings(update),
    ),
];

pub(super) fn resolve(action: &str) -> Option<RouteDefinition> {
    find_route(ROUTES, action)
}

fn get(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    settings_response(request.id, state)
}

fn update(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    let params = match parse_params::<UpdateSettingParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    let value = match setting_value(params.value) {
        Some(value) => value,
        None => {
            return ServerMessage::error(
                request.id,
                "settings.invalid_value",
                "setting values must be a boolean, string, or finite number",
            );
        }
    };

    match state.update_setting(&params.id, value) {
        Ok(()) => settings_response(request.id, state),
        Err(error) => settings_error(request.id, error),
    }
}

fn setting_value(value: SettingValueParam) -> Option<SettingValue> {
    match value {
        SettingValueParam::Boolean(value) => Some(SettingValue::Boolean(value)),
        SettingValueParam::String(value) => Some(SettingValue::String(value)),
        SettingValueParam::Number(value) if value.is_finite() => Some(SettingValue::Number(value)),
        SettingValueParam::Number(_) => None,
    }
}

fn settings_response(id: u64, state: &core::State) -> ServerMessage {
    ServerMessage::ok(id, SettingsSnapshot::from_settings(state.settings()))
}

fn settings_error(id: u64, error: SettingsError) -> ServerMessage {
    let code = match error {
        SettingsError::UnknownSetting(_) => "settings.unknown_setting",
        SettingsError::InvalidValue { .. } => "settings.invalid_value",
    };

    ServerMessage::error(id, code, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::messages::envelope::Domain;

    #[test]
    fn update_changes_the_core_setting_and_returns_the_canonical_snapshot() {
        let mut state = core::State::new();
        let request = request(serde_json::json!({
            "id": core::settings::EDITOR_SOFT_WRAP,
            "value": true
        }));

        let response = update(&mut state, &request);
        let response = serde_json::to_value(response).expect("response should serialize");

        assert!(
            response["ok"]
                .as_bool()
                .expect("response should contain ok")
        );
        assert_eq!(
            response["result"]["categories"][1]["items"][0]["value"],
            true
        );
        assert_eq!(
            state.settings().boolean(core::settings::EDITOR_SOFT_WRAP),
            Some(true)
        );
    }

    #[test]
    fn update_rejects_values_that_do_not_match_the_backend_definition() {
        let mut state = core::State::new();
        let request = request(serde_json::json!({
            "id": core::settings::EDITOR_SOFT_WRAP,
            "value": "true"
        }));

        let response = update(&mut state, &request);
        let response = serde_json::to_value(response).expect("response should serialize");

        assert_eq!(response["ok"], false);
        assert_eq!(response["error"]["code"], "settings.invalid_value");
        assert_eq!(
            state.settings().boolean(core::settings::EDITOR_SOFT_WRAP),
            Some(false)
        );
    }

    fn request(params: serde_json::Value) -> RequestEnvelope {
        RequestEnvelope {
            id: 1,
            domain: Domain::Settings,
            action: "update".to_owned(),
            params,
        }
    }
}
