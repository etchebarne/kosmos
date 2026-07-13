use crate::tabs::terminal::{TerminalError, TerminalOutput, TerminalSize, available_shells};
use crate::tree::{TabId, WorkspaceId};

use super::State;

impl State {
    pub fn open_terminal(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        columns: u16,
        rows: u16,
    ) -> Result<TerminalOutput, TerminalError> {
        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(TerminalError::WorkspaceNotFound)?;
        let workspace_directory = self
            .terminal_workspace_directory(workspace_id, tab_id)?
            .to_path_buf();
        let directory = self
            .terminal_view_state(workspace_id, tab_id)
            .map(|state| state.directory())
            .filter(|directory| directory.is_dir())
            .unwrap_or(&workspace_directory)
            .to_path_buf();
        let size = TerminalSize::new(columns, rows)?;

        let output = self
            .terminal_sessions
            .open(workspace_id, tab_id, &directory, size)?;
        let directory = self
            .terminal_sessions
            .working_directory(workspace_id, tab_id)
            .unwrap_or(directory);
        self.update_terminal_view_state(workspace_id, tab_id, directory);

        Ok(output)
    }

    pub fn write_terminal_input(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        input: &str,
    ) -> Result<(), TerminalError> {
        let workspace_id = self.terminal_workspace_id(workspace_id, tab_id)?;

        self.terminal_sessions
            .write_input(workspace_id, tab_id, input)
    }

    pub fn read_terminal_output(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<TerminalOutput, TerminalError> {
        let workspace_id = self.terminal_workspace_id(workspace_id, tab_id)?;
        let directory = self
            .terminal_sessions
            .working_directory(workspace_id, tab_id);
        let output = self.terminal_sessions.read_output(workspace_id, tab_id)?;

        if let Some(directory) = directory {
            self.update_terminal_view_state(workspace_id, tab_id, directory);
        }

        Ok(output)
    }

    pub fn resize_terminal(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        columns: u16,
        rows: u16,
    ) -> Result<(), TerminalError> {
        let workspace_id = self.terminal_workspace_id(workspace_id, tab_id)?;
        let size = TerminalSize::new(columns, rows)?;

        self.terminal_sessions.resize(workspace_id, tab_id, size)
    }

    pub fn restart_terminal(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        columns: u16,
        rows: u16,
        shell_path: &str,
    ) -> Result<TerminalOutput, TerminalError> {
        let workspace_id = self.terminal_workspace_id(workspace_id, tab_id)?;
        let workspace_directory = self
            .terminal_workspace_directory(workspace_id, tab_id)?
            .to_path_buf();
        let directory = self
            .terminal_sessions
            .working_directory(workspace_id, tab_id)
            .or_else(|| {
                self.terminal_view_state(workspace_id, tab_id)
                    .map(|state| state.directory().to_path_buf())
            })
            .filter(|directory| directory.is_dir())
            .unwrap_or(workspace_directory);
        let size = TerminalSize::new(columns, rows)?;
        let shell = available_shells()
            .into_iter()
            .find(|shell| shell.path() == shell_path)
            .ok_or_else(|| TerminalError::ShellNotAvailable(shell_path.to_owned()))?;

        let output =
            self.terminal_sessions
                .restart(workspace_id, tab_id, &directory, size, &shell)?;
        self.update_terminal_view_state(workspace_id, tab_id, directory);

        Ok(output)
    }
}
