use std::collections::{HashMap, HashSet};
use std::env;
use std::error::Error as StdError;
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::tree::{TabId, WorkspaceId};

pub type Result<T> = std::result::Result<T, TerminalError>;

const MAX_BUFFERED_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
const MAX_INPUT_BYTES: usize = 256 * 1024;
const MAX_PENDING_INPUTS: usize = 64;
const EXIT_OUTPUT_GRACE_PERIOD: Duration = Duration::from_millis(100);
const SHELLS_FILE: &str = "/etc/shells";
const FALLBACK_SHELLS: &[&str] = &["/bin/bash", "/bin/zsh", "/bin/fish"];
const SIDECAR_ENVIRONMENT_VARIABLES: &[&str] =
    &["KOSMOS_DATABASE", "KOSMOS_PARENT_PID", "KOSMOS_SOCKET"];
const INTERACTIVE_SHELLS: &[&str] = &[
    "bash", "zsh", "fish", "nu", "ksh", "mksh", "tcsh", "csh", "xonsh", "elvish", "pwsh",
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalViewState {
    workspace_id: WorkspaceId,
    tab_id: TabId,
    directory: PathBuf,
}

impl TerminalViewState {
    pub fn new(workspace_id: WorkspaceId, tab_id: TabId, directory: impl Into<PathBuf>) -> Self {
        Self {
            workspace_id,
            tab_id,
            directory: directory.into(),
        }
    }

    pub fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }

    pub fn tab_id(&self) -> TabId {
        self.tab_id
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub(crate) fn set_directory(&mut self, directory: impl Into<PathBuf>) {
        self.directory = directory.into();
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalShell {
    name: String,
    path: String,
    is_default: bool,
}

impl TerminalShell {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn is_default(&self) -> bool {
        self.is_default
    }
}

pub fn available_shells() -> Vec<TerminalShell> {
    let configured_shells = fs::read_to_string(SHELLS_FILE).ok();
    let default_shell = env::var("SHELL").ok();

    discover_shells(
        configured_shells.as_deref(),
        default_shell.as_deref(),
        |path| Path::new(path).is_file(),
    )
}

fn discover_shells(
    configured_shells: Option<&str>,
    default_shell: Option<&str>,
    is_available: impl Fn(&str) -> bool,
) -> Vec<TerminalShell> {
    let configured_paths = configured_shells
        .into_iter()
        .flat_map(str::lines)
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    let fallback_paths = configured_shells
        .is_none()
        .then_some(FALLBACK_SHELLS.iter().copied())
        .into_iter()
        .flatten();
    let mut seen = HashSet::new();
    let shells = default_shell
        .into_iter()
        .chain(fallback_paths)
        .chain(configured_paths)
        .filter(|path| Path::new(path).is_absolute())
        .filter(|path| is_available(path))
        .filter_map(|path| shell_name(path).map(|name| (name.to_owned(), path.to_owned())))
        .filter(|(name, _)| seen.insert(name.clone()))
        .collect::<Vec<_>>();

    shells
        .into_iter()
        .enumerate()
        .map(|(index, (name, path))| TerminalShell {
            name,
            path,
            is_default: index == 0,
        })
        .collect()
}

fn shell_name(path: &str) -> Option<&str> {
    let name = Path::new(path).file_name()?.to_str()?;

    INTERACTIVE_SHELLS.contains(&name).then_some(name)
}

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
    truncated: bool,
    exit_status: Option<TerminalExitStatus>,
}

impl TerminalOutput {
    fn new(output: String, truncated: bool, exit_status: Option<TerminalExitStatus>) -> Self {
        Self {
            output,
            truncated,
            exit_status,
        }
    }

    pub fn output(&self) -> &str {
        &self.output
    }

    pub fn truncated(&self) -> bool {
        self.truncated
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

        let session = TerminalSession::spawn(cwd, size, None)?;
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

    pub fn restart(
        &mut self,
        workspace_id: WorkspaceId,
        tab_id: TabId,
        cwd: &Path,
        size: TerminalSize,
        shell: &TerminalShell,
    ) -> Result<TerminalOutput> {
        let key = TerminalSessionKey::new(workspace_id, tab_id);

        if !self.sessions.contains_key(&key) {
            return Err(TerminalError::SessionNotFound);
        }

        let replacement = TerminalSession::spawn(cwd, size, Some(Path::new(shell.path())))?;
        self.sessions.insert(key, replacement);
        self.sessions
            .get_mut(&key)
            .expect("terminal session was just replaced")
            .read_output()
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

    pub(crate) fn working_directory(
        &self,
        workspace_id: WorkspaceId,
        tab_id: TabId,
    ) -> Option<PathBuf> {
        self.sessions
            .get(&TerminalSessionKey::new(workspace_id, tab_id))?
            .working_directory()
    }

    pub(crate) fn working_directories(&self) -> Vec<(WorkspaceId, TabId, PathBuf)> {
        self.sessions
            .iter()
            .filter_map(|(key, session)| {
                session
                    .working_directory()
                    .map(|directory| (key.workspace_id, key.tab_id, directory))
            })
            .collect()
    }

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(WorkspaceId, TabId) -> bool) {
        self.sessions
            .retain(|key, _| keep(key.workspace_id, key.tab_id));
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.sessions.len()
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
    input: mpsc::SyncSender<Vec<u8>>,
    child: Box<dyn Child + Send + Sync>,
    output: Arc<Mutex<TerminalOutputBuffer>>,
    exit_status: Option<TerminalExitStatus>,
    exit_observed_at: Option<Instant>,
}

impl TerminalSession {
    fn spawn(cwd: &Path, size: TerminalSize, shell: Option<&Path>) -> Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(size.pty_size())
            .map_err(TerminalError::pty)?;
        let portable_pty::PtyPair { master, slave } = pair;
        let command = match shell {
            Some(shell) => CommandBuilder::new(shell.as_os_str()),
            None => CommandBuilder::new_default_prog(),
        };
        let command = terminal_command(command, cwd);

        let child = slave.spawn_command(command).map_err(TerminalError::pty)?;
        drop(slave);

        let reader = master.try_clone_reader().map_err(TerminalError::pty)?;
        let writer = master.take_writer().map_err(TerminalError::pty)?;
        let input = spawn_writer(writer);
        let output = Arc::new(Mutex::new(TerminalOutputBuffer::default()));

        spawn_reader(reader, Arc::clone(&output));

        Ok(Self {
            master,
            input,
            child,
            output,
            exit_status: None,
            exit_observed_at: None,
        })
    }

    fn write_input(&mut self, input: &str) -> Result<()> {
        let input = terminal_input(input)?;

        match self.input.try_send(input) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(_)) => Err(TerminalError::InputBufferFull),
            Err(mpsc::TrySendError::Disconnected(_)) => Err(TerminalError::WriterUnavailable),
        }
    }

    fn read_output(&mut self) -> Result<TerminalOutput> {
        let exit_status = self.exit_status()?;
        let grace_period_elapsed = self
            .exit_observed_at
            .is_some_and(|observed_at| observed_at.elapsed() >= EXIT_OUTPUT_GRACE_PERIOD);
        let (output, truncated, reader_finished) = self.drain_output(grace_period_elapsed)?;
        let exit_status = exit_status.filter(|_| reader_finished || grace_period_elapsed);

        Ok(TerminalOutput::new(output, truncated, exit_status))
    }

    fn resize(&mut self, size: TerminalSize) -> Result<()> {
        self.master
            .resize(size.pty_size())
            .map_err(TerminalError::pty)
    }

    fn working_directory(&self) -> Option<PathBuf> {
        let process_id = self.child.process_id()?;

        fs::read_link(format!("/proc/{process_id}/cwd")).ok()
    }

    fn drain_output(&self, flush_incomplete: bool) -> Result<(String, bool, bool)> {
        let (bytes, truncated, reader_finished) = {
            let mut output = self
                .output
                .lock()
                .map_err(|_| TerminalError::ReadBufferUnavailable)?;

            output.drain(flush_incomplete)
        };

        Ok((
            String::from_utf8_lossy(&bytes).into_owned(),
            truncated,
            reader_finished,
        ))
    }

    fn exit_status(&mut self) -> Result<Option<TerminalExitStatus>> {
        if self.exit_status.is_none()
            && let Some(status) = self.child.try_wait()?
        {
            self.exit_status = Some(status.into());
            self.exit_observed_at = Some(Instant::now());
        }

        Ok(self.exit_status.clone())
    }
}

fn terminal_command(mut command: CommandBuilder, cwd: &Path) -> CommandBuilder {
    command.cwd(cwd.as_os_str());
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    for variable in SIDECAR_ENVIRONMENT_VARIABLES {
        command.env_remove(variable);
    }
    command
}

fn terminal_input(input: &str) -> Result<Vec<u8>> {
    if input.len() > MAX_INPUT_BYTES {
        Err(TerminalError::InputTooLarge {
            size: input.len(),
            limit: MAX_INPUT_BYTES,
        })
    } else {
        Ok(input.as_bytes().to_vec())
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
    InputBufferFull,
    InputTooLarge { size: usize, limit: usize },
    InvalidSize { columns: u16, rows: u16 },
    Io(io::Error),
    Pty(String),
    ReadBufferUnavailable,
    SessionNotFound,
    ShellNotAvailable(String),
    TabNotFound,
    WorkspaceNotFound,
    WriterUnavailable,
}

impl TerminalError {
    fn pty(error: impl fmt::Display) -> Self {
        Self::Pty(error.to_string())
    }
}

impl fmt::Display for TerminalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputBufferFull => formatter.write_str("terminal input buffer is full"),
            Self::InputTooLarge { size, limit } => {
                write!(
                    formatter,
                    "terminal input is {size} bytes; limit is {limit}"
                )
            }
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
            Self::ShellNotAvailable(shell) => {
                write!(formatter, "terminal shell is not available: {shell}")
            }
            Self::TabNotFound => formatter.write_str("terminal tab does not exist"),
            Self::WorkspaceNotFound => formatter.write_str("workspace does not exist"),
            Self::WriterUnavailable => formatter.write_str("terminal input writer is unavailable"),
        }
    }
}

impl StdError for TerminalError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::InputBufferFull
            | Self::InputTooLarge { .. }
            | Self::InvalidSize { .. }
            | Self::Pty(_)
            | Self::ReadBufferUnavailable
            | Self::SessionNotFound
            | Self::ShellNotAvailable(_)
            | Self::TabNotFound
            | Self::WorkspaceNotFound
            | Self::WriterUnavailable => None,
        }
    }
}

impl From<io::Error> for TerminalError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Default)]
struct TerminalOutputBuffer {
    bytes: Vec<u8>,
    finished: bool,
    truncated: bool,
}

impl TerminalOutputBuffer {
    fn append(&mut self, bytes: &[u8]) {
        if bytes.len() >= MAX_BUFFERED_OUTPUT_BYTES {
            self.bytes.clear();
            let start = next_utf8_boundary(bytes, bytes.len() - MAX_BUFFERED_OUTPUT_BYTES);
            self.bytes.extend_from_slice(&bytes[start..]);
            self.truncated = true;
            return;
        }

        let overflow = self
            .bytes
            .len()
            .saturating_add(bytes.len())
            .saturating_sub(MAX_BUFFERED_OUTPUT_BYTES);

        if overflow > 0 {
            let end = next_utf8_boundary(&self.bytes, overflow);
            self.bytes.drain(..end);
            self.truncated = true;
        }

        self.bytes.extend_from_slice(bytes);
    }

    fn finish(&mut self) {
        self.finished = true;
    }

    fn drain(&mut self, flush_incomplete: bool) -> (Vec<u8>, bool, bool) {
        let retained_bytes = if self.finished || flush_incomplete {
            0
        } else {
            incomplete_utf8_suffix_len(&self.bytes)
        };
        let incomplete = self.bytes.split_off(self.bytes.len() - retained_bytes);
        let bytes = std::mem::replace(&mut self.bytes, incomplete);
        let truncated = std::mem::take(&mut self.truncated);

        (bytes, truncated, self.finished)
    }
}

fn next_utf8_boundary(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && bytes[index] & 0b1100_0000 == 0b1000_0000 {
        index += 1;
    }

    index
}

fn incomplete_utf8_suffix_len(bytes: &[u8]) -> usize {
    let first_candidate = bytes.len().saturating_sub(3);

    for start in (first_candidate..bytes.len()).rev() {
        let expected_len = utf8_sequence_len(bytes[start]);
        let actual_len = bytes.len() - start;

        if expected_len > actual_len
            && bytes[start + 1..]
                .iter()
                .all(|byte| byte & 0b1100_0000 == 0b1000_0000)
        {
            return actual_len;
        }
    }

    0
}

fn utf8_sequence_len(first_byte: u8) -> usize {
    match first_byte {
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => 1,
    }
}

fn spawn_reader(mut reader: Box<dyn Read + Send>, output: Arc<Mutex<TerminalOutputBuffer>>) {
    thread::spawn(move || {
        let mut buffer = [0; 8192];

        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(bytes_read) => {
                    let Ok(mut output) = output.lock() else {
                        break;
                    };

                    output.append(&buffer[..bytes_read]);
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }

        if let Ok(mut output) = output.lock() {
            output.finish();
        }
    });
}

fn spawn_writer(mut writer: Box<dyn Write + Send>) -> mpsc::SyncSender<Vec<u8>> {
    let (input, pending_input) = mpsc::sync_channel::<Vec<u8>>(MAX_PENDING_INPUTS);

    thread::spawn(move || {
        while let Ok(input) = pending_input.recv() {
            if writer
                .write_all(&input)
                .and_then(|_| writer.flush())
                .is_err()
            {
                break;
            }
        }
    });

    input
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

    #[test]
    fn shell_discovery_ignores_comments_duplicates_and_missing_paths() {
        let shells = discover_shells(
            Some("# system shells\n/bin/bash\n/bin/zsh\n/bin/bash\n/missing"),
            Some("/bin/zsh"),
            |path| path != "/missing",
        );

        assert_eq!(shells.len(), 2);
        assert_eq!(shells[0].name(), "zsh");
        assert_eq!(shells[0].path(), "/bin/zsh");
        assert!(shells[0].is_default());
        assert_eq!(shells[1].name(), "bash");
        assert_eq!(shells[1].path(), "/bin/bash");
    }

    #[test]
    fn shell_discovery_includes_an_available_environment_shell() {
        let shells = discover_shells(Some("/bin/sh"), Some("/custom/fish"), |_| true);

        assert_eq!(shells.len(), 1);
        assert_eq!(shells[0].path(), "/custom/fish");
        assert!(shells[0].is_default());
    }

    #[test]
    fn shell_discovery_keeps_only_unique_interactive_shells() {
        let shells = discover_shells(
            Some("/bin/sh\n/usr/bin/git-shell\n/bin/bash\n/usr/bin/bash\n/usr/bin/fish"),
            Some("/usr/bin/fish"),
            |_| true,
        );

        assert_eq!(
            shells.iter().map(|shell| shell.name()).collect::<Vec<_>>(),
            ["fish", "bash"]
        );
    }

    #[test]
    fn terminal_input_size_is_bounded() {
        let input = "x".repeat(MAX_INPUT_BYTES + 1);
        let error = terminal_input(&input).expect_err("oversized input should fail");

        assert!(matches!(
            error,
            TerminalError::InputTooLarge { size, limit }
                if size == MAX_INPUT_BYTES + 1 && limit == MAX_INPUT_BYTES
        ));
    }

    #[test]
    fn terminal_commands_do_not_expose_sidecar_environment() {
        let mut command = CommandBuilder::new("/bin/sh");
        for variable in SIDECAR_ENVIRONMENT_VARIABLES {
            command.env(variable, "private");
        }

        let command = terminal_command(command, Path::new("/workspace"));

        for variable in SIDECAR_ENVIRONMENT_VARIABLES {
            assert!(command.get_env(variable).is_none());
        }
        assert_eq!(
            command.get_env("TERM"),
            Some(std::ffi::OsStr::new("xterm-256color"))
        );
    }

    #[test]
    fn terminal_output_buffer_is_bounded() {
        let mut output = TerminalOutputBuffer::default();

        output.append(&vec![b'x'; MAX_BUFFERED_OUTPUT_BYTES + 32]);
        let (bytes, truncated, _) = output.drain(false);

        assert_eq!(bytes.len(), MAX_BUFFERED_OUTPUT_BYTES);
        assert!(truncated);
    }

    #[test]
    fn terminal_output_buffer_preserves_incomplete_utf8_sequences() {
        let mut output = TerminalOutputBuffer::default();

        output.append(&[0xE2, 0x82]);
        assert_eq!(output.drain(false), (Vec::new(), false, false));

        output.append(&[0xAC]);
        let (bytes, truncated, _) = output.drain(false);

        assert_eq!(String::from_utf8(bytes).unwrap(), "\u{20ac}");
        assert!(!truncated);
    }

    #[test]
    fn terminal_output_buffer_truncates_at_utf8_boundaries() {
        let mut output = TerminalOutputBuffer::default();
        let bytes = [0xE2, 0x82, 0xAC].repeat(MAX_BUFFERED_OUTPUT_BYTES / 3 + 2);

        output.append(&bytes);
        let (bytes, truncated, _) = output.drain(false);

        assert!(String::from_utf8(bytes).is_ok());
        assert!(truncated);
    }

    #[test]
    fn terminal_output_buffer_flushes_incomplete_utf8_after_reader_finishes() {
        let mut output = TerminalOutputBuffer::default();
        output.append(&[0xE2, 0x82]);
        output.finish();

        let (bytes, _, finished) = output.drain(false);

        assert_eq!(bytes, [0xE2, 0x82]);
        assert!(finished);
    }
}
