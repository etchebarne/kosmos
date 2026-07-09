use core::tabs::terminal::TerminalOutput;
use serde::{Deserialize, Serialize};

use super::pane::WorkspaceIdParam;
use super::tab::TabIdParam;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenTerminalParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) columns: u16,
    pub(crate) rows: u16,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TerminalTabParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WriteTerminalInputParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) data: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResizeTerminalParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) columns: u16,
    pub(crate) rows: u16,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TerminalOutputSnapshot {
    output: String,
    exited: bool,
    exit_code: Option<u32>,
    signal: Option<String>,
}

impl TerminalOutputSnapshot {
    pub(crate) fn from_output(output: &TerminalOutput) -> Self {
        let exit_status = output.exit_status();

        Self {
            output: output.output().to_owned(),
            exited: output.exited(),
            exit_code: exit_status.map(|status| status.exit_code()),
            signal: exit_status.and_then(|status| status.signal().map(str::to_owned)),
        }
    }
}
