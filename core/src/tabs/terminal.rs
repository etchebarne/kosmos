use std::collections::HashMap;
use std::error::Error as StdError;
use std::fmt;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::tree::{TabId, WorkspaceId};

pub type Result<T> = std::result::Result<T, TerminalError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalSize {
    columns: u16,
    rows: u16,
}

impl TerminalSize {
    pub fn new(columns: u16, rows: u16) -> Result<Self> {
        if columns == 0 || rows == 0 {
            Err(TerminalError::InvalidSize { columns, rows })
        } else {
            Ok(Self { columns, rows })
        }
    }

    fn pty_size(self) -> PtySize {
        PtySize {
            rows: self.rows,
            cols: self.columns,
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalExitStatus {
    exit_code: u32,
    signal: Option<String>,
}

impl TerminalExitStatus {
    pub fn exit_code(&self) -> u32 {
        self.exit_code
    }

    pub fn signal(&self) -> Option<&str> {
        self.signal.as_deref()
    }
}

impl From<portable_pty::ExitStatus> for TerminalExitStatus {
    fn from(status: portable_pty::ExitStatus) -> Self {
        Self {
            exit_code: status.exit_code(),
            signal: status.signal().map(str::to_owned),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalOutput {
    output: String,
    exit_status: Option<TerminalExitStatus>,
}

impl TerminalOutput {
    fn new(output: String, exit_status: Option<TerminalExitStatus>) -> Self {
        Self {
            output,
            exit_status,
        }
    }

    pub fn output(&self) -> &str {
        &self.output
    }

    pub fn exit_status(&self) -> Option<&TerminalExitStatus> {
        self.exit_status.as_ref()
    }

    pub fn exited(&self) -> bool {
        self.exit_status.is_some()
    }
}

#[derive(Default)]
pub struct TerminalSessions {
    sessions: HashMap<TerminalSessionKey, TerminalSession>,
}

impl TerminalSessions {
    pub fn open(
        &mut self,
        workspace_id: WorkspaceId,
        tab_id: TabId,
        cwd: &Path,
        size: TerminalSize,
    ) -> Result<TerminalOutput> {
        let key = TerminalSessionKey::new(workspace_id, tab_id);

        if let Some(session) = self.sessions.get_mut(&key) {
            session.resize(size)?;
            return session.read_output();
        }

        let session = TerminalSession::spawn(cwd, size)?;
        self.sessions.insert(key, session);
        self.sessions
            .get_mut(&key)
            .expect("terminal session was just inserted")
            .read_output()
    }

    pub fn write_input(
        &mut self,
        workspace_id: WorkspaceId,
        tab_id: TabId,
        input: &str,
    ) -> Result<()> {
        self.session_mut(workspace_id, tab_id)?.write_input(input)
    }

    pub fn read_output(
        &mut self,
        workspace_id: WorkspaceId,
        tab_id: TabId,
    ) -> Result<TerminalOutput> {
        self.session_mut(workspace_id, tab_id)?.read_output()
    }

    pub fn resize(
        &mut self,
        workspace_id: WorkspaceId,
        tab_id: TabId,
        size: TerminalSize,
    ) -> Result<()> {
        self.session_mut(workspace_id, tab_id)?.resize(size)
    }

    pub fn close(&mut self, workspace_id: WorkspaceId, tab_id: TabId) -> bool {
        self.sessions
            .remove(&TerminalSessionKey::new(workspace_id, tab_id))
            .is_some()
    }

    pub fn close_workspace(&mut self, workspace_id: WorkspaceId) {
        self.sessions
            .retain(|key, _| key.workspace_id != workspace_id);
    }

    fn session_mut(
        &mut self,
        workspace_id: WorkspaceId,
        tab_id: TabId,
    ) -> Result<&mut TerminalSession> {
        self.sessions
            .get_mut(&TerminalSessionKey::new(workspace_id, tab_id))
            .ok_or(TerminalError::SessionNotFound)
    }
}

impl fmt::Debug for TerminalSessions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TerminalSessions")
            .field("session_count", &self.sessions.len())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct TerminalSessionKey {
    workspace_id: WorkspaceId,
    tab_id: TabId,
}

impl TerminalSessionKey {
    fn new(workspace_id: WorkspaceId, tab_id: TabId) -> Self {
        Self {
            workspace_id,
            tab_id,
        }
    }
}

struct TerminalSession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    output: Arc<Mutex<Vec<u8>>>,
    exit_status: Option<TerminalExitStatus>,
}

impl TerminalSession {
    fn spawn(cwd: &Path, size: TerminalSize) -> Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(size.pty_size())
            .map_err(TerminalError::pty)?;
        let portable_pty::PtyPair { master, slave } = pair;
        let mut command = CommandBuilder::new_default_prog();

        command.cwd(cwd.as_os_str());
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");

        let child = slave.spawn_command(command).map_err(TerminalError::pty)?;
        drop(slave);

        let reader = master.try_clone_reader().map_err(TerminalError::pty)?;
        let writer = master.take_writer().map_err(TerminalError::pty)?;
        let output = Arc::new(Mutex::new(Vec::new()));

        spawn_reader(reader, Arc::clone(&output));

        Ok(Self {
            master,
            writer,
            child,
            output,
            exit_status: None,
        })
    }

    fn write_input(&mut self, input: &str) -> Result<()> {
        self.writer.write_all(input.as_bytes())?;
        self.writer.flush()?;

        Ok(())
    }

    fn read_output(&mut self) -> Result<TerminalOutput> {
        let output = self.drain_output()?;
        let exit_status = self.exit_status()?;

        Ok(TerminalOutput::new(output, exit_status))
    }

    fn resize(&mut self, size: TerminalSize) -> Result<()> {
        self.master
            .resize(size.pty_size())
            .map_err(TerminalError::pty)
    }

    fn drain_output(&self) -> Result<String> {
        let bytes = {
            let mut output = self
                .output
                .lock()
                .map_err(|_| TerminalError::ReadBufferUnavailable)?;

            std::mem::take(&mut *output)
        };

        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn exit_status(&mut self) -> Result<Option<TerminalExitStatus>> {
        if self.exit_status.is_none() {
            if let Some(status) = self.child.try_wait()? {
                self.exit_status = Some(status.into());
            }
        }

        Ok(self.exit_status.clone())
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if self.exit_status.is_some() {
            return;
        }

        if matches!(self.child.try_wait(), Ok(Some(_))) {
            return;
        }

        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug)]
pub enum TerminalError {
    InvalidSize { columns: u16, rows: u16 },
    Io(io::Error),
    Pty(String),
    ReadBufferUnavailable,
    SessionNotFound,
    TabNotFound,
    WorkspaceNotFound,
}

impl TerminalError {
    fn pty(error: impl fmt::Display) -> Self {
        Self::Pty(error.to_string())
    }
}

impl fmt::Display for TerminalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSize { columns, rows } => write!(
                formatter,
                "terminal size must be non-zero, got {columns} columns and {rows} rows"
            ),
            Self::Io(error) => write!(formatter, "terminal I/O failed: {error}"),
            Self::Pty(error) => write!(formatter, "terminal process failed: {error}"),
            Self::ReadBufferUnavailable => {
                formatter.write_str("terminal output buffer is unavailable")
            }
            Self::SessionNotFound => formatter.write_str("terminal session does not exist"),
            Self::TabNotFound => formatter.write_str("terminal tab does not exist"),
            Self::WorkspaceNotFound => formatter.write_str("workspace does not exist"),
        }
    }
}

impl StdError for TerminalError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::InvalidSize { .. }
            | Self::Pty(_)
            | Self::ReadBufferUnavailable
            | Self::SessionNotFound
            | Self::TabNotFound
            | Self::WorkspaceNotFound => None,
        }
    }
}

impl From<io::Error> for TerminalError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

fn spawn_reader(mut reader: Box<dyn Read + Send>, output: Arc<Mutex<Vec<u8>>>) {
    thread::spawn(move || {
        let mut buffer = [0; 8192];

        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(bytes_read) => {
                    let Ok(mut output) = output.lock() else {
                        break;
                    };

                    output.extend_from_slice(&buffer[..bytes_read]);
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_size_rejects_zero_dimensions() {
        let error = TerminalSize::new(0, 24).expect_err("zero columns should be rejected");

        assert!(matches!(
            error,
            TerminalError::InvalidSize {
                columns: 0,
                rows: 24
            }
        ));
        assert!(TerminalSize::new(80, 24).is_ok());
    }
}
