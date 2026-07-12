use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::window::{UpdateWindowStateParams, WindowStateSnapshot};
use super::{Route, RouteDefinition, find_route, parse_params};

pub(super) const ROUTES: &[Route] = &[
    Route {
        action: "get",
        definition: RouteDefinition::snapshot(get),
    },
    Route {
        action: "update",
        definition: RouteDefinition::window(update),
    },
];

pub(super) fn resolve(action: &str) -> Option<RouteDefinition> {
    find_route(ROUTES, action)
}

fn get(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    ServerMessage::ok(
        request.id,
        state.window_state().map(WindowStateSnapshot::from_state),
    )
}

fn update(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    let params = match parse_params::<UpdateWindowStateParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    let Some(window_state) = core::window::WindowState::new(
        params.x,
        params.y,
        params.width,
        params.height,
        params.maximized,
        params.fullscreen,
    ) else {
        return ServerMessage::error(
            request.id,
            "window.invalid_state",
            "window dimensions must be greater than zero",
        );
    };

    state.update_window_state(window_state);
    ServerMessage::ok(request.id, WindowStateSnapshot::from_state(window_state))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_changes_core_window_state() {
        let mut state = core::State::new();
        let request = RequestEnvelope {
            id: 1,
            domain: super::super::super::messages::envelope::Domain::Window,
            action: "update".to_owned(),
            params: serde_json::json!({
                "x": -100,
                "y": 50,
                "width": 1440,
                "height": 900,
                "maximized": true,
                "fullscreen": false
            }),
        };

        let response = update(&mut state, &request);

        assert!(response.is_ok());
        assert_eq!(state.window_state().map(|state| state.width()), Some(1440));
        assert_eq!(
            state.window_state().map(|state| state.is_maximized()),
            Some(true)
        );
    }
}
