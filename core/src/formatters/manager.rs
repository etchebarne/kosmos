use std::collections::BTreeMap;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;

use super::catalog::{formatter_catalog, formatter_definition, formatters_for_language};
use super::installation::{
    FormatterPaths, clean_temporary_directories, install, installation_supported,
    installed_executable, installed_version, node_version, uninstall,
};
use super::{FormatterError, FormatterFailure, FormatterInstallationState, FormatterStatus};

const COMMAND_QUEUE_CAPACITY: usize = 4;
const FORMAT_TIMEOUT: Duration = Duration::from_secs(5);
const IO_DRAIN_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_FORMATTED_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone)]
pub struct FormatterManager {
    inner: Arc<ManagerInner>,
}

struct ManagerInner {
    paths: FormatterPaths,
    entries: Arc<Mutex<BTreeMap<String, ManagerEntry>>>,
    sender: SyncSender<ManagerCommand>,
}

struct WorkerContext {
    paths: FormatterPaths,
    entries: Arc<Mutex<BTreeMap<String, ManagerEntry>>>,
}

struct ManagerEntry {
    installed_version: Option<String>,
    operation: ManagerOperation,
}

enum ManagerOperation {
    Idle,
    Installing,
    Uninstalling,
    Failed(FormatterFailure),
}

enum ManagerCommand {
    Install(&'static str),
    Uninstall(&'static str),
}

impl FormatterManager {
    pub fn open(paths: FormatterPaths) -> Result<Self, FormatterError> {
        paths.prepare()?;
        clean_temporary_directories(&paths);
        let entries = formatter_catalog()
            .iter()
            .map(|definition| {
                (
                    definition.id.to_owned(),
                    ManagerEntry {
                        installed_version: installed_version(&paths, definition),
                        operation: ManagerOperation::Idle,
                    },
                )
            })
            .collect();
        let entries = Arc::new(Mutex::new(entries));
        let (sender, receiver) = mpsc::sync_channel(COMMAND_QUEUE_CAPACITY);
        let worker = WorkerContext {
            paths: paths.clone(),
            entries: Arc::clone(&entries),
        };
        thread::Builder::new()
            .name("kosmos-formatter-installer".to_owned())
            .spawn(move || worker.run(receiver))
            .map_err(|error| FormatterError::WorkerUnavailable(error.to_string()))?;
        Ok(Self {
            inner: Arc::new(ManagerInner {
                paths,
                entries,
                sender,
            }),
        })
    }

    pub fn list(&self) -> Vec<FormatterStatus> {
        formatter_catalog()
            .iter()
            .filter_map(|definition| self.status(definition.id).ok())
            .collect()
    }

    pub fn status(&self, formatter_id: &str) -> Result<FormatterStatus, FormatterError> {
        let definition = formatter_definition(formatter_id)
            .ok_or_else(|| FormatterError::UnknownFormatter(formatter_id.to_owned()))?;
        let entries = self
            .inner
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let entry = entries
            .get(formatter_id)
            .expect("formatter catalog entries are initialized");
        Ok(FormatterStatus {
            id: definition.id.to_owned(),
            name: definition.name.to_owned(),
            description: definition.description.to_owned(),
            languages: definition
                .languages
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            language_ids: definition
                .language_ids
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            catalog_version: definition.version.to_owned(),
            installed_version: entry.installed_version.clone(),
            installation_state: match entry.operation {
                ManagerOperation::Idle if entry.installed_version.is_some() => {
                    FormatterInstallationState::Installed
                }
                ManagerOperation::Idle => FormatterInstallationState::NotInstalled,
                ManagerOperation::Installing => FormatterInstallationState::Installing,
                ManagerOperation::Uninstalling => FormatterInstallationState::Uninstalling,
                ManagerOperation::Failed(_) => FormatterInstallationState::Failed,
            },
            last_error: match &entry.operation {
                ManagerOperation::Failed(error) => Some(error.clone()),
                _ => None,
            },
            supported: installation_supported(),
        })
    }

    pub fn install(&self, formatter_id: &str) -> Result<FormatterStatus, FormatterError> {
        let definition = formatter_definition(formatter_id)
            .ok_or_else(|| FormatterError::UnknownFormatter(formatter_id.to_owned()))?;
        if !installation_supported() {
            return Err(FormatterError::UnsupportedPlatform);
        }
        {
            let mut entries = self
                .inner
                .entries
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let entry = entries
                .get_mut(formatter_id)
                .expect("formatter entry exists");
            if matches!(
                entry.operation,
                ManagerOperation::Installing | ManagerOperation::Uninstalling
            ) {
                return Err(FormatterError::OperationInProgress);
            }
            if entry.installed_version.as_deref() == Some(definition.version)
                && installed_executable(&self.inner.paths, definition).is_some()
            {
                entry.operation = ManagerOperation::Idle;
                drop(entries);
                return self.status(formatter_id);
            }
            entry.operation = ManagerOperation::Installing;
        }
        if let Err(error) = self.try_send(ManagerCommand::Install(definition.id)) {
            fail_entry(&self.inner.entries, definition.id, &error);
            return Err(error);
        }
        self.status(formatter_id)
    }

    pub fn uninstall(&self, formatter_id: &str) -> Result<FormatterStatus, FormatterError> {
        let definition = formatter_definition(formatter_id)
            .ok_or_else(|| FormatterError::UnknownFormatter(formatter_id.to_owned()))?;
        {
            let mut entries = self
                .inner
                .entries
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let entry = entries
                .get_mut(formatter_id)
                .expect("formatter entry exists");
            if matches!(
                entry.operation,
                ManagerOperation::Installing | ManagerOperation::Uninstalling
            ) {
                return Err(FormatterError::OperationInProgress);
            }
            if entry.installed_version.is_none() {
                entry.operation = ManagerOperation::Idle;
                drop(entries);
                return self.status(formatter_id);
            }
            entry.operation = ManagerOperation::Uninstalling;
        }
        if let Err(error) = self.try_send(ManagerCommand::Uninstall(definition.id)) {
            fail_entry(&self.inner.entries, definition.id, &error);
            return Err(error);
        }
        self.status(formatter_id)
    }

    pub fn format(
        &self,
        language_id: &str,
        workspace_root: &Path,
        absolute_path: &Path,
        text: &str,
    ) -> Result<Option<String>, FormatterError> {
        for definition in formatters_for_language(language_id) {
            let executable = {
                let entries = self
                    .inner
                    .entries
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                let entry = entries.get(definition.id).expect("formatter entry exists");
                if matches!(entry.operation, ManagerOperation::Uninstalling)
                    || entry.installed_version.is_none()
                {
                    continue;
                }
                installed_executable(&self.inner.paths, definition)
            };
            if let Some(executable) = executable {
                return run_formatter(&executable, workspace_root, absolute_path, text).map(Some);
            }
        }
        Ok(None)
    }

    fn try_send(&self, command: ManagerCommand) -> Result<(), FormatterError> {
        self.inner
            .sender
            .try_send(command)
            .map_err(|error| match error {
                TrySendError::Full(_) => FormatterError::WorkerBusy,
                TrySendError::Disconnected(_) => {
                    FormatterError::WorkerUnavailable("formatter installer stopped".to_owned())
                }
            })
    }
}

impl WorkerContext {
    fn run(&self, receiver: Receiver<ManagerCommand>) {
        while let Ok(command) = receiver.recv() {
            let (id, result) = match command {
                ManagerCommand::Install(id) => {
                    let definition = formatter_definition(id).expect("formatter exists");
                    (id, install(&self.paths, definition))
                }
                ManagerCommand::Uninstall(id) => {
                    let definition = formatter_definition(id).expect("formatter exists");
                    (id, uninstall(&self.paths, definition))
                }
            };
            let mut entries = self
                .entries
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let entry = entries.get_mut(id).expect("formatter entry exists");
            match result {
                Ok(()) => {
                    entry.installed_version = installed_version(
                        &self.paths,
                        formatter_definition(id).expect("formatter exists"),
                    );
                    entry.operation = ManagerOperation::Idle;
                }
                Err(error) => {
                    entry.operation = ManagerOperation::Failed(FormatterFailure {
                        code: error.code().to_owned(),
                        message: error.to_string(),
                    });
                }
            }
        }
    }
}

fn run_formatter(
    executable: &Path,
    workspace_root: &Path,
    absolute_path: &Path,
    text: &str,
) -> Result<String, FormatterError> {
    if !absolute_path.starts_with(workspace_root) {
        return Err(FormatterError::InvalidDocument(
            "formatter path is outside the workspace".to_owned(),
        ));
    }
    let mut command = Command::new(executable);
    command
        .arg("--stdin-filepath")
        .arg(absolute_path)
        .current_dir(workspace_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    if node_version().is_some_and(|version| ((22, 6)..(24, 3)).contains(&version)) {
        let options = std::env::var("NODE_OPTIONS").unwrap_or_default();
        command.env(
            "NODE_OPTIONS",
            format!("{options} --experimental-strip-types").trim(),
        );
    }
    let mut child = command
        .spawn()
        .map_err(|error| FormatterError::Execution(error.to_string()))?;
    let process_group = Pid::from_raw(i32::try_from(child.id()).unwrap_or(i32::MAX));
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| FormatterError::Execution("formatter stdin was unavailable".to_owned()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| FormatterError::Execution("formatter stdout was unavailable".to_owned()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| FormatterError::Execution("formatter stderr was unavailable".to_owned()))?;
    let input = text.as_bytes().to_vec();
    let (writer_sender, writer_result) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = writer_sender.send(std::io::Write::write_all(&mut stdin, &input));
    });
    let (output_sender, output_result) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = output_sender.send(read_bounded(stdout, MAX_FORMATTED_BYTES));
    });
    let (error_sender, error_result) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = error_sender.send(read_bounded(stderr, 256 * 1024));
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait().map_err(FormatterError::io)? {
            Some(status) => break status,
            None if started.elapsed() < FORMAT_TIMEOUT => thread::sleep(Duration::from_millis(10)),
            None => {
                let _ = killpg(process_group, Signal::SIGKILL);
                let _ = child.kill();
                let _ = child.wait();
                return Err(FormatterError::Timeout);
            }
        }
    };
    let _ = killpg(process_group, Signal::SIGKILL);
    writer_result
        .recv_timeout(IO_DRAIN_TIMEOUT)
        .map_err(|_| FormatterError::Execution("formatter input did not close".to_owned()))?
        .map_err(FormatterError::io)?;
    let output = output_result
        .recv_timeout(IO_DRAIN_TIMEOUT)
        .map_err(|_| FormatterError::Execution("formatter output did not close".to_owned()))??;
    let errors = error_result
        .recv_timeout(IO_DRAIN_TIMEOUT)
        .map_err(|_| FormatterError::Execution("formatter errors did not close".to_owned()))??;
    if !status.success() {
        return Err(FormatterError::Execution(
            String::from_utf8_lossy(&errors).trim().to_owned(),
        ));
    }
    String::from_utf8(output).map_err(|error| FormatterError::InvalidOutput(error.to_string()))
}

fn read_bounded(mut reader: impl Read, limit: usize) -> Result<Vec<u8>, FormatterError> {
    let mut output = Vec::new();
    reader
        .by_ref()
        .take((limit + 1) as u64)
        .read_to_end(&mut output)
        .map_err(FormatterError::io)?;
    if output.len() > limit {
        Err(FormatterError::OutputTooLarge)
    } else {
        Ok(output)
    }
}

fn fail_entry(
    entries: &Mutex<BTreeMap<String, ManagerEntry>>,
    formatter_id: &str,
    error: &FormatterError,
) {
    let mut entries = entries.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = entries.get_mut(formatter_id) {
        entry.operation = ManagerOperation::Failed(FormatterFailure {
            code: error.code().to_owned(),
            message: error.to_string(),
        });
    }
}

impl std::fmt::Debug for FormatterManager {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FormatterManager")
            .finish_non_exhaustive()
    }
}
