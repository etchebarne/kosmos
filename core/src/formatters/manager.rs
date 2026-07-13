use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use super::catalog::{
    FormatterDefinition, FormatterInvocation, formatter_applies, formatter_catalog,
    formatter_definition,
};
use super::installation::{
    FormatterPaths, clean_temporary_directories, install, installation_supported,
    installed_executable, installed_version, node_version, uninstall,
};
use super::process::{ProcessError, ProcessLimits, run_process, stderr_message};
use super::{FormatterError, FormatterFailure, FormatterInstallationState, FormatterStatus};
use crate::events::ToolingCapabilities;

const COMMAND_QUEUE_CAPACITY: usize = 4;
const FORMAT_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_FORMATTED_BYTES: usize = 2 * 1024 * 1024;
const MAX_FORMATTER_STDERR_BYTES: usize = 256 * 1024;

#[derive(Clone)]
pub struct FormatterManager {
    inner: Arc<ManagerInner>,
}

struct ManagerInner {
    paths: FormatterPaths,
    store: crate::persistence::StateStore,
    entries: Arc<Mutex<BTreeMap<String, ManagerEntry>>>,
    priorities: Arc<Mutex<Vec<&'static str>>>,
    sender: SyncSender<ManagerCommand>,
    tooling: Arc<Mutex<ToolingCapabilities>>,
}

struct WorkerContext {
    paths: FormatterPaths,
    entries: Arc<Mutex<BTreeMap<String, ManagerEntry>>>,
    tooling: Arc<Mutex<ToolingCapabilities>>,
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
    pub fn open(
        paths: FormatterPaths,
        store: crate::persistence::StateStore,
    ) -> Result<Self, FormatterError> {
        paths.prepare()?;
        clean_temporary_directories(&paths);
        let priorities = normalize_priorities(
            store
                .formatter_priorities()
                .map_err(|error| FormatterError::Io(error.to_string()))?,
        );
        store
            .set_formatter_priorities(
                &priorities
                    .iter()
                    .map(|id| (*id).to_owned())
                    .collect::<Vec<_>>(),
            )
            .map_err(|error| FormatterError::Io(error.to_string()))?;
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
        let tooling = Arc::new(Mutex::new(ToolingCapabilities::default()));
        let (sender, receiver) = mpsc::sync_channel(COMMAND_QUEUE_CAPACITY);
        let worker = WorkerContext {
            paths: paths.clone(),
            entries: Arc::clone(&entries),
            tooling: Arc::clone(&tooling),
        };
        thread::Builder::new()
            .name("kosmos-formatter-installer".to_owned())
            .spawn(move || worker.run(receiver))
            .map_err(|error| FormatterError::WorkerUnavailable(error.to_string()))?;
        Ok(Self {
            inner: Arc::new(ManagerInner {
                paths,
                store,
                entries,
                priorities: Arc::new(Mutex::new(priorities)),
                sender,
                tooling,
            }),
        })
    }

    pub fn list(&self) -> Vec<FormatterStatus> {
        self.priority_ids()
            .iter()
            .filter_map(|id| self.status(id).ok())
            .collect()
    }

    pub fn priorities(&self) -> Vec<String> {
        self.priority_ids()
            .iter()
            .map(|id| (*id).to_owned())
            .collect()
    }

    pub fn set_priorities(
        &self,
        formatter_ids: Vec<String>,
    ) -> Result<Vec<FormatterStatus>, FormatterError> {
        let priorities = validate_priorities(&formatter_ids)?;
        self.inner
            .store
            .set_formatter_priorities(&formatter_ids)
            .map_err(|error| FormatterError::Io(error.to_string()))?;
        *self
            .inner
            .priorities
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = priorities;
        self.tooling_changed();
        Ok(self.list())
    }

    pub fn status(&self, formatter_id: &str) -> Result<FormatterStatus, FormatterError> {
        let definition = formatter_definition(formatter_id)
            .ok_or_else(|| FormatterError::UnknownFormatter(formatter_id.to_owned()))?;
        let priorities = self.priority_ids();
        let priority = priorities
            .iter()
            .position(|id| *id == formatter_id)
            .expect("every catalog formatter has a priority");
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
            extensions: definition
                .extensions
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            filenames: definition
                .filenames
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            priority,
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
            supported: installation_supported(definition),
        })
    }

    pub fn install(&self, formatter_id: &str) -> Result<FormatterStatus, FormatterError> {
        let definition = formatter_definition(formatter_id)
            .ok_or_else(|| FormatterError::UnknownFormatter(formatter_id.to_owned()))?;
        if !installation_supported(definition) {
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
        self.tooling_changed();
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
        self.tooling_changed();
        if let Err(error) = self.try_send(ManagerCommand::Uninstall(definition.id)) {
            fail_entry(&self.inner.entries, definition.id, &error);
            return Err(error);
        }
        self.status(formatter_id)
    }

    pub fn format(
        &self,
        language_id: &str,
        relative_path: &Path,
        workspace_root: &Path,
        absolute_path: &Path,
        text: &str,
    ) -> Result<Option<String>, FormatterError> {
        for definition in applicable_formatters(&self.priority_ids(), language_id, relative_path) {
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
                return run_formatter(definition, &executable, workspace_root, absolute_path, text)
                    .map(Some);
            }
        }
        Ok(None)
    }

    pub(crate) fn applicable_formatter(
        &self,
        language_id: &str,
        relative_path: &Path,
    ) -> Option<String> {
        let entries = self
            .inner
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        applicable_formatters(&self.priority_ids(), language_id, relative_path)
            .into_iter()
            .find(|definition| {
                entries.get(definition.id).is_some_and(|entry| {
                    !matches!(entry.operation, ManagerOperation::Uninstalling)
                        && entry.installed_version.is_some()
                        && installed_executable(&self.inner.paths, definition).is_some()
                })
            })
            .map(|definition| definition.id.to_owned())
    }

    pub(crate) fn set_tooling_capabilities(&self, tooling: ToolingCapabilities) {
        *self
            .inner
            .tooling
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = tooling;
    }

    fn priority_ids(&self) -> Vec<&'static str> {
        self.inner
            .priorities
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
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

    fn tooling_changed(&self) {
        self.inner
            .tooling
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .changed();
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
            self.tooling
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .changed();
        }
    }
}

fn normalize_priorities(stored: Vec<String>) -> Vec<&'static str> {
    let mut priorities = Vec::with_capacity(formatter_catalog().len());
    let mut seen = HashSet::new();
    for id in stored {
        if let Some(definition) = formatter_definition(&id)
            && seen.insert(definition.id)
        {
            priorities.push(definition.id);
        }
    }
    for definition in formatter_catalog() {
        if seen.insert(definition.id) {
            priorities.push(definition.id);
        }
    }
    priorities
}

fn validate_priorities(ids: &[String]) -> Result<Vec<&'static str>, FormatterError> {
    if ids.len() != formatter_catalog().len() {
        return Err(FormatterError::InvalidPreferences(
            "priority list must contain every formatter exactly once".to_owned(),
        ));
    }
    let priorities = normalize_priorities(ids.to_vec());
    if priorities.len() != ids.len()
        || priorities
            .iter()
            .zip(ids)
            .any(|(resolved, requested)| *resolved != requested)
    {
        return Err(FormatterError::InvalidPreferences(
            "priority list contains an unknown or duplicate formatter".to_owned(),
        ));
    }
    Ok(priorities)
}

fn applicable_formatters(
    priorities: &[&str],
    language_id: &str,
    relative_path: &Path,
) -> Vec<&'static FormatterDefinition> {
    priorities
        .iter()
        .filter_map(|id| formatter_definition(id))
        .filter(|definition| formatter_applies(definition, language_id, relative_path))
        .collect()
}

fn run_formatter(
    definition: &FormatterDefinition,
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
    match definition.invocation {
        FormatterInvocation::Prettier => {
            command.arg("--stdin-filepath").arg(absolute_path);
        }
        FormatterInvocation::Ruff => {
            command
                .args(["format", "--stdin-filename"])
                .arg(absolute_path)
                .arg("-");
        }
        FormatterInvocation::Shfmt => {
            command.arg("--filename").arg(absolute_path);
        }
    }
    command.current_dir(workspace_root);
    if definition.invocation == FormatterInvocation::Prettier
        && node_version().is_some_and(|version| ((22, 6)..(24, 3)).contains(&version))
    {
        let options = std::env::var("NODE_OPTIONS").unwrap_or_default();
        command.env(
            "NODE_OPTIONS",
            format!("{options} --experimental-strip-types").trim(),
        );
    }
    let output = run_process(
        &mut command,
        Some(text.as_bytes()),
        ProcessLimits {
            timeout: FORMAT_TIMEOUT,
            stdout_bytes: MAX_FORMATTED_BYTES,
            stderr_bytes: MAX_FORMATTER_STDERR_BYTES,
        },
    )
    .map_err(|error| match error {
        ProcessError::Start(error) => FormatterError::Execution(error.to_string()),
        ProcessError::Timeout => FormatterError::Timeout,
        ProcessError::ProcessIdUnavailable => {
            FormatterError::Execution("formatter process id was unavailable".to_owned())
        }
        ProcessError::Input(error) | ProcessError::Wait(error) => FormatterError::io(error),
        ProcessError::InputUnavailable => {
            FormatterError::Execution("formatter stdin was unavailable".to_owned())
        }
        ProcessError::OutputUnavailable => {
            FormatterError::Execution("formatter output was unavailable".to_owned())
        }
        ProcessError::Drain => {
            FormatterError::Execution("formatter output did not close".to_owned())
        }
    })?;
    if output.stdout_truncated {
        return Err(FormatterError::OutputTooLarge);
    }
    if !output.status.success() {
        let errors = stderr_message(&output);
        return Err(FormatterError::Execution(if errors.is_empty() {
            format!("process exited with {}", output.status)
        } else {
            errors
        }));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| FormatterError::InvalidOutput(error.to_string()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn preferences_are_complete_deterministic_and_validated() {
        assert_eq!(
            normalize_priorities(vec!["shfmt".to_owned()]),
            vec!["shfmt", "prettier", "ruff"]
        );
        assert_eq!(
            validate_priorities(&["ruff".to_owned(), "prettier".to_owned(), "shfmt".to_owned()])
                .unwrap(),
            vec!["ruff", "prettier", "shfmt"]
        );
        assert!(validate_priorities(&["prettier".to_owned()]).is_err());
        assert!(
            validate_priorities(&[
                "prettier".to_owned(),
                "prettier".to_owned(),
                "shfmt".to_owned()
            ])
            .is_err()
        );
    }

    #[test]
    fn applicability_respects_persisted_priority_before_lsp_fallback() {
        let defaults = normalize_priorities(Vec::new());
        assert_eq!(
            applicable_formatters(&defaults, "typescript", Path::new("script.py"))
                .into_iter()
                .map(FormatterDefinition::id)
                .collect::<Vec<_>>(),
            vec!["prettier", "ruff"]
        );
        assert_eq!(
            applicable_formatters(
                &["ruff", "prettier", "shfmt"],
                "typescript",
                Path::new("script.py")
            )
            .into_iter()
            .map(FormatterDefinition::id)
            .collect::<Vec<_>>(),
            vec!["ruff", "prettier"]
        );
    }

    #[test]
    fn formatter_invocations_pass_stdin_and_the_canonical_path() {
        let directory = test_directory("invocations");
        let file = directory.join("nested.py");
        fs::write(&file, "source").unwrap();
        let script = directory.join("formatter");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nset -eu\n[ \"$1\" = format ]\n[ \"$2\" = --stdin-filename ]\n[ \"$3\" = '{}' ]\n[ \"$4\" = - ]\ncat\n",
                file.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();

        let formatted = run_formatter(
            formatter_definition("ruff").unwrap(),
            &script,
            &directory,
            &file,
            "print( 1 )\n",
        )
        .unwrap();
        assert_eq!(formatted, "print( 1 )\n");
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn formatter_errors_are_actionable_and_bounded() {
        let directory = test_directory("errors");
        let file = directory.join("test.sh");
        fs::write(&file, "source").unwrap();
        let script = directory.join("formatter");
        fs::write(
            &script,
            "#!/bin/sh\nprintf 'invalid shell syntax at line 2' >&2\nexit 2\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();

        let input = "invalid".repeat(128 * 1024);
        let error = run_formatter(
            formatter_definition("shfmt").unwrap(),
            &script,
            &directory,
            &file,
            &input,
        )
        .unwrap_err();
        assert!(error.to_string().contains("invalid shell syntax at line 2"));
        let _ = fs::remove_dir_all(directory);
    }

    fn test_directory(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "kosmos-formatter-manager-{name}-{}-{}",
            std::process::id(),
            NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        path
    }
}
