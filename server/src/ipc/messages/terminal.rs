use core::tabs::terminal::{TerminalOutput, TerminalReplaySegment, TerminalShell};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ids::{TabIdParam, WorkspaceIdParam};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenTerminalParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) columns: u16,
    pub(crate) rows: u16,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TerminalTabParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WriteTerminalInputParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) data: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResizeTerminalParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) columns: u16,
    pub(crate) rows: u16,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RestartTerminalParams {
    pub(crate) workspace_id: Option<WorkspaceIdParam>,
    pub(crate) tab_id: TabIdParam,
    pub(crate) columns: u16,
    pub(crate) rows: u16,
    pub(crate) shell: String,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TerminalShellSnapshot {
    name: String,
    path: String,
    is_default: bool,
}

impl TerminalShellSnapshot {
    pub(crate) fn from_shell(shell: &TerminalShell) -> Self {
        Self {
            name: shell.name().to_owned(),
            path: shell.path().to_owned(),
            is_default: shell.is_default(),
        }
    }
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TerminalOutputSnapshot {
    output: String,
    truncated: bool,
    exited: bool,
    exit_code: Option<u32>,
    signal: Option<String>,
    replay: Vec<TerminalReplaySegmentSnapshot>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TerminalReplaySegmentSnapshot {
    columns: u16,
    rows: u16,
    output: String,
}

impl TerminalReplaySegmentSnapshot {
    fn from_segment(segment: &TerminalReplaySegment) -> Self {
        Self {
            columns: segment.size().columns(),
            rows: segment.size().rows(),
            output: segment.output().to_owned(),
        }
    }
}

impl TerminalOutputSnapshot {
    pub(crate) fn from_output(output: &TerminalOutput) -> Self {
        let exit_status = output.exit_status();

        Self {
            output: output.output().to_owned(),
            truncated: output.truncated(),
            exited: output.exited(),
            exit_code: exit_status.map(|status| status.exit_code()),
            signal: exit_status.and_then(|status| status.signal().map(str::to_owned)),
            replay: output
                .replay()
                .iter()
                .map(TerminalReplaySegmentSnapshot::from_segment)
                .collect(),
        }
    }
}
