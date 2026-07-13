use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateWindowStateParams {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) maximized: bool,
    pub(crate) fullscreen: bool,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WindowStateSnapshot {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    maximized: bool,
    fullscreen: bool,
}

impl WindowStateSnapshot {
    pub(crate) fn from_state(state: core::window::WindowState) -> Self {
        Self {
            x: state.x(),
            y: state.y(),
            width: state.width(),
            height: state.height(),
            maximized: state.is_maximized(),
            fullscreen: state.is_fullscreen(),
        }
    }
}
