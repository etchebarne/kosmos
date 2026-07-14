use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::CommandBuilder;
use zbus::blocking::{Connection, Proxy, connection};
use zbus::zvariant::{DynamicType, OwnedObjectPath, OwnedValue, Value};

const TERMINAL_HOST_MODE: &str = "--kosmos-terminal-host";
const WORKLOAD_SLICE: &str = "app-kosmos-workloads.slice";
const MANAGER_DESTINATION: &str = "org.freedesktop.systemd1";
const MANAGER_PATH: &str = "/org/freedesktop/systemd1";
const MANAGER_INTERFACE: &str = "org.freedesktop.systemd1.Manager";
const CGROUP_CONTROLLERS: &str = "/sys/fs/cgroup/cgroup.controllers";
const CGROUP_MEMBERSHIP: &str = "/proc/self/cgroup";
const MEMINFO: &str = "/proc/meminfo";
const OOM_SCORE_ADJ: &str = "/proc/self/oom_score_adj";
const BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(10);
const DBUS_METHOD_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_READINESS_MESSAGE_BYTES: usize = 512;

static NEXT_SCOPE_ID: AtomicU64 = AtomicU64::new(1);
static SCOPE_STOPPER: OnceLock<Arc<dyn ScopeStopper>> = OnceLock::new();

type Property = (String, PropertyValue);
type AuxiliaryUnit = (String, Vec<(String, OwnedValue)>);

#[derive(Clone, Debug, Eq, PartialEq)]
enum PropertyValue {
    Boolean(bool),
    Text(String),
    Unsigned(u64),
    ProcessIds(Vec<u32>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MemoryLimits {
    high: u64,
    max: u64,
    swap_max: u64,
}

impl MemoryLimits {
    fn from_meminfo(meminfo: &str, percent: f64) -> Result<Self, WorkloadError> {
        let total_kib = parse_mem_total_kib(meminfo)?;
        let total_bytes = total_kib.saturating_mul(1024);
        let millionths = (percent * 1_000_000.0).round() as u128;
        let max =
            ((u128::from(total_bytes) * millionths) / 100_000_000).min(u128::from(u64::MAX)) as u64;

        Ok(Self {
            high: percentage(max, 80),
            max,
            swap_max: percentage(max, 40),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct JobCompletion {
    path: String,
    result: String,
}

trait WorkloadManager {
    fn version(&self) -> Result<u32, WorkloadError>;
    fn subscribe(&mut self) -> Result<(), WorkloadError>;
    fn unit_exists(&self, unit: &str) -> Result<bool, WorkloadError>;
    fn set_unit_properties(&self, unit: &str, properties: &[Property])
    -> Result<(), WorkloadError>;
    fn start_transient_unit(
        &self,
        unit: &str,
        properties: &[Property],
    ) -> Result<String, WorkloadError>;
    fn wait_for_job(&self, path: &str, timeout: Duration) -> Result<JobCompletion, WorkloadError>;
    fn stop_unit(&self, unit: &str) -> Result<(), WorkloadError>;
}

trait ScopeStopper: Send + Sync {
    fn stop(&self, unit: &str) -> Result<(), WorkloadError>;
}

struct SystemdScopeStopper {
    manager: Mutex<Option<SystemdManager>>,
}

impl ScopeStopper for SystemdScopeStopper {
    fn stop(&self, unit: &str) -> Result<(), WorkloadError> {
        let mut manager = self
            .manager
            .lock()
            .map_err(|_| WorkloadError::Manager("workload controller lock is poisoned".into()))?;
        let mut last_error = None;
        for _ in 0..2 {
            if manager.is_none() {
                match SystemdManager::connect() {
                    Ok(connected) => *manager = Some(connected),
                    Err(error) => {
                        last_error = Some(error);
                        continue;
                    }
                }
            }
            let result = manager
                .as_ref()
                .expect("workload manager was initialized")
                .stop_unit(unit);
            if result.is_ok() {
                return Ok(());
            }
            last_error = result.err();
            *manager = None;
        }
        Err(last_error.unwrap_or_else(|| {
            WorkloadError::Manager(format!("could not stop workload scope {unit}"))
        }))
    }
}

pub(crate) struct WorkloadScope {
    unit: String,
    stopper: Option<Arc<dyn ScopeStopper>>,
    stopped: bool,
}

impl WorkloadScope {
    fn new(unit: String) -> Self {
        Self {
            unit,
            stopper: Some(scope_stopper()),
            stopped: false,
        }
    }

    pub(crate) fn stop(&mut self) {
        if self.stopped {
            return;
        }
        if let Some(stopper) = &self.stopper {
            match stopper.stop(&self.unit) {
                Ok(()) => self.stopped = true,
                Err(error) => eprintln!(
                    "could not stop terminal workload scope {}: {error}",
                    self.unit
                ),
            }
        } else {
            self.stopped = true;
        }
    }

    #[cfg(test)]
    fn with_stopper(unit: String, stopper: Arc<dyn ScopeStopper>) -> Self {
        Self {
            unit,
            stopper: Some(stopper),
            stopped: false,
        }
    }
}

impl Drop for WorkloadScope {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(crate) struct TerminalHost {
    executable: PathBuf,
    arguments: Vec<OsString>,
    listener: UnixListener,
    socket_path: PathBuf,
    scope_name: String,
}

impl TerminalHost {
    pub(crate) fn prepare(
        shell: &Path,
        login_shell: bool,
        memory_limit_percent: f64,
    ) -> Result<Self, WorkloadError> {
        let scope_name = next_scope_name();
        let socket_path = readiness_socket_path(&scope_name)?;
        let listener = bind_readiness_socket(&socket_path)?;
        let executable = env::current_exe().map_err(WorkloadError::Io)?;
        let mut arguments = vec![
            OsString::from(TERMINAL_HOST_MODE),
            OsString::from("--scope"),
            OsString::from(&scope_name),
            OsString::from("--memory-percent"),
            OsString::from(memory_limit_percent.to_string()),
            OsString::from("--readiness"),
            socket_path.as_os_str().to_owned(),
            OsString::from("--shell"),
            shell.as_os_str().to_owned(),
        ];
        if login_shell {
            arguments.push(OsString::from("--login"));
        }

        Ok(Self {
            executable,
            arguments,
            listener,
            socket_path,
            scope_name,
        })
    }

    pub(crate) fn command(&self) -> CommandBuilder {
        let mut command = CommandBuilder::new(self.executable.as_os_str());
        command.args(&self.arguments);
        command
    }

    pub(crate) fn finish(
        &self,
        expected_process_id: u32,
    ) -> Result<Option<WorkloadScope>, WorkloadError> {
        let mut stream = accept_readiness(&self.listener, expected_process_id, BOOTSTRAP_TIMEOUT)?;
        let readiness = read_readiness(&mut stream, BOOTSTRAP_TIMEOUT)?;

        match readiness {
            Readiness::Scoped => Ok(Some(WorkloadScope::new(self.scope_name.clone()))),
            Readiness::Degraded => Ok(None),
        }
    }

    pub(crate) fn stop_scope(&self) {
        let _ = scope_stopper().stop(&self.scope_name);
    }
}

pub(crate) fn current_executable_supports_terminal_host() -> bool {
    let Ok(executable) = env::current_exe() else {
        return true;
    };
    let is_test_harness = executable
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|parent| parent == "deps")
        && executable
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.starts_with("core-") || name.starts_with("kosmos_server-"));
    !is_test_harness
}

impl Drop for TerminalHost {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket_path);
    }
}

#[derive(Debug)]
pub(crate) enum WorkloadError {
    Bootstrap(String),
    InvalidArguments(String),
    Io(io::Error),
    Manager(String),
    ManagerUnavailable(String),
    ScopeStart(String),
}

impl fmt::Display for WorkloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bootstrap(message) => write!(formatter, "terminal bootstrap failed: {message}"),
            Self::InvalidArguments(message) => {
                write!(formatter, "invalid terminal host arguments: {message}")
            }
            Self::Io(error) => write!(formatter, "terminal workload I/O failed: {error}"),
            Self::Manager(message) => {
                write!(formatter, "systemd workload manager failed: {message}")
            }
            Self::ManagerUnavailable(message) => {
                write!(formatter, "user systemd manager is unavailable: {message}")
            }
            Self::ScopeStart(message) => {
                write!(formatter, "terminal scope attachment failed: {message}")
            }
        }
    }
}

impl std::error::Error for WorkloadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for WorkloadError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn run_terminal_host(arguments: impl IntoIterator<Item = OsString>) -> Option<io::Result<()>> {
    let mut arguments = arguments.into_iter();
    let _executable = arguments.next();
    if arguments.next().as_deref() != Some(OsStr::new(TERMINAL_HOST_MODE)) {
        return None;
    }

    Some(
        parse_terminal_host_arguments(arguments)
            .and_then(run_terminal_host_inner)
            .map_err(io::Error::other),
    )
}

struct TerminalHostArguments {
    scope_name: String,
    memory_limit_percent: f64,
    readiness_path: PathBuf,
    shell: PathBuf,
    login_shell: bool,
}

fn parse_terminal_host_arguments(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<TerminalHostArguments, WorkloadError> {
    let mut arguments = arguments.into_iter();
    let mut scope_name = None;
    let mut memory_limit_percent = None;
    let mut readiness_path = None;
    let mut shell = None;
    let mut login_shell = false;

    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--scope") if scope_name.is_none() => {
                scope_name = Some(next_argument(&mut arguments, "--scope")?);
            }
            Some("--memory-percent") if memory_limit_percent.is_none() => {
                memory_limit_percent = Some(next_argument(&mut arguments, "--memory-percent")?);
            }
            Some("--readiness") if readiness_path.is_none() => {
                readiness_path = Some(PathBuf::from(next_argument(&mut arguments, "--readiness")?));
            }
            Some("--shell") if shell.is_none() => {
                shell = Some(PathBuf::from(next_argument(&mut arguments, "--shell")?));
            }
            Some("--login") if !login_shell => login_shell = true,
            _ => {
                return Err(WorkloadError::InvalidArguments(format!(
                    "unexpected or duplicate argument {:?}",
                    argument
                )));
            }
        }
    }

    let scope_name = scope_name
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| WorkloadError::InvalidArguments("missing or invalid --scope".into()))?;
    if !safe_scope_name(&scope_name) {
        return Err(WorkloadError::InvalidArguments(
            "scope name is not safe".into(),
        ));
    }
    let memory_limit_percent = memory_limit_percent
        .and_then(|value| value.into_string().ok())
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && (10.0..=75.0).contains(value))
        .ok_or_else(|| WorkloadError::InvalidArguments("invalid --memory-percent".into()))?;
    let readiness_path = readiness_path
        .filter(|path| path.is_absolute())
        .ok_or_else(|| WorkloadError::InvalidArguments("invalid --readiness".into()))?;
    let shell = shell
        .filter(|path| path.is_absolute())
        .ok_or_else(|| WorkloadError::InvalidArguments("invalid --shell".into()))?;

    Ok(TerminalHostArguments {
        scope_name,
        memory_limit_percent,
        readiness_path,
        shell,
        login_shell,
    })
}

fn next_argument(
    arguments: &mut impl Iterator<Item = OsString>,
    option: &str,
) -> Result<OsString, WorkloadError> {
    arguments
        .next()
        .ok_or_else(|| WorkloadError::InvalidArguments(format!("{option} requires a value")))
}

fn run_terminal_host_inner(arguments: TerminalHostArguments) -> Result<(), WorkloadError> {
    let mut readiness =
        UnixStream::connect(&arguments.readiness_path).map_err(WorkloadError::Io)?;
    set_close_on_exec(&readiness)?;

    if !cgroup_v2_available() {
        return run_degraded_terminal(arguments, readiness, "cgroup v2 is unavailable");
    }

    let mut manager = match SystemdManager::connect() {
        Ok(manager) => manager,
        Err(WorkloadError::ManagerUnavailable(reason)) => {
            return run_degraded_terminal(arguments, readiness, &reason);
        }
        Err(error) => {
            notify_error(&mut readiness, &error);
            return Err(error);
        }
    };
    let limits = match fs::read_to_string(MEMINFO)
        .map_err(WorkloadError::Io)
        .and_then(|meminfo| MemoryLimits::from_meminfo(&meminfo, arguments.memory_limit_percent))
        .and_then(|limits| {
            ensure_workload_slice(&mut manager, limits)?;
            Ok(limits)
        }) {
        Ok(limits) => limits,
        Err(WorkloadError::ManagerUnavailable(reason)) => {
            return run_degraded_terminal(arguments, readiness, &reason);
        }
        Err(error) => {
            notify_error(&mut readiness, &error);
            return Err(error);
        }
    };

    match start_terminal_scope(&mut manager, &arguments.scope_name, limits) {
        Ok(()) => {}
        Err(WorkloadError::ManagerUnavailable(reason)) => {
            return run_degraded_terminal(arguments, readiness, &reason);
        }
        Err(error) => {
            let _ = manager.stop_unit(&arguments.scope_name);
            notify_error(&mut readiness, &error);
            return Err(error);
        }
    }
    if let Err(error) = verify_cgroup_membership(
        &fs::read_to_string(CGROUP_MEMBERSHIP).map_err(WorkloadError::Io)?,
        &arguments.scope_name,
    ) {
        let _ = manager.stop_unit(&arguments.scope_name);
        notify_error(&mut readiness, &error);
        return Err(error);
    }
    drop(manager);

    set_oom_score_adjustment();
    readiness
        .write_all(b"scoped\n")
        .and_then(|_| readiness.flush())
        .map_err(WorkloadError::Io)?;
    exec_shell(arguments, readiness)
}

fn run_degraded_terminal(
    arguments: TerminalHostArguments,
    mut readiness: UnixStream,
    reason: &str,
) -> Result<(), WorkloadError> {
    eprintln!("Kosmos terminal workload containment unavailable: {reason}; starting uncontained.");
    set_oom_score_adjustment();
    readiness
        .write_all(b"degraded\n")
        .and_then(|_| readiness.flush())
        .map_err(WorkloadError::Io)?;
    exec_shell(arguments, readiness)
}

fn exec_shell(
    arguments: TerminalHostArguments,
    mut readiness: UnixStream,
) -> Result<(), WorkloadError> {
    let argv0 = if arguments.login_shell {
        let name = arguments
            .shell
            .file_name()
            .ok_or_else(|| WorkloadError::InvalidArguments("shell has no file name".into()))?;
        let mut argv0 = OsString::from("-");
        argv0.push(name);
        argv0
    } else {
        arguments.shell.as_os_str().to_owned()
    };
    let error = Command::new(&arguments.shell).arg0(argv0).exec();
    let workload_error = WorkloadError::Io(error);
    notify_error(&mut readiness, &workload_error);
    Err(workload_error)
}

fn notify_error(readiness: &mut UnixStream, error: &WorkloadError) {
    let message = error.to_string().replace(['\n', '\r'], " ");
    let message = &message[..message.floor_char_boundary(400)];
    let _ = writeln!(readiness, "error:{message}");
    let _ = readiness.flush();
}

fn ensure_workload_slice(
    manager: &mut impl WorkloadManager,
    limits: MemoryLimits,
) -> Result<(), WorkloadError> {
    let properties = slice_properties(limits);
    if manager.unit_exists(WORKLOAD_SLICE)? {
        return manager.set_unit_properties(WORKLOAD_SLICE, &properties);
    }

    manager.subscribe()?;
    let job = manager.start_transient_unit(WORKLOAD_SLICE, &properties)?;
    require_successful_job(manager.wait_for_job(&job, BOOTSTRAP_TIMEOUT)?)
}

fn start_terminal_scope(
    manager: &mut impl WorkloadManager,
    scope_name: &str,
    limits: MemoryLimits,
) -> Result<(), WorkloadError> {
    manager.subscribe()?;
    let version = manager.version()?;
    let properties = scope_properties(limits, std::process::id(), version >= 253);
    let job = manager
        .start_transient_unit(scope_name, &properties)
        .map_err(|error| WorkloadError::ScopeStart(error.to_string()))?;
    let completion = manager
        .wait_for_job(&job, BOOTSTRAP_TIMEOUT)
        .map_err(|error| WorkloadError::ScopeStart(error.to_string()))?;
    require_successful_job(completion).map_err(|error| WorkloadError::ScopeStart(error.to_string()))
}

fn require_successful_job(completion: JobCompletion) -> Result<(), WorkloadError> {
    if completion.result == "done" {
        Ok(())
    } else {
        Err(WorkloadError::Manager(format!(
            "systemd job {} completed with result {}",
            completion.path, completion.result
        )))
    }
}

fn slice_properties(limits: MemoryLimits) -> Vec<Property> {
    resource_properties("Kosmos development workloads", limits, 2048)
}

fn scope_properties(
    limits: MemoryLimits,
    process_id: u32,
    supports_scope_oom_policy: bool,
) -> Vec<Property> {
    let mut properties = resource_properties("Kosmos terminal workload", limits, 1024);
    properties.push(("PIDs".into(), PropertyValue::ProcessIds(vec![process_id])));
    properties.push(("Slice".into(), PropertyValue::Text(WORKLOAD_SLICE.into())));
    if supports_scope_oom_policy {
        properties.push(("OOMPolicy".into(), PropertyValue::Text("kill".into())));
    }
    properties
}

fn resource_properties(description: &str, limits: MemoryLimits, tasks_max: u64) -> Vec<Property> {
    vec![
        (
            "Description".into(),
            PropertyValue::Text(description.into()),
        ),
        ("MemoryAccounting".into(), PropertyValue::Boolean(true)),
        ("TasksAccounting".into(), PropertyValue::Boolean(true)),
        ("MemoryHigh".into(), PropertyValue::Unsigned(limits.high)),
        ("MemoryMax".into(), PropertyValue::Unsigned(limits.max)),
        (
            "MemorySwapMax".into(),
            PropertyValue::Unsigned(limits.swap_max),
        ),
        ("TasksMax".into(), PropertyValue::Unsigned(tasks_max)),
        (
            "CollectMode".into(),
            PropertyValue::Text("inactive-or-failed".into()),
        ),
    ]
}

struct SystemdManager {
    connection: Connection,
    proxy: Proxy<'static>,
    job_monitor: Option<JobMonitor>,
    version: u32,
}

impl SystemdManager {
    fn connect() -> Result<Self, WorkloadError> {
        let connection = connection::Builder::session()
            .map_err(manager_unavailable)?
            .method_timeout(DBUS_METHOD_TIMEOUT)
            .build()
            .map_err(manager_unavailable)?;
        let proxy = Proxy::new_owned(
            connection.clone(),
            MANAGER_DESTINATION,
            MANAGER_PATH,
            MANAGER_INTERFACE,
        )
        .map_err(manager_error)?;
        let version = proxy
            .get_property::<String>("Version")
            .map_err(manager_error)
            .and_then(|version| parse_systemd_version(&version))?;

        Ok(Self {
            connection,
            proxy,
            job_monitor: None,
            version,
        })
    }
}

impl WorkloadManager for SystemdManager {
    fn version(&self) -> Result<u32, WorkloadError> {
        Ok(self.version)
    }

    fn subscribe(&mut self) -> Result<(), WorkloadError> {
        if self.job_monitor.is_some() {
            return Ok(());
        }

        let signals = self
            .proxy
            .receive_signal("JobRemoved")
            .map_err(manager_error)?;
        self.proxy
            .call::<_, _, ()>("Subscribe", &())
            .map_err(manager_error)?;
        self.job_monitor = Some(JobMonitor::new(self.connection.clone(), signals));
        Ok(())
    }

    fn unit_exists(&self, unit: &str) -> Result<bool, WorkloadError> {
        match self
            .proxy
            .call::<_, _, OwnedObjectPath>("GetUnit", &(unit,))
        {
            Ok(_) => Ok(true),
            Err(zbus::Error::MethodError(name, _, _))
                if name.as_str() == "org.freedesktop.systemd1.NoSuchUnit" =>
            {
                Ok(false)
            }
            Err(error) => Err(manager_error(error)),
        }
    }

    fn set_unit_properties(
        &self,
        unit: &str,
        properties: &[Property],
    ) -> Result<(), WorkloadError> {
        let properties = dbus_properties(properties)?;
        self.proxy
            .call("SetUnitProperties", &(unit, true, properties))
            .map_err(manager_error)
    }

    fn start_transient_unit(
        &self,
        unit: &str,
        properties: &[Property],
    ) -> Result<String, WorkloadError> {
        let properties = dbus_properties(properties)?;
        let auxiliary: Vec<AuxiliaryUnit> = Vec::new();
        let path: OwnedObjectPath = self
            .proxy
            .call(
                "StartTransientUnit",
                &(unit, "replace", properties, auxiliary),
            )
            .map_err(manager_error)?;
        Ok(path.to_string())
    }

    fn wait_for_job(&self, path: &str, timeout: Duration) -> Result<JobCompletion, WorkloadError> {
        self.job_monitor
            .as_ref()
            .ok_or_else(|| WorkloadError::Manager("systemd job monitor is not subscribed".into()))?
            .wait_for(path, timeout)
    }

    fn stop_unit(&self, unit: &str) -> Result<(), WorkloadError> {
        match self
            .proxy
            .call::<_, _, OwnedObjectPath>("StopUnit", &(unit, "replace"))
        {
            Ok(_) => Ok(()),
            Err(zbus::Error::MethodError(name, _, _))
                if name.as_str() == "org.freedesktop.systemd1.NoSuchUnit" =>
            {
                Ok(())
            }
            Err(error) => Err(manager_error(error)),
        }
    }
}

struct JobMonitor {
    completions: mpsc::Receiver<Result<JobCompletion, String>>,
    connection: Option<Connection>,
    worker: Option<thread::JoinHandle<()>>,
}

impl JobMonitor {
    fn new(
        connection: Connection,
        mut signals: zbus::blocking::proxy::SignalIterator<'static>,
    ) -> Self {
        let (sender, completions) = mpsc::channel();
        let worker = thread::spawn(move || {
            for message in &mut signals {
                let completion = message
                    .body()
                    .deserialize::<(u32, OwnedObjectPath, String, String)>()
                    .map(|(_, path, _, result)| JobCompletion {
                        path: path.to_string(),
                        result,
                    })
                    .map_err(|error| error.to_string());
                if sender.send(completion).is_err() {
                    break;
                }
            }
        });

        Self {
            completions,
            connection: Some(connection),
            worker: Some(worker),
        }
    }

    fn wait_for(&self, path: &str, timeout: Duration) -> Result<JobCompletion, WorkloadError> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(WorkloadError::Manager(
                    "timed out waiting for systemd job".into(),
                ));
            }

            match self.completions.recv_timeout(remaining) {
                Ok(Ok(completion)) if completion.path == path => return Ok(completion),
                Ok(Ok(_)) => {}
                Ok(Err(error)) => return Err(WorkloadError::Manager(error)),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err(WorkloadError::Manager(
                        "timed out waiting for systemd job".into(),
                    ));
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(WorkloadError::ManagerUnavailable(
                        "systemd job monitor disconnected".into(),
                    ));
                }
            }
        }
    }
}

impl Drop for JobMonitor {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.take() {
            let _ = connection.close();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn manager_error(error: impl fmt::Display) -> WorkloadError {
    let message = error.to_string();
    if [
        "org.freedesktop.DBus.Error.ServiceUnknown",
        "org.freedesktop.DBus.Error.NameHasNoOwner",
        "disconnected",
        "Connection reset",
        "Broken pipe",
        "NoReply",
        "TimedOut",
        "Timeout",
        "timed out",
    ]
    .iter()
    .any(|marker| message.contains(marker))
    {
        WorkloadError::ManagerUnavailable(message)
    } else {
        WorkloadError::Manager(message)
    }
}

fn manager_unavailable(error: impl fmt::Display) -> WorkloadError {
    WorkloadError::ManagerUnavailable(error.to_string())
}

fn dbus_properties(properties: &[Property]) -> Result<Vec<(String, OwnedValue)>, WorkloadError> {
    properties
        .iter()
        .map(|(name, value)| Ok((name.clone(), dbus_value(value)?)))
        .collect()
}

fn dbus_value(value: &PropertyValue) -> Result<OwnedValue, WorkloadError> {
    match value {
        PropertyValue::Boolean(value) => owned_value(*value),
        PropertyValue::Text(value) => owned_value(value.clone()),
        PropertyValue::Unsigned(value) => owned_value(*value),
        PropertyValue::ProcessIds(value) => owned_value(value.clone()),
    }
}

fn owned_value<T>(value: T) -> Result<OwnedValue, WorkloadError>
where
    T: DynamicType + Into<Value<'static>>,
{
    Value::new(value).try_to_owned().map_err(manager_error)
}

fn scope_stopper() -> Arc<dyn ScopeStopper> {
    SCOPE_STOPPER
        .get_or_init(|| {
            Arc::new(SystemdScopeStopper {
                manager: Mutex::new(None),
            }) as Arc<dyn ScopeStopper>
        })
        .clone()
}

fn parse_mem_total_kib(meminfo: &str) -> Result<u64, WorkloadError> {
    meminfo
        .lines()
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            (fields.next()? == "MemTotal:")
                .then(|| fields.next()?.parse::<u64>().ok())
                .flatten()
        })
        .ok_or_else(|| WorkloadError::Bootstrap("MemTotal is missing from /proc/meminfo".into()))
}

fn percentage(value: u64, percent: u64) -> u64 {
    ((u128::from(value) * u128::from(percent)) / 100).min(u128::from(u64::MAX)) as u64
}

fn parse_systemd_version(version: &str) -> Result<u32, WorkloadError> {
    let digits = version
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    digits
        .parse()
        .map_err(|_| WorkloadError::Manager(format!("unrecognized systemd version {version:?}")))
}

fn cgroup_v2_available() -> bool {
    Path::new(CGROUP_CONTROLLERS).is_file()
}

fn verify_cgroup_membership(cgroup: &str, scope_name: &str) -> Result<(), WorkloadError> {
    let path = cgroup
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .ok_or_else(|| WorkloadError::Bootstrap("unified cgroup membership is missing".into()))?;
    let components = Path::new(path)
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>();
    if components
        .windows(2)
        .any(|components| components == [WORKLOAD_SLICE, scope_name])
    {
        Ok(())
    } else {
        Err(WorkloadError::Bootstrap(format!(
            "process did not enter expected scope {scope_name}"
        )))
    }
}

fn next_scope_name() -> String {
    let id = NEXT_SCOPE_ID.fetch_add(1, Ordering::Relaxed);
    format!("app-kosmos-terminal-{}-{id}.scope", std::process::id())
}

fn safe_scope_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".scope") else {
        return false;
    };
    !stem.is_empty()
        && stem
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn runtime_directory() -> PathBuf {
    env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join("kosmos")
}

fn readiness_socket_path(scope_name: &str) -> Result<PathBuf, WorkloadError> {
    let directory = runtime_directory();
    fs::create_dir_all(&directory).map_err(WorkloadError::Io)?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
        .map_err(WorkloadError::Io)?;
    Ok(directory.join(format!("{scope_name}.sock")))
}

fn bind_readiness_socket(path: &Path) -> Result<UnixListener, WorkloadError> {
    let listener = UnixListener::bind(path).map_err(WorkloadError::Io)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(WorkloadError::Io)?;
    listener.set_nonblocking(true).map_err(WorkloadError::Io)?;
    Ok(listener)
}

fn accept_readiness(
    listener: &UnixListener,
    expected_process_id: u32,
    timeout: Duration,
) -> Result<UnixStream, WorkloadError> {
    let deadline = Instant::now() + timeout;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                verify_peer_process(&stream, expected_process_id)?;
                return Ok(stream);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(WorkloadError::Bootstrap(
                        "timed out waiting for terminal host".into(),
                    ));
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(WorkloadError::Io(error)),
        }
    }
}

fn verify_peer_process(stream: &UnixStream, expected_process_id: u32) -> Result<(), WorkloadError> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut credentials).cast(),
            &raw mut length,
        )
    };
    if result != 0 {
        return Err(WorkloadError::Io(io::Error::last_os_error()));
    }
    if credentials.pid == expected_process_id as libc::pid_t {
        Ok(())
    } else {
        Err(WorkloadError::Bootstrap(
            "terminal host readiness came from an unexpected process".into(),
        ))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Readiness {
    Scoped,
    Degraded,
}

fn read_readiness(stream: &mut UnixStream, timeout: Duration) -> Result<Readiness, WorkloadError> {
    let deadline = Instant::now() + timeout;
    let mut pending = Vec::new();
    let mut readiness = None;
    let mut buffer = [0; 128];

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(WorkloadError::Bootstrap(
                "timed out waiting for terminal shell exec".into(),
            ));
        }
        stream
            .set_read_timeout(Some(remaining))
            .map_err(WorkloadError::Io)?;

        match stream.read(&mut buffer) {
            Ok(0) if pending.is_empty() => {
                return readiness.ok_or_else(|| {
                    WorkloadError::Bootstrap(
                        "terminal host exited before reporting readiness".into(),
                    )
                });
            }
            Ok(0) => {
                return Err(WorkloadError::Bootstrap(
                    "terminal host sent an incomplete readiness message".into(),
                ));
            }
            Ok(bytes_read) => {
                pending.extend_from_slice(&buffer[..bytes_read]);
                if pending.len() > MAX_READINESS_MESSAGE_BYTES {
                    return Err(WorkloadError::Bootstrap(
                        "terminal host readiness message is too large".into(),
                    ));
                }
                while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
                    let line = pending.drain(..=end).collect::<Vec<_>>();
                    let line = std::str::from_utf8(&line[..line.len() - 1]).map_err(|_| {
                        WorkloadError::Bootstrap("terminal host readiness is not UTF-8".into())
                    })?;
                    if let Some(error) = line.strip_prefix("error:") {
                        return Err(WorkloadError::Bootstrap(error.to_owned()));
                    }
                    let state = match line {
                        "scoped" => Readiness::Scoped,
                        "degraded" => Readiness::Degraded,
                        _ => {
                            return Err(WorkloadError::Bootstrap(
                                "terminal host sent an unknown readiness message".into(),
                            ));
                        }
                    };
                    if readiness.replace(state).is_some() {
                        return Err(WorkloadError::Bootstrap(
                            "terminal host sent duplicate readiness".into(),
                        ));
                    }
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Err(WorkloadError::Bootstrap(
                    "timed out waiting for terminal shell exec".into(),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(WorkloadError::Io(error)),
        }
    }
}

fn set_close_on_exec(stream: &UnixStream) -> Result<(), WorkloadError> {
    let descriptor = stream.as_raw_fd();
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFD) };
    if flags == -1
        || unsafe { libc::fcntl(descriptor, libc::F_SETFD, flags | libc::FD_CLOEXEC) } == -1
    {
        Err(WorkloadError::Io(io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

fn set_oom_score_adjustment() {
    if let Err(error) = write_oom_score_adjustment(Path::new(OOM_SCORE_ADJ)) {
        eprintln!("could not raise terminal workload OOM preference: {error}");
    }
}

fn write_oom_score_adjustment(path: &Path) -> io::Result<()> {
    fs::write(path, "500\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct FakeManager {
        calls: RefCell<Vec<String>>,
        exists: bool,
        fail_start: bool,
        fail_subscribe: bool,
        version: u32,
        properties: RefCell<Vec<Vec<Property>>>,
    }

    impl WorkloadManager for FakeManager {
        fn version(&self) -> Result<u32, WorkloadError> {
            Ok(self.version)
        }

        fn subscribe(&mut self) -> Result<(), WorkloadError> {
            self.calls.borrow_mut().push("subscribe".into());
            if self.fail_subscribe {
                return Err(WorkloadError::ManagerUnavailable("manager stopped".into()));
            }
            Ok(())
        }

        fn unit_exists(&self, _unit: &str) -> Result<bool, WorkloadError> {
            Ok(self.exists)
        }

        fn set_unit_properties(
            &self,
            unit: &str,
            properties: &[Property],
        ) -> Result<(), WorkloadError> {
            self.calls.borrow_mut().push(format!("set:{unit}"));
            self.properties.borrow_mut().push(properties.to_vec());
            Ok(())
        }

        fn start_transient_unit(
            &self,
            unit: &str,
            properties: &[Property],
        ) -> Result<String, WorkloadError> {
            self.calls.borrow_mut().push(format!("start:{unit}"));
            self.properties.borrow_mut().push(properties.to_vec());
            if self.fail_start {
                return Err(WorkloadError::ManagerUnavailable("manager stopped".into()));
            }
            Ok(format!("/jobs/{unit}"))
        }

        fn wait_for_job(
            &self,
            path: &str,
            _timeout: Duration,
        ) -> Result<JobCompletion, WorkloadError> {
            self.calls.borrow_mut().push(format!("wait:{path}"));
            Ok(JobCompletion {
                path: path.into(),
                result: "done".into(),
            })
        }

        fn stop_unit(&self, _unit: &str) -> Result<(), WorkloadError> {
            Ok(())
        }
    }

    struct CountingStopper(AtomicUsize);

    impl ScopeStopper for CountingStopper {
        fn stop(&self, _unit: &str) -> Result<(), WorkloadError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    struct RetryStopper(AtomicUsize);

    impl ScopeStopper for RetryStopper {
        fn stop(&self, _unit: &str) -> Result<(), WorkloadError> {
            if self.0.fetch_add(1, Ordering::Relaxed) == 0 {
                Err(WorkloadError::Manager("temporary failure".into()))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn memory_limits_are_calculated_from_physical_memory() {
        let limits = MemoryLimits::from_meminfo("MemTotal:       1000000 kB\n", 25.0).unwrap();

        assert_eq!(limits.max, 256_000_000);
        assert_eq!(limits.high, 204_800_000);
        assert_eq!(limits.swap_max, 102_400_000);
        assert!(MemoryLimits::from_meminfo("MemFree: 1 kB", 25.0).is_err());
    }

    #[test]
    fn systemd_versions_detect_scope_oom_policy_support() {
        assert_eq!(parse_systemd_version("249.11-0ubuntu3.17").unwrap(), 249);
        assert_eq!(parse_systemd_version("253 (253.5-1)").unwrap(), 253);
        assert!(parse_systemd_version("unknown").is_err());
    }

    #[test]
    fn scope_properties_gate_oom_policy_by_systemd_version() {
        let limits = MemoryLimits {
            high: 80,
            max: 100,
            swap_max: 40,
        };
        let old = scope_properties(limits, 42, false);
        let new = scope_properties(limits, 42, true);

        assert!(!old.iter().any(|(name, _)| name == "OOMPolicy"));
        assert!(new.contains(&("OOMPolicy".into(), PropertyValue::Text("kill".into()))));
        assert!(new.contains(&("PIDs".into(), PropertyValue::ProcessIds(vec![42]))));
        assert!(new.contains(&("MemoryMax".into(), PropertyValue::Unsigned(100))));
        assert!(new.contains(&("TasksMax".into(), PropertyValue::Unsigned(1024))));
    }

    #[test]
    fn manager_subscribes_before_starting_and_waits_for_the_exact_job() {
        let limits = MemoryLimits {
            high: 80,
            max: 100,
            swap_max: 40,
        };
        let mut manager = FakeManager {
            version: 253,
            ..FakeManager::default()
        };

        ensure_workload_slice(&mut manager, limits).unwrap();
        assert_eq!(
            manager.calls.into_inner(),
            vec![
                "subscribe".to_owned(),
                format!("start:{WORKLOAD_SLICE}"),
                format!("wait:/jobs/{WORKLOAD_SLICE}"),
            ]
        );
    }

    #[test]
    fn scope_start_distinguishes_pre_invocation_fallback_from_accepted_failure() {
        let limits = MemoryLimits {
            high: 80,
            max: 100,
            swap_max: 40,
        };
        let mut unavailable_before_start = FakeManager {
            fail_subscribe: true,
            version: 253,
            ..FakeManager::default()
        };
        assert!(matches!(
            start_terminal_scope(&mut unavailable_before_start, "test.scope", limits),
            Err(WorkloadError::ManagerUnavailable(_))
        ));

        let mut unavailable_after_invocation = FakeManager {
            fail_start: true,
            version: 253,
            ..FakeManager::default()
        };
        assert!(matches!(
            start_terminal_scope(&mut unavailable_after_invocation, "test.scope", limits),
            Err(WorkloadError::ScopeStart(_))
        ));
    }

    #[test]
    fn unavailable_systemd_service_is_classified_for_degraded_fallback() {
        assert!(matches!(
            manager_error("org.freedesktop.DBus.Error.ServiceUnknown: missing"),
            WorkloadError::ManagerUnavailable(_)
        ));
    }

    #[test]
    fn scope_names_are_unique_and_safe() {
        let first = next_scope_name();
        let second = next_scope_name();

        assert_ne!(first, second);
        assert!(safe_scope_name(&first));
        assert!(!safe_scope_name("../unsafe.scope"));
    }

    #[test]
    fn cgroup_verification_requires_exact_slice_and_scope_components() {
        let scope = "app-kosmos-terminal-1-1.scope";
        let valid = format!("0::/user.slice/{WORKLOAD_SLICE}/{scope}\n");

        assert!(verify_cgroup_membership(&valid, scope).is_ok());
        assert!(verify_cgroup_membership(&format!("0::/{scope}-other\n"), scope).is_err());
    }

    #[test]
    fn workload_scope_stops_only_once() {
        let stopper = Arc::new(CountingStopper(AtomicUsize::new(0)));
        let mut scope = WorkloadScope::with_stopper("test.scope".into(), stopper.clone());

        scope.stop();
        scope.stop();
        drop(scope);

        assert_eq!(stopper.0.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn workload_scope_retries_a_failed_explicit_stop_when_dropped() {
        let stopper = Arc::new(RetryStopper(AtomicUsize::new(0)));
        let mut scope = WorkloadScope::with_stopper("test.scope".into(), stopper.clone());

        scope.stop();
        drop(scope);

        assert_eq!(stopper.0.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn readiness_requires_status_followed_by_exec_eof() {
        let (mut parent, mut helper) = UnixStream::pair().unwrap();
        helper.write_all(b"scoped\n").unwrap();
        drop(helper);

        assert_eq!(
            read_readiness(&mut parent, Duration::from_secs(1)).unwrap(),
            Readiness::Scoped
        );
    }

    #[test]
    fn readiness_rejects_errors_and_duplicate_transitions() {
        for input in [b"error:failed\n".as_slice(), b"scoped\ndegraded\n"] {
            let (mut parent, mut helper) = UnixStream::pair().unwrap();
            helper.write_all(input).unwrap();
            drop(helper);

            assert!(read_readiness(&mut parent, Duration::from_secs(1)).is_err());
        }
    }

    #[test]
    fn terminal_host_arguments_are_strictly_validated() {
        let arguments = [
            OsString::from("--scope"),
            OsString::from("app-kosmos-terminal-1-1.scope"),
            OsString::from("--memory-percent"),
            OsString::from("25"),
            OsString::from("--readiness"),
            OsString::from("/tmp/ready.sock"),
            OsString::from("--shell"),
            OsString::from("/bin/bash"),
            OsString::from("--login"),
        ];

        let parsed = parse_terminal_host_arguments(arguments).unwrap();
        assert!(parsed.login_shell);
        assert_eq!(parsed.memory_limit_percent, 25.0);
        assert!(parse_terminal_host_arguments([OsString::from("--login")]).is_err());
    }

    #[test]
    fn normal_process_arguments_do_not_dispatch_terminal_host_mode() {
        assert!(run_terminal_host([OsString::from("kosmos-server")]).is_none());
    }

    #[test]
    fn oom_score_adjustment_writes_the_inherited_preference() {
        let path = env::temp_dir().join(format!("kosmos-oom-score-{}", std::process::id()));
        fs::write(&path, "0\n").unwrap();

        write_oom_score_adjustment(&path).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "500\n");
        fs::remove_file(path).unwrap();
    }

    #[test]
    #[ignore = "requires KOSMOS_SYSTEMD_INTEGRATION=1 and a user systemd manager"]
    fn real_cgroup_scope_places_and_stops_child() {
        if env::var("KOSMOS_SYSTEMD_INTEGRATION").as_deref() != Ok("1") {
            return;
        }

        let scope_name =
            env::var("KOSMOS_SYSTEMD_INTEGRATION_SCOPE").unwrap_or_else(|_| next_scope_name());
        if env::var("KOSMOS_SYSTEMD_INTEGRATION_CHILD").as_deref() == Ok("1") {
            assert!(cgroup_v2_available(), "cgroup v2 is unavailable");
            let mut manager =
                SystemdManager::connect().expect("user systemd manager is unavailable");
            let scope_limits = MemoryLimits {
                high: 48 * 1024 * 1024,
                max: 64 * 1024 * 1024,
                swap_max: 24 * 1024 * 1024,
            };
            if !manager
                .unit_exists(WORKLOAD_SLICE)
                .expect("workload slice should be queryable")
            {
                let aggregate_limits =
                    MemoryLimits::from_meminfo(&fs::read_to_string(MEMINFO).unwrap(), 75.0)
                        .unwrap();
                ensure_workload_slice(&mut manager, aggregate_limits)
                    .expect("workload slice should start");
            }
            start_terminal_scope(&mut manager, &scope_name, scope_limits)
                .expect("terminal scope should start");
            verify_cgroup_membership(&fs::read_to_string(CGROUP_MEMBERSHIP).unwrap(), &scope_name)
                .expect("child should enter the workload scope");
            drop(manager);

            let error = Command::new("/bin/sleep").arg("30").exec();
            panic!("integration child could not exec sleep: {error}");
        }

        assert!(cgroup_v2_available(), "cgroup v2 is unavailable");
        let current_cgroup = fs::read_to_string(CGROUP_MEMBERSHIP).unwrap();
        let manager = SystemdManager::connect().expect("user systemd manager is unavailable");
        let slice_existed = manager
            .unit_exists(WORKLOAD_SLICE)
            .expect("workload slice should be queryable");
        let mut child = Command::new(env::current_exe().unwrap())
            .args([
                "--exact",
                "workloads::tests::real_cgroup_scope_places_and_stops_child",
                "--ignored",
                "--nocapture",
            ])
            .env("KOSMOS_SYSTEMD_INTEGRATION", "1")
            .env("KOSMOS_SYSTEMD_INTEGRATION_CHILD", "1")
            .env("KOSMOS_SYSTEMD_INTEGRATION_SCOPE", &scope_name)
            .spawn()
            .expect("integration child should spawn");

        let deadline = Instant::now() + BOOTSTRAP_TIMEOUT;
        let child_cgroup = loop {
            let path = format!("/proc/{}/cgroup", child.id());
            if let Ok(cgroup) = fs::read_to_string(path)
                && verify_cgroup_membership(&cgroup, &scope_name).is_ok()
            {
                break Ok(cgroup);
            }
            if Instant::now() >= deadline {
                break Err("child did not enter the workload scope");
            }
            thread::sleep(Duration::from_millis(20));
        };

        let stop_result = manager.stop_unit(&scope_name);
        let stop_deadline = Instant::now() + Duration::from_secs(2);
        let scope_exit = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) if Instant::now() < stop_deadline => {
                    thread::sleep(Duration::from_millis(20));
                }
                Ok(None) => break Err("scope stop did not terminate the child"),
                Err(_) => break Err("integration child status was unavailable"),
            }
        };
        if scope_exit.is_err() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if !slice_existed {
            let _ = manager.stop_unit(WORKLOAD_SLICE);
        }
        let child_cgroup = child_cgroup.expect("child should enter the workload scope");
        assert_ne!(current_cgroup, child_cgroup);
        stop_result.expect("scope should stop");
        let status = scope_exit.expect("scope stop should terminate the child");
        assert!(!status.success(), "scope stop should terminate the child");
    }
}
