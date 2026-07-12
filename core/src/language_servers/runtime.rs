use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

use globset::{GlobBuilder, GlobMatcher};
use notify::event::{ModifyKind, RenameMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, percent_encode};
use serde_json::{Value, json};

use crate::events::{CoreEvent, CoreEventDispatcher, LanguageServerDiagnosticsChanged};
use crate::tree::WorkspaceId;

use super::catalog::{LanguageServerDefinition, language_server_catalog};
use super::edits::{WorkspaceEditTransactions, validate_text_edits};
use super::secure_edit::random_token;
use super::{
    LanguageServerChange, LanguageServerCodeAction, LanguageServerCodeActionRequest,
    LanguageServerCodeActionResolveRequest, LanguageServerColor, LanguageServerColorInformation,
    LanguageServerColorPresentation, LanguageServerColorPresentationRequest,
    LanguageServerCompletionItem, LanguageServerCompletionList, LanguageServerCompletionRequest,
    LanguageServerCompletionResolveRequest, LanguageServerCompletionTextEdit,
    LanguageServerDiagnostic, LanguageServerDiagnosticSeverity, LanguageServerDiagnosticSnapshot,
    LanguageServerDocumentOpen, LanguageServerDocumentSymbol, LanguageServerError,
    LanguageServerExecuteCommandRequest, LanguageServerFormattingOptions, LanguageServerHover,
    LanguageServerHoverContent, LanguageServerLocation, LanguageServerLog, LanguageServerLogKind,
    LanguageServerMarkupKind, LanguageServerParameterInformation, LanguageServerParameterLabel,
    LanguageServerPosition, LanguageServerPrepareRename, LanguageServerRange,
    LanguageServerRequestCancellation, LanguageServerRuntimeState, LanguageServerRuntimeStatus,
    LanguageServerSignatureHelp, LanguageServerSignatureInformation, LanguageServerTextEdit,
    LanguageServerWorkspaceSymbol, LanguageServerWorkspaceSymbolResolveRequest,
    WorkspaceEditOpenDocument, WorkspaceEditRoot,
};

const MAX_LSP_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const FEATURE_TIMEOUT: Duration = Duration::from_secs(3);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
const DIAGNOSTIC_SETTLE_DELAY: Duration = Duration::from_millis(200);
const WATCHED_FILE_DEBOUNCE: Duration = Duration::from_millis(100);
const WATCH_EVENT_QUEUE_CAPACITY: usize = 256;
const MAX_DYNAMIC_REGISTRATIONS: usize = 128;
const MAX_CAPABILITY_STRING_BYTES: usize = 256;
const MAX_WATCH_PATTERNS: usize = 256;
const MAX_GLOB_PATTERN_BYTES: usize = 1_024;
const MAX_PROGRESS_TOKENS: usize = 128;
const MAX_CONFIGURATION_ITEMS: usize = 256;
const MAX_SERVER_LOG_ENTRIES: usize = 200;
const MAX_SERVER_LOG_BYTES: usize = 256 * 1024;
const MAX_SERVER_LOG_ENTRY_BYTES: usize = 16 * 1024;
const MAX_RESTART_ATTEMPTS: u32 = 5;
const STABLE_RUNNING_INTERVAL: Duration = Duration::from_secs(30);
const MAX_WATCHED_FILE_RESYNC_FILES: usize = 4_096;
const MAX_VALIDATED_CODE_ACTIONS: usize = 512;
const MAX_CODE_ACTIONS_PER_REQUEST: usize = 256;
const VALIDATED_CODE_ACTION_TTL: Duration = Duration::from_secs(60);
const INITIAL_RESTART_DELAY: Duration = Duration::from_millis(250);
const MAX_RESTART_DELAY: Duration = Duration::from_secs(4);
const URI_PATH_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'/')
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');
type PendingResponse = Result<Value, LanguageServerError>;
type PendingRequests = Arc<Mutex<HashMap<i64, SyncSender<PendingResponse>>>>;
type PublishedDiagnostics = Arc<Mutex<HashMap<String, DiagnosticSnapshot>>>;

#[derive(Clone, Debug)]
struct MessageWriter {
    sender: SyncSender<OutboundMessage>,
}

#[derive(Debug)]
struct OutboundMessage {
    body: Vec<u8>,
    completion: SyncSender<Result<(), String>>,
}

#[derive(Debug)]
struct DiagnosticSnapshot {
    version: Option<i64>,
    revision: DocumentRevision,
    diagnostics: Vec<LanguageServerDiagnostic>,
    published_at: Instant,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct DocumentRevision {
    generation: u64,
    version: i64,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct PendingDiagnosticKey {
    session: SessionKey,
    epoch: u64,
    uri: String,
}

#[derive(Debug)]
struct PendingDiagnostic {
    version: Option<i64>,
    revision: DocumentRevision,
    diagnostics: Vec<LanguageServerDiagnostic>,
}

type PendingDiagnostics = Arc<Mutex<HashMap<PendingDiagnosticKey, PendingDiagnostic>>>;

#[derive(Debug)]
pub(crate) struct LanguageServerRuntime {
    sessions: Mutex<HashMap<SessionKey, Arc<LanguageServerSession>>>,
    supervision: Mutex<HashMap<SessionKey, SessionSupervision>>,
    documents: Arc<Mutex<HashMap<DocumentKey, DocumentBinding>>>,
    open_workspaces: Mutex<std::collections::HashSet<WorkspaceId>>,
    logs: RuntimeLogs,
    events: CoreEventDispatcher,
    supervisor: SyncSender<SupervisorCommand>,
    pending_diagnostics: PendingDiagnostics,
    workspace_edits: Arc<WorkspaceEditTransactions>,
    next_code_action_id: AtomicU64,
    validated_code_actions: Mutex<HashMap<u64, ValidatedCodeAction>>,
}

#[derive(Clone, Debug)]
struct RuntimeLogs {
    buffers: Arc<Mutex<HashMap<String, LogBuffer>>>,
    events: CoreEventDispatcher,
}

impl Default for RuntimeLogs {
    fn default() -> Self {
        Self {
            buffers: Arc::new(Mutex::new(HashMap::new())),
            events: CoreEventDispatcher::default(),
        }
    }
}

#[derive(Debug, Default)]
struct LogBuffer {
    entries: VecDeque<LanguageServerLog>,
    bytes: usize,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct SessionKey {
    workspace_id: WorkspaceId,
    server_id: &'static str,
    project_root: PathBuf,
    workspace_root: PathBuf,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct DocumentKey {
    workspace_id: WorkspaceId,
    path: String,
    server_id: &'static str,
}

#[derive(Debug)]
struct DocumentBinding {
    generation: u64,
    version: i64,
    uri: String,
    language_id: String,
    text: String,
    session_key: SessionKey,
    session: Arc<LanguageServerSession>,
}

#[derive(Debug)]
struct ValidatedCodeAction {
    created_at: Instant,
    workspace_id: WorkspaceId,
    path: String,
    generation: u64,
    version: i64,
    server_id: String,
    raw: Value,
    session: Arc<LanguageServerSession>,
    resolved: bool,
    command: Option<AuthorizedCommand>,
}

#[derive(Debug)]
struct AuthorizedCommand {
    token: String,
    command: String,
    arguments: Vec<Value>,
    version: i64,
}

#[derive(Debug)]
struct SessionSupervision {
    definition: &'static LanguageServerDefinition,
    executable: PathBuf,
    epoch: u64,
    restart_breaker: RestartBreaker,
    state: LanguageServerRuntimeState,
    running_since: Option<Instant>,
}

#[derive(Debug, Default)]
struct RestartBreaker {
    attempts: u32,
}

impl RestartBreaker {
    fn next_delay(&mut self) -> Option<(Duration, u32)> {
        let delay = restart_delay(self.attempts)?;
        self.attempts += 1;
        Some((delay, self.attempts))
    }

    fn reset(&mut self) {
        self.attempts = 0;
    }

    fn next_delay_after_run(&mut self, running_since: Option<Instant>) -> Option<(Duration, u32)> {
        if running_since.is_some_and(|started| started.elapsed() >= STABLE_RUNNING_INTERVAL) {
            self.reset();
        }
        self.next_delay()
    }
}

enum SupervisorCommand {
    Exited {
        key: SessionKey,
        epoch: u64,
        reason: String,
        intentional: bool,
    },
    Restart {
        key: SessionKey,
        epoch: u64,
    },
    DiagnosticsReady,
}

struct ActiveDocumentBinding {
    server_id: &'static str,
    session: Arc<LanguageServerSession>,
    uri: String,
}

#[derive(Clone, Debug)]
struct WorkspaceRoot {
    workspace_id: WorkspaceId,
    path: PathBuf,
}

#[derive(Debug)]
struct LanguageServerSession {
    writer: MessageWriter,
    child: Arc<Mutex<Child>>,
    pending: PendingRequests,
    next_request_id: AtomicI64,
    alive: Arc<AtomicBool>,
    intentional_shutdown: Arc<AtomicBool>,
    exit_reporter: SessionExitReporter,
    diagnostics: PublishedDiagnostics,
    text_document_sync: TextDocumentSyncKind,
    text_document_save: Option<bool>,
    static_capabilities: HashSet<String>,
    static_workspace_symbol_resolve: bool,
    static_code_action_resolve: bool,
    execute_commands: HashSet<String>,
    dynamic_capabilities: Arc<Mutex<DynamicCapabilityState>>,
    server_id: &'static str,
    logs: RuntimeLogs,
}

#[derive(Default)]
struct DynamicCapabilityState {
    registrations: HashMap<String, RegisteredCapability>,
    progress_tokens: HashSet<String>,
}

impl std::fmt::Debug for DynamicCapabilityState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DynamicCapabilityState")
            .field("registration_count", &self.registrations.len())
            .field("progress_token_count", &self.progress_tokens.len())
            .finish()
    }
}

struct RegisteredCapability {
    method: String,
    resolve_provider: bool,
    prepare_provider: bool,
    commands: HashSet<String>,
    _watched_files: Option<WatchedFilesRegistration>,
    watch_pattern_count: usize,
}

struct WatchedFilesRegistration {
    watcher: Option<RecommendedWatcher>,
    stopped: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Drop for WatchedFilesRegistration {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        drop(self.watcher.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[derive(Clone)]
struct WatchPattern {
    root: PathBuf,
    matcher: GlobMatcher,
    kind: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WatchedFileChange {
    path: PathBuf,
    kind: u8,
}

struct PreparedRegistration {
    id: String,
    method: String,
    resolve_provider: bool,
    prepare_provider: bool,
    commands: HashSet<String>,
    watch_patterns: Option<Vec<WatchPattern>>,
}

struct ServerRequestContext<'a> {
    writer: &'a MessageWriter,
    project_root: &'a Path,
    workspace_root: &'a Path,
    dynamic_capabilities: &'a Arc<Mutex<DynamicCapabilityState>>,
    workspace_id: WorkspaceId,
    documents: &'a Arc<Mutex<HashMap<DocumentKey, DocumentBinding>>>,
    workspace_edits: &'a Arc<WorkspaceEditTransactions>,
    events: &'a CoreEventDispatcher,
}

#[derive(Debug)]
struct JsonRpcError {
    code: i64,
    message: String,
}

#[derive(Clone, Debug)]
struct SessionExitReporter {
    alive: Arc<AtomicBool>,
    intentional: Arc<AtomicBool>,
    reported: Arc<AtomicBool>,
    supervisor: SyncSender<SupervisorCommand>,
    pending_diagnostics: PendingDiagnostics,
    key: SessionKey,
    epoch: u64,
}

struct SessionExitTarget {
    supervisor: SyncSender<SupervisorCommand>,
    pending_diagnostics: PendingDiagnostics,
    key: SessionKey,
    epoch: u64,
}

impl SessionExitReporter {
    fn report(&self, reason: String) {
        self.alive.store(false, Ordering::Release);
        if self.reported.swap(true, Ordering::AcqRel) {
            return;
        }
        let _ = self.supervisor.send(SupervisorCommand::Exited {
            key: self.key.clone(),
            epoch: self.epoch,
            reason,
            intentional: self.intentional.load(Ordering::Acquire),
        });
    }

    fn publish_diagnostics(
        &self,
        uri: String,
        version: Option<i64>,
        revision: DocumentRevision,
        diagnostics: Vec<LanguageServerDiagnostic>,
    ) {
        self.pending_diagnostics
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(
                PendingDiagnosticKey {
                    session: self.key.clone(),
                    epoch: self.epoch,
                    uri,
                },
                PendingDiagnostic {
                    version,
                    revision,
                    diagnostics,
                },
            );
        let _ = self
            .supervisor
            .try_send(SupervisorCommand::DiagnosticsReady);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TextDocumentSyncKind {
    Full,
    Incremental,
}

enum PrepareRenameResult {
    Rejected,
    Default,
    Range(LanguageServerRange, Option<String>),
}

impl LanguageServerRuntime {
    pub(crate) fn new(workspace_edits: Arc<WorkspaceEditTransactions>) -> Arc<Self> {
        let (supervisor, commands) = mpsc::sync_channel(64);
        let pending_diagnostics = Arc::new(Mutex::new(HashMap::new()));
        let events = CoreEventDispatcher::default();
        let runtime = Arc::new(Self {
            sessions: Mutex::new(HashMap::new()),
            supervision: Mutex::new(HashMap::new()),
            documents: Arc::new(Mutex::new(HashMap::new())),
            open_workspaces: Mutex::new(std::collections::HashSet::new()),
            logs: RuntimeLogs {
                buffers: Arc::new(Mutex::new(HashMap::new())),
                events: events.clone(),
            },
            events,
            supervisor,
            pending_diagnostics,
            workspace_edits,
            next_code_action_id: AtomicU64::new(1),
            validated_code_actions: Mutex::new(HashMap::new()),
        });
        spawn_supervisor(Arc::downgrade(&runtime), commands);
        runtime
    }

    pub(crate) fn set_event_sink(&self, sink: Arc<dyn crate::events::CoreEventSink>) {
        self.events.set_sink(sink);
    }

    pub(crate) fn open_document(
        &self,
        definition: &'static LanguageServerDefinition,
        executable: &Path,
        document: LanguageServerDocumentOpen<'_>,
    ) -> Result<(), LanguageServerError> {
        if !self
            .open_workspaces
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .contains(&document.workspace_id)
        {
            return Err(LanguageServerError::WorkspaceClosed);
        }
        let project_root =
            project_root(definition, document.workspace_root, document.absolute_path);
        let key = SessionKey {
            workspace_id: document.workspace_id,
            server_id: definition.id,
            project_root: project_root.clone(),
            workspace_root: document.workspace_root.to_path_buf(),
        };
        let existing_session = self
            .sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&key)
            .filter(|session| session.is_alive())
            .cloned();
        let session = match existing_session {
            Some(session) => session,
            None => {
                if let Some(state) = self
                    .supervision
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .get(&key)
                    .map(|supervision| supervision.state)
                {
                    return Err(LanguageServerError::RuntimeUnavailable(format!(
                        "language server is {}",
                        runtime_state_name(state)
                    )));
                }
                let epoch = 1;
                let started = self
                    .spawn_session(&key, definition, executable, epoch)
                    .map_err(|error| {
                        self.logs.append(
                            definition.id,
                            LanguageServerLogKind::Runtime,
                            format!("startup failed: {error}"),
                        );
                        error
                    })?;
                let started = Arc::new(started);
                self.sessions
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .insert(key.clone(), Arc::clone(&started));
                self.supervision
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .insert(
                        key.clone(),
                        SessionSupervision {
                            definition,
                            executable: executable.to_path_buf(),
                            epoch,
                            restart_breaker: RestartBreaker::default(),
                            state: LanguageServerRuntimeState::Running,
                            running_since: Some(Instant::now()),
                        },
                    );
                self.emit_status(definition.id);
                started
            }
        };
        let uri = file_uri(document.absolute_path);
        let document_key = DocumentKey {
            workspace_id: document.workspace_id,
            path: document.relative_path.to_owned(),
            server_id: definition.id,
        };
        let previous = self
            .documents
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&document_key);
        if let Some(previous) = previous {
            if previous.generation == document.generation && previous.session.is_alive() {
                self.documents
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .insert(document_key, previous);
                return Ok(());
            }
            let _ = previous.session.close_document(&previous.uri);
        }

        let language_id = definition
            .protocol_language_id(document.language_id, document.relative_path)
            .to_owned();
        session.open_document(&uri, &language_id, document.version, document.text)?;
        let open_workspaces = self
            .open_workspaces
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !open_workspaces.contains(&document.workspace_id) {
            drop(open_workspaces);
            let _ = session.close_document(&uri);
            let workspace_ids = self
                .open_workspaces
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone();
            self.retain_workspaces(&workspace_ids);
            return Err(LanguageServerError::WorkspaceClosed);
        }
        self.documents
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(
                document_key,
                DocumentBinding {
                    generation: document.generation,
                    version: document.version,
                    uri,
                    language_id,
                    text: document.text.to_owned(),
                    session_key: key,
                    session,
                },
            );
        drop(open_workspaces);
        Ok(())
    }

    pub(crate) fn change_document(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        changes: &[LanguageServerChange],
        text: &str,
    ) -> Result<(), LanguageServerError> {
        let bindings = {
            let documents = self
                .documents
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            documents
                .iter()
                .filter(|(key, _)| key.workspace_id == workspace_id && key.path == path)
                .map(|(key, document)| {
                    if document.generation != generation || version != document.version + 1 {
                        return Err(LanguageServerError::StaleDocument);
                    }
                    Ok((
                        key.clone(),
                        Arc::clone(&document.session),
                        document.uri.clone(),
                    ))
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        if bindings.is_empty() {
            return Err(LanguageServerError::DocumentNotOpen);
        }

        {
            let mut documents = self
                .documents
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            for (key, _, _) in &bindings {
                if let Some(document) = documents.get_mut(key)
                    && document.generation == generation
                    && document.version + 1 == version
                {
                    document.version = version;
                    document.text = text.to_owned();
                }
            }
        }
        let mut first_error = None;
        for (_, session, uri) in bindings {
            if session.is_alive()
                && let Err(error) = session.change_document(&uri, version, changes, text)
            {
                first_error.get_or_insert(error);
            }
        }
        if let Some(error) = first_error {
            Err(error)
        } else {
            Ok(())
        }
    }

    pub(crate) fn close_document(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
    ) -> Result<(), LanguageServerError> {
        let documents = {
            let mut documents = self
                .documents
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let keys = documents
                .iter()
                .filter(|(key, document)| {
                    key.workspace_id == workspace_id
                        && key.path == path
                        && document.generation == generation
                })
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            keys.into_iter()
                .filter_map(|key| documents.remove(&key).map(|document| (key, document)))
                .collect::<Vec<_>>()
        };
        let mut first_error = None;
        for (key, document) in &documents {
            self.emit_diagnostics(key, document, Vec::new());
            if document.session.is_alive()
                && let Err(error) = document.session.close_document(&document.uri)
            {
                first_error.get_or_insert(error);
            }
        }
        self.close_unused_sessions();
        first_error.map_or(Ok(()), Err)
    }

    pub(crate) fn save_document(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        text: &str,
    ) -> Result<(), LanguageServerError> {
        let bindings = self.document_bindings(workspace_id, path, generation, version)?;
        let mut first_error = None;
        for binding in bindings {
            if binding.session.is_alive()
                && let Err(error) = binding.session.save_document(&binding.uri, text)
            {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    pub(crate) fn hover(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Option<LanguageServerHover>, LanguageServerError> {
        let bindings = self.document_bindings_for(
            workspace_id,
            path,
            generation,
            version,
            "textDocument/hover",
            "hover",
        )?;
        let results = run_for_bindings(bindings, |binding| {
            binding.session.hover(&binding.uri, position, cancellation)
        });
        let mut contents = Vec::new();
        let mut range = None;
        let mut first_error = None;
        for result in results {
            match result {
                Ok(Some(hover)) => {
                    contents.extend(hover.contents);
                    range = range.or(hover.range);
                }
                Ok(None) => {}
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        if contents.is_empty() {
            first_error.map_or(Ok(None), Err)
        } else {
            Ok(Some(LanguageServerHover { contents, range }))
        }
    }

    pub(crate) fn signature_help(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Option<LanguageServerSignatureHelp>, LanguageServerError> {
        let bindings = self.document_bindings_for(
            workspace_id,
            path,
            generation,
            version,
            "textDocument/signatureHelp",
            "signature help",
        )?;
        let results = run_for_bindings(bindings, |binding| {
            binding
                .session
                .signature_help(&binding.uri, position, cancellation)
        });
        let mut signatures = Vec::new();
        let mut active_signature = None;
        let mut active_parameter = None;
        let mut first_error = None;
        for result in results {
            match result {
                Ok(Some(help)) => {
                    let offset = u32::try_from(signatures.len()).unwrap_or(u32::MAX);
                    active_signature = active_signature.or_else(|| {
                        help.active_signature
                            .and_then(|active| offset.checked_add(active))
                    });
                    active_parameter = active_parameter.or(help.active_parameter);
                    signatures.extend(help.signatures);
                }
                Ok(None) => {}
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        if cancellation.is_cancelled() {
            return Err(LanguageServerError::RequestCancelled);
        }
        if signatures.is_empty() {
            first_error.map_or(Ok(None), Err)
        } else {
            Ok(Some(LanguageServerSignatureHelp {
                signatures,
                active_signature,
                active_parameter,
            }))
        }
    }

    pub(crate) fn definition(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerLocation>, LanguageServerError> {
        self.document_locations(
            workspace_id,
            path,
            generation,
            version,
            position,
            "textDocument/definition",
            "definition",
            None,
            cancellation,
        )
    }

    pub(crate) fn declaration(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerLocation>, LanguageServerError> {
        self.document_locations(
            workspace_id,
            path,
            generation,
            version,
            position,
            "textDocument/declaration",
            "declaration",
            None,
            cancellation,
        )
    }

    pub(crate) fn type_definition(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerLocation>, LanguageServerError> {
        self.document_locations(
            workspace_id,
            path,
            generation,
            version,
            position,
            "textDocument/typeDefinition",
            "type definition",
            None,
            cancellation,
        )
    }

    pub(crate) fn implementation(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerLocation>, LanguageServerError> {
        self.document_locations(
            workspace_id,
            path,
            generation,
            version,
            position,
            "textDocument/implementation",
            "implementation",
            None,
            cancellation,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn references(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        include_declaration: bool,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerLocation>, LanguageServerError> {
        self.document_locations(
            workspace_id,
            path,
            generation,
            version,
            position,
            "textDocument/references",
            "references",
            Some(include_declaration),
            cancellation,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn document_locations(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        method: &str,
        feature: &str,
        include_declaration: Option<bool>,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerLocation>, LanguageServerError> {
        let bindings =
            self.document_bindings_for(workspace_id, path, generation, version, method, feature)?;
        let roots = self.workspace_roots();
        let results = run_for_bindings(bindings, |binding| {
            binding.session.locations(
                method,
                &binding.uri,
                position,
                include_declaration,
                &roots,
                cancellation,
            )
        });
        aggregate_vectors(results, cancellation)
    }

    pub(crate) fn document_symbols(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerDocumentSymbol>, LanguageServerError> {
        let bindings = self.document_bindings_for(
            workspace_id,
            path,
            generation,
            version,
            "textDocument/documentSymbol",
            "document symbols",
        )?;
        let roots = self.workspace_roots();
        let results = run_for_bindings(bindings, |binding| {
            binding
                .session
                .document_symbols(workspace_id, path, &binding.uri, &roots, cancellation)
        });
        aggregate_vectors(results, cancellation)
    }

    pub(crate) fn workspace_symbols(
        &self,
        query: &str,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerWorkspaceSymbol>, LanguageServerError> {
        let roots = self.workspace_roots();
        let sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .filter(|(_, session)| session.is_alive() && session.supports("workspace/symbol"))
            .map(|(key, session)| (key.clone(), Arc::clone(session)))
            .collect::<Vec<_>>();
        if sessions.is_empty() {
            return Err(LanguageServerError::FeatureNotSupported(
                "workspace symbols".to_owned(),
            ));
        }
        let results = thread::scope(|scope| {
            sessions
                .into_iter()
                .map(|(key, session)| {
                    let roots = &roots;
                    scope.spawn(move || {
                        session.workspace_symbols(key.workspace_id, query, roots, cancellation)
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|worker| worker.join().expect("workspace symbol worker panicked"))
                .collect::<Vec<_>>()
        });
        aggregate_vectors(results, cancellation)
    }

    pub(crate) fn resolve_workspace_symbol(
        &self,
        request: LanguageServerWorkspaceSymbolResolveRequest,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<LanguageServerWorkspaceSymbol, LanguageServerError> {
        let roots = self.workspace_roots();
        let session = self
            .sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .find(|(key, session)| {
                key.workspace_id == request.workspace_id
                    && key.server_id == request.server_id
                    && session.is_alive()
                    && session.supports_workspace_symbol_resolve()
            })
            .map(|(_, session)| Arc::clone(session))
            .ok_or_else(|| {
                LanguageServerError::FeatureNotSupported("workspace symbol resolve".to_owned())
            })?;
        session.resolve_workspace_symbol(request, &roots, cancellation)
    }

    pub(crate) fn diagnostics(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
    ) -> Result<Option<Vec<LanguageServerDiagnosticSnapshot>>, LanguageServerError> {
        let bindings = {
            let documents = self
                .documents
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            documents
                .iter()
                .filter(|(key, _)| key.workspace_id == workspace_id && key.path == path)
                .map(|(key, document)| {
                    if document.generation != generation || document.version != version {
                        return Err(LanguageServerError::StaleDocument);
                    }
                    Ok((
                        key.server_id.to_owned(),
                        Arc::clone(&document.session),
                        document.uri.clone(),
                    ))
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        if bindings.is_empty() {
            return Err(LanguageServerError::DocumentNotOpen);
        }
        let mut published = false;
        let mut diagnostics = Vec::new();
        for (server_id, session, uri) in bindings {
            if let Some(server_diagnostics) = session.diagnostics(&uri, version) {
                published = true;
                diagnostics.push(LanguageServerDiagnosticSnapshot {
                    server_id,
                    diagnostics: server_diagnostics,
                });
            }
        }
        Ok(published.then_some(diagnostics))
    }

    pub(crate) fn completion(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        request: &LanguageServerCompletionRequest,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<LanguageServerCompletionList, LanguageServerError> {
        let bindings = self.document_bindings_for(
            workspace_id,
            path,
            generation,
            version,
            "textDocument/completion",
            "completion",
        )?;
        let mut items = Vec::new();
        let mut is_incomplete = false;
        let mut succeeded = false;
        let mut first_error = None;
        let results = run_for_bindings(bindings, |binding| {
            binding
                .session
                .completion(binding.server_id, &binding.uri, request, cancellation)
        });
        for result in results {
            match result {
                Ok(completion) => {
                    succeeded = true;
                    is_incomplete |= completion.is_incomplete;
                    items.extend(completion.items);
                }
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        if succeeded {
            if !request.filter.is_empty() && items.len() > 1_000 {
                items.retain(|item| completion_matches(item, &request.filter));
            }
            Ok(LanguageServerCompletionList {
                items,
                is_incomplete,
            })
        } else {
            Err(first_error.unwrap_or(LanguageServerError::DocumentNotOpen))
        }
    }

    pub(crate) fn resolve_completion(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        request: LanguageServerCompletionResolveRequest,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<LanguageServerCompletionItem, LanguageServerError> {
        let bindings = self.document_bindings_for(
            workspace_id,
            path,
            generation,
            version,
            "textDocument/completion",
            "completion resolve",
        )?;
        let binding = bindings
            .into_iter()
            .find(|binding| binding.server_id == request.server_id)
            .ok_or(LanguageServerError::DocumentNotOpen)?;
        binding
            .session
            .resolve_completion(&request.server_id, request.raw, cancellation)
    }

    pub(crate) fn document_colors(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerColorInformation>, LanguageServerError> {
        let bindings = self.document_bindings_for(
            workspace_id,
            path,
            generation,
            version,
            "textDocument/documentColor",
            "document colors",
        )?;
        let mut colors = Vec::new();
        let mut succeeded = false;
        let mut first_error = None;
        let results = run_for_bindings(bindings, |binding| {
            binding
                .session
                .document_colors(binding.server_id, &binding.uri, cancellation)
        });
        for result in results {
            match result {
                Ok(server_colors) => {
                    succeeded = true;
                    colors.extend(server_colors);
                }
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        if succeeded {
            Ok(colors)
        } else {
            Err(first_error.unwrap_or(LanguageServerError::DocumentNotOpen))
        }
    }

    pub(crate) fn color_presentations(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        request: &LanguageServerColorPresentationRequest,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerColorPresentation>, LanguageServerError> {
        let bindings = self.document_bindings_for(
            workspace_id,
            path,
            generation,
            version,
            "textDocument/documentColor",
            "color presentations",
        )?;
        let binding = bindings
            .into_iter()
            .find(|binding| binding.server_id == request.server_id)
            .ok_or(LanguageServerError::DocumentNotOpen)?;
        binding.session.color_presentations(
            &binding.uri,
            request.range,
            request.color,
            cancellation,
        )
    }

    pub(crate) fn formatting(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        options: LanguageServerFormattingOptions,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerTextEdit>, LanguageServerError> {
        let (key, session, uri, text) = {
            let documents = self
                .documents
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let mut document_exists = false;
            let mut selected = None;
            for definition in language_server_catalog() {
                let key = DocumentKey {
                    workspace_id,
                    path: path.to_owned(),
                    server_id: definition.id,
                };
                let Some(document) = documents.get(&key) else {
                    continue;
                };
                document_exists = true;
                if !document.session.supports("textDocument/formatting") {
                    continue;
                }
                if document.generation != generation || document.version != version {
                    return Err(LanguageServerError::StaleDocument);
                }
                selected = Some((
                    key,
                    Arc::clone(&document.session),
                    document.uri.clone(),
                    document.text.clone(),
                ));
                break;
            }
            selected.ok_or_else(|| {
                if document_exists {
                    LanguageServerError::FeatureNotSupported("document formatting".to_owned())
                } else {
                    LanguageServerError::DocumentNotOpen
                }
            })?
        };
        let edits = session.formatting(&uri, options, cancellation)?;
        validate_text_edits(&text, &edits)?;

        let documents = self
            .documents
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let current = documents
            .get(&key)
            .filter(|document| {
                document.generation == generation
                    && document.version == version
                    && Arc::ptr_eq(&document.session, &session)
            })
            .ok_or(LanguageServerError::ContentModified)?;
        if current.text != text {
            return Err(LanguageServerError::ContentModified);
        }
        Ok(edits)
    }

    pub(crate) fn open_documents(&self) -> Vec<WorkspaceEditOpenDocument> {
        let mut open = HashMap::new();
        for (key, document) in self
            .documents
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
        {
            open.entry((key.workspace_id, key.path.clone()))
                .or_insert_with(|| WorkspaceEditOpenDocument {
                    workspace_id: key.workspace_id,
                    path: key.path.clone(),
                    generation: document.generation,
                    version: document.version,
                    text: document.text.clone(),
                });
        }
        open.into_values().collect()
    }

    pub(crate) fn prepare_rename(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Option<LanguageServerPrepareRename>, LanguageServerError> {
        let bindings = self.document_bindings_for(
            workspace_id,
            path,
            generation,
            version,
            "textDocument/rename",
            "rename",
        )?;
        let mut first_error = None;
        for binding in bindings {
            if !binding.session.supports("textDocument/prepareRename") {
                return Ok(Some(LanguageServerPrepareRename {
                    server_id: binding.server_id.to_owned(),
                    range: None,
                    placeholder: None,
                }));
            }
            match binding
                .session
                .prepare_rename(&binding.uri, position, cancellation)
            {
                Ok(PrepareRenameResult::Range(range, placeholder)) => {
                    return Ok(Some(LanguageServerPrepareRename {
                        server_id: binding.server_id.to_owned(),
                        range: Some(range),
                        placeholder,
                    }));
                }
                Ok(PrepareRenameResult::Default) => {
                    return Ok(Some(LanguageServerPrepareRename {
                        server_id: binding.server_id.to_owned(),
                        range: None,
                        placeholder: None,
                    }));
                }
                Ok(PrepareRenameResult::Rejected) => {}
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        first_error.map_or(Ok(None), Err)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn rename(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        new_name: &str,
        server_id: Option<&str>,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Value, LanguageServerError> {
        if new_name.is_empty() || new_name.len() > MAX_CAPABILITY_STRING_BYTES {
            return Err(LanguageServerError::InvalidDocument(
                "rename target must contain between 1 and 256 bytes".to_owned(),
            ));
        }
        let bindings = self.document_bindings_for(
            workspace_id,
            path,
            generation,
            version,
            "textDocument/rename",
            "rename",
        )?;
        let binding = bindings
            .into_iter()
            .find(|binding| server_id.is_none_or(|server_id| binding.server_id == server_id))
            .ok_or_else(|| LanguageServerError::FeatureNotSupported("rename server".to_owned()))?;
        binding
            .session
            .rename(&binding.uri, position, new_name, cancellation)
    }

    pub(crate) fn code_actions(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        request: &LanguageServerCodeActionRequest,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerCodeAction>, LanguageServerError> {
        let bindings = self.document_bindings_for(
            workspace_id,
            path,
            generation,
            version,
            "textDocument/codeAction",
            "code actions",
        )?;
        let results = run_for_bindings(bindings, |binding| {
            binding
                .session
                .code_actions(binding.server_id, &binding.uri, request, cancellation)
        });
        let mut actions = aggregate_vectors(results, cancellation)?;
        if actions.len() > MAX_CODE_ACTIONS_PER_REQUEST {
            return Err(LanguageServerError::Protocol(
                "language servers returned too many code actions".to_owned(),
            ));
        }
        for action in &mut actions {
            self.register_code_action(action, workspace_id, path, generation, version)?;
        }
        Ok(actions)
    }

    pub(crate) fn resolve_code_action(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        request: LanguageServerCodeActionResolveRequest,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<LanguageServerCodeAction, LanguageServerError> {
        self.validate_code_action_identity(
            request.action_id,
            workspace_id,
            path,
            generation,
            Some(version),
            &request.server_id,
            &request.raw,
            None,
        )?;
        let bindings = self.document_bindings_for(
            workspace_id,
            path,
            generation,
            version,
            "textDocument/codeAction",
            "code action resolve",
        )?;
        let binding = bindings
            .into_iter()
            .find(|binding| binding.server_id == request.server_id)
            .filter(|binding| binding.session.supports_code_action_resolve())
            .ok_or_else(|| {
                LanguageServerError::FeatureNotSupported("code action resolve".to_owned())
            })?;
        self.validate_code_action_identity(
            request.action_id,
            workspace_id,
            path,
            generation,
            Some(version),
            &request.server_id,
            &request.raw,
            Some(&binding.session),
        )?;
        let action_id = request.action_id;
        {
            let mut actions = self
                .validated_code_actions
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let validated = actions
                .get_mut(&action_id)
                .ok_or(LanguageServerError::ContentModified)?;
            mark_code_action_resolving(&mut validated.resolved)?;
        }
        let resolved = binding.session.resolve_code_action(request, cancellation)?;
        let mut resolved = resolved;
        resolved.resolve_supported = false;
        let command = authorize_code_action_command(&resolved.raw, version)?;
        resolved.command_authorization = command.as_ref().map(|command| command.token.clone());
        let mut actions = self
            .validated_code_actions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let validated = actions
            .get_mut(&action_id)
            .ok_or(LanguageServerError::ContentModified)?;
        validated.raw = resolved.raw.clone();
        validated.command = command;
        Ok(resolved)
    }

    pub(crate) fn execute_command(
        &self,
        request: LanguageServerExecuteCommandRequest,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Value, LanguageServerError> {
        let binding = self
            .document_bindings(
                request.workspace_id,
                &request.path,
                request.generation,
                request.version,
            )?
            .into_iter()
            .find(|binding| binding.server_id == request.server_id)
            .ok_or(LanguageServerError::DocumentNotOpen)?;
        let session = binding.session;
        let command = {
            let mut actions = self
                .validated_code_actions
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let action = actions
                .values_mut()
                .find(|action| {
                    action.created_at.elapsed() <= VALIDATED_CODE_ACTION_TTL
                        && action.workspace_id == request.workspace_id
                        && action.path == request.path
                        && action.generation == request.generation
                        && action.server_id == request.server_id
                        && Arc::ptr_eq(&action.session, &session)
                        && action.command.as_ref().is_some_and(|command| {
                            command.token == request.authorization
                                && command.version == request.version
                        })
                })
                .ok_or_else(|| {
                    LanguageServerError::InvalidDocument(
                        "execute-command authorization is invalid, stale, or already used"
                            .to_owned(),
                    )
                })?;
            take_authorized_command(&mut action.command, &request.authorization)?
        };
        if !session.advertises_command(&command.command) {
            return Err(LanguageServerError::FeatureNotSupported(format!(
                "command `{}` was not advertised by the language server",
                command.command
            )));
        }
        session.execute_command(&command.command, command.arguments, cancellation)
    }

    pub(crate) fn validate_code_action(
        &self,
        action: &LanguageServerCodeAction,
    ) -> Result<WorkspaceId, LanguageServerError> {
        let (workspace_id, path, generation, version, session) = {
            let validated = self
                .validated_code_actions
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let current = validated.get(&action.action_id).ok_or_else(|| {
                LanguageServerError::InvalidDocument(
                    "code action expired or was not issued by this session".to_owned(),
                )
            })?;
            if current.created_at.elapsed() > VALIDATED_CODE_ACTION_TTL
                || current.server_id != action.server_id
                || current.raw != action.raw
            {
                return Err(LanguageServerError::ContentModified);
            }
            (
                current.workspace_id,
                current.path.clone(),
                current.generation,
                current.version,
                Arc::clone(&current.session),
            )
        };
        let current = self
            .document_bindings(workspace_id, &path, generation, version)?
            .into_iter()
            .find(|binding| binding.server_id == action.server_id)
            .ok_or(LanguageServerError::DocumentNotOpen)?;
        if !Arc::ptr_eq(&current.session, &session) {
            return Err(LanguageServerError::ContentModified);
        }
        Ok(workspace_id)
    }

    fn register_code_action(
        &self,
        action: &mut LanguageServerCodeAction,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
    ) -> Result<(), LanguageServerError> {
        let session = self
            .document_bindings(workspace_id, path, generation, version)?
            .into_iter()
            .find(|binding| binding.server_id == action.server_id)
            .map(|binding| binding.session)
            .ok_or(LanguageServerError::DocumentNotOpen)?;
        let mut validated = self
            .validated_code_actions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        validated.retain(|_, action| action.created_at.elapsed() <= VALIDATED_CODE_ACTION_TTL);
        if validated.len() >= MAX_VALIDATED_CODE_ACTIONS {
            return Err(LanguageServerError::WorkerBusy);
        }
        let id = self
            .next_code_action_id
            .fetch_add(1, Ordering::Relaxed)
            .max(1);
        action.action_id = id;
        let command = authorize_code_action_command(&action.raw, version)?;
        action.command_authorization = command.as_ref().map(|command| command.token.clone());
        validated.insert(
            id,
            ValidatedCodeAction {
                created_at: Instant::now(),
                workspace_id,
                path: path.to_owned(),
                generation,
                version,
                server_id: action.server_id.clone(),
                raw: action.raw.clone(),
                session,
                resolved: false,
                command,
            },
        );
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_code_action_identity(
        &self,
        action_id: u64,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: Option<i64>,
        server_id: &str,
        raw: &Value,
        session: Option<&Arc<LanguageServerSession>>,
    ) -> Result<(), LanguageServerError> {
        let validated = self
            .validated_code_actions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let action = validated.get(&action_id).ok_or_else(|| {
            LanguageServerError::InvalidDocument(
                "code action expired or was not issued by this session".to_owned(),
            )
        })?;
        if action.created_at.elapsed() > VALIDATED_CODE_ACTION_TTL
            || action.workspace_id != workspace_id
            || action.path != path
            || action.generation != generation
            || version.is_some_and(|version| action.version != version)
            || action.server_id != server_id
            || action.raw != *raw
            || session.is_some_and(|session| !Arc::ptr_eq(session, &action.session))
        {
            return Err(LanguageServerError::ContentModified);
        }
        Ok(())
    }

    pub(crate) fn bind_code_action_command_to_staged_edit(
        &self,
        action_id: u64,
        staged: Option<&super::StagedWorkspaceEdit>,
    ) -> Result<(), LanguageServerError> {
        let mut actions = self
            .validated_code_actions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let action = actions
            .get_mut(&action_id)
            .ok_or(LanguageServerError::ContentModified)?;
        let Some(command) = action.command.as_mut() else {
            return Ok(());
        };
        command.version = staged
            .and_then(|staged| {
                staged.documents.iter().find(|document| {
                    document.workspace_id == action.workspace_id
                        && document.path == action.path
                        && document.generation == Some(action.generation)
                        && document.version == Some(action.version)
                })
            })
            .map_or(action.version, |_| action.version.saturating_add(1));
        Ok(())
    }

    fn document_bindings(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
    ) -> Result<Vec<ActiveDocumentBinding>, LanguageServerError> {
        let documents = self
            .documents
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let bindings = documents
            .iter()
            .filter(|(key, _)| key.workspace_id == workspace_id && key.path == path)
            .map(|(key, document)| {
                if document.generation != generation || document.version != version {
                    return Err(LanguageServerError::StaleDocument);
                }
                Ok(ActiveDocumentBinding {
                    server_id: key.server_id,
                    session: Arc::clone(&document.session),
                    uri: document.uri.clone(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if bindings.is_empty() {
            Err(LanguageServerError::DocumentNotOpen)
        } else {
            Ok(bindings)
        }
    }

    fn document_bindings_for(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        method: &str,
        feature: &str,
    ) -> Result<Vec<ActiveDocumentBinding>, LanguageServerError> {
        let bindings = self.document_bindings(workspace_id, path, generation, version)?;
        let supported = bindings
            .into_iter()
            .filter(|binding| binding.session.supports(method))
            .collect::<Vec<_>>();
        if supported.is_empty() {
            Err(LanguageServerError::FeatureNotSupported(feature.to_owned()))
        } else {
            Ok(supported)
        }
    }

    fn workspace_roots(&self) -> Vec<WorkspaceRoot> {
        let open = self
            .open_workspaces
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let mut roots = self
            .sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .keys()
            .filter(|key| open.contains(&key.workspace_id))
            .filter_map(|key| {
                fs::canonicalize(&key.workspace_root)
                    .ok()
                    .map(|path| WorkspaceRoot {
                        workspace_id: key.workspace_id,
                        path,
                    })
            })
            .collect::<Vec<_>>();
        roots.sort_by(|left, right| {
            right
                .path
                .components()
                .count()
                .cmp(&left.path.components().count())
        });
        roots.dedup_by(|left, right| {
            left.workspace_id == right.workspace_id && left.path == right.path
        });
        roots
    }

    pub(crate) fn close_server(&self, server_id: &str) {
        let documents = {
            let mut document_map = self
                .documents
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let keys = document_map
                .iter()
                .filter(|(key, _)| key.server_id == server_id)
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            keys.into_iter()
                .filter_map(|key| document_map.remove(&key).map(|document| (key, document)))
                .collect::<Vec<_>>()
        };
        for (key, document) in &documents {
            self.emit_diagnostics(key, document, Vec::new());
        }
        self.supervision
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|key, _| key.server_id != server_id);
        let sessions = remove_sessions(&self.sessions, |key| key.server_id == server_id);
        mark_intentional(&sessions);
        dispose_runtime_resources(
            documents
                .into_iter()
                .map(|(_, document)| document)
                .collect(),
            sessions,
        );
        self.emit_status(server_id);
    }

    pub(crate) fn restart_server(&self, server_id: &str) {
        let restarts = {
            let mut supervision = self
                .supervision
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            supervision
                .iter_mut()
                .filter(|(key, _)| key.server_id == server_id)
                .map(|(key, session)| {
                    session.restart_breaker.reset();
                    session.epoch = session.epoch.wrapping_add(1);
                    session.state = LanguageServerRuntimeState::Restarting;
                    session.running_since = None;
                    (key.clone(), session.epoch)
                })
                .collect::<Vec<_>>()
        };
        let replaced = remove_sessions(&self.sessions, |key| key.server_id == server_id);
        for session in replaced {
            session.terminate();
        }
        for (key, epoch) in restarts {
            if self
                .supervisor
                .try_send(SupervisorCommand::Restart {
                    key: key.clone(),
                    epoch,
                })
                .is_err()
            {
                if let Some(session) = self
                    .supervision
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .get_mut(&key)
                    .filter(|session| session.epoch == epoch)
                {
                    session.state = LanguageServerRuntimeState::Crashed;
                }
                self.logs.append(
                    server_id,
                    LanguageServerLogKind::Runtime,
                    "manual restart could not be queued; the old process was terminated".to_owned(),
                );
            }
        }
        self.logs.append(
            server_id,
            LanguageServerLogKind::Runtime,
            "manual restart requested; automatic restart breaker reset".to_owned(),
        );
        self.emit_status(server_id);
    }

    pub(crate) fn server_status(&self, server_id: &str) -> LanguageServerRuntimeStatus {
        let supervision = self
            .supervision
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let matching = supervision
            .iter()
            .filter(|(key, _)| key.server_id == server_id)
            .collect::<Vec<_>>();
        let session_count = matching.len();
        let workspace_count = matching
            .iter()
            .map(|(key, _)| key.workspace_id)
            .collect::<std::collections::HashSet<_>>()
            .len();
        let running = matching
            .iter()
            .filter(|(_, session)| session.state == LanguageServerRuntimeState::Running)
            .count();
        let restarting = matching
            .iter()
            .any(|(_, session)| session.state == LanguageServerRuntimeState::Restarting);
        let crashed = matching
            .iter()
            .any(|(_, session)| session.state == LanguageServerRuntimeState::Crashed);
        let state = if session_count == 0 {
            LanguageServerRuntimeState::Inactive
        } else if running == session_count {
            LanguageServerRuntimeState::Running
        } else if running > 0 {
            LanguageServerRuntimeState::Degraded
        } else if restarting {
            LanguageServerRuntimeState::Restarting
        } else if crashed {
            LanguageServerRuntimeState::Crashed
        } else {
            LanguageServerRuntimeState::Degraded
        };
        LanguageServerRuntimeStatus {
            state,
            session_count,
            workspace_count,
        }
    }

    pub(crate) fn logs(&self, server_id: &str) -> Vec<LanguageServerLog> {
        self.logs.entries(server_id)
    }

    pub(crate) fn retain_workspaces(&self, workspace_ids: &std::collections::HashSet<WorkspaceId>) {
        let mut open_workspaces = self
            .open_workspaces
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        open_workspaces.clone_from(workspace_ids);
        let documents = {
            let mut document_map = self
                .documents
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let keys = document_map
                .keys()
                .filter(|key| !workspace_ids.contains(&key.workspace_id))
                .cloned()
                .collect::<Vec<_>>();
            keys.into_iter()
                .filter_map(|key| document_map.remove(&key))
                .collect::<Vec<_>>()
        };
        let removed_keys = self
            .supervision
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .keys()
            .filter(|key| !workspace_ids.contains(&key.workspace_id))
            .cloned()
            .collect::<Vec<_>>();
        self.supervision
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|key, _| workspace_ids.contains(&key.workspace_id));
        let sessions = remove_sessions(&self.sessions, |key| {
            !workspace_ids.contains(&key.workspace_id)
        });
        mark_intentional(&sessions);
        drop(open_workspaces);
        dispose_runtime_resources(documents, sessions);
        for server_id in removed_keys
            .iter()
            .map(|key| key.server_id)
            .collect::<std::collections::HashSet<_>>()
        {
            self.emit_status(server_id);
        }
    }

    fn spawn_session(
        &self,
        key: &SessionKey,
        definition: &'static LanguageServerDefinition,
        executable: &Path,
        epoch: u64,
    ) -> Result<LanguageServerSession, LanguageServerError> {
        LanguageServerSession::spawn(
            executable,
            definition.launch_args,
            &key.project_root,
            &key.workspace_root,
            definition.id,
            self.logs.clone(),
            key.workspace_id,
            Arc::clone(&self.documents),
            Arc::clone(&self.workspace_edits),
            self.events.clone(),
            SessionExitTarget {
                supervisor: self.supervisor.clone(),
                pending_diagnostics: Arc::clone(&self.pending_diagnostics),
                key: key.clone(),
                epoch,
            },
        )
    }

    pub(crate) fn emit_status(&self, server_id: &str) {
        self.events.emit(CoreEvent::LanguageServerStatusChanged {
            server_id: server_id.to_owned(),
        });
    }

    fn emit_diagnostics(
        &self,
        key: &DocumentKey,
        document: &DocumentBinding,
        diagnostics: Vec<LanguageServerDiagnostic>,
    ) {
        self.events
            .emit(CoreEvent::LanguageServerDiagnosticsChanged(
                LanguageServerDiagnosticsChanged {
                    workspace_id: key.workspace_id,
                    path: key.path.clone(),
                    server_id: key.server_id.to_owned(),
                    generation: document.generation,
                    version: document.version,
                    diagnostics,
                },
            ));
    }

    fn close_unused_sessions(&self) {
        let active = self
            .documents
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .values()
            .map(|document| document.session_key.clone())
            .collect::<std::collections::HashSet<_>>();
        let removed = {
            let mut supervision = self
                .supervision
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let removed = supervision
                .keys()
                .filter(|key| !active.contains(*key))
                .cloned()
                .collect::<Vec<_>>();
            supervision.retain(|key, _| active.contains(key));
            removed
        };
        let removed_set = removed
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        let sessions = remove_sessions(&self.sessions, |key| removed_set.contains(key));
        mark_intentional(&sessions);
        dispose_runtime_resources(Vec::new(), sessions);
        for server_id in removed
            .iter()
            .map(|key| key.server_id)
            .collect::<std::collections::HashSet<_>>()
        {
            self.emit_status(server_id);
        }
    }

    fn handle_supervisor_command(&self, command: SupervisorCommand) {
        match command {
            SupervisorCommand::Exited {
                key,
                epoch,
                reason,
                intentional,
            } => self.handle_exit(key, epoch, reason, intentional),
            SupervisorCommand::Restart { key, epoch } => self.restart_session(key, epoch),
            SupervisorCommand::DiagnosticsReady => self.publish_pending_diagnostics(),
        }
    }

    fn handle_exit(&self, key: SessionKey, epoch: u64, reason: String, intentional: bool) {
        let has_documents = self
            .documents
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .values()
            .any(|document| document.session_key == key);
        let next = {
            let mut supervision = self
                .supervision
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let Some(session) = supervision.get_mut(&key) else {
                return;
            };
            if session.epoch != epoch || intentional {
                return;
            }
            let running_since = session.running_since.take();
            if !has_documents {
                supervision.remove(&key);
                None
            } else if let Some((delay, attempt)) =
                session.restart_breaker.next_delay_after_run(running_since)
            {
                session.state = LanguageServerRuntimeState::Restarting;
                session.epoch = session.epoch.wrapping_add(1);
                Some((delay, session.epoch, attempt))
            } else {
                session.state = LanguageServerRuntimeState::Crashed;
                None
            }
        };
        let exited = self
            .sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&key);
        if let Some(session) = exited {
            session.terminate();
        }
        let documents = self
            .documents
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for (document_key, document) in documents
            .iter()
            .filter(|(_, document)| document.session_key == key)
        {
            self.emit_diagnostics(document_key, document, Vec::new());
        }
        drop(documents);
        match next {
            Some((delay, next_epoch, attempt)) => {
                self.logs.append(
                    key.server_id,
                    LanguageServerLogKind::Runtime,
                    format!(
                        "unexpected exit ({reason}); restart attempt {attempt}/{MAX_RESTART_ATTEMPTS} in {} ms",
                        delay.as_millis()
                    ),
                );
                let supervisor = self.supervisor.clone();
                let restart_key = key.clone();
                let _ = thread::Builder::new()
                    .name("kosmos-language-server-backoff".to_owned())
                    .spawn(move || {
                        thread::sleep(delay);
                        let _ = supervisor.send(SupervisorCommand::Restart {
                            key: restart_key,
                            epoch: next_epoch,
                        });
                    });
            }
            None if has_documents => self.logs.append(
                key.server_id,
                LanguageServerLogKind::Runtime,
                format!(
                    "unexpected exit ({reason}); automatic restart limit of {MAX_RESTART_ATTEMPTS} reached"
                ),
            ),
            None => {}
        }
        self.emit_status(key.server_id);
    }

    fn restart_session(&self, key: SessionKey, epoch: u64) {
        let metadata = self
            .supervision
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&key)
            .filter(|session| {
                session.epoch == epoch && session.state == LanguageServerRuntimeState::Restarting
            })
            .map(|session| (session.definition, session.executable.clone()));
        let Some((definition, executable)) = metadata else {
            return;
        };
        let session = match self.spawn_session(&key, definition, &executable, epoch) {
            Ok(session) => Arc::new(session),
            Err(error) => {
                self.handle_exit(key, epoch, format!("restart failed: {error}"), false);
                return;
            }
        };
        let mut documents = self
            .documents
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let replay_result = {
            let mut result = Ok(());
            for document in documents
                .values()
                .filter(|document| document.session_key == key)
            {
                let replay = document_replay(
                    document.generation,
                    document.version,
                    &document.uri,
                    &document.language_id,
                    &document.text,
                );
                if let Err(error) = session.open_document(
                    replay.uri,
                    replay.language_id,
                    replay.version,
                    replay.text,
                ) {
                    result = Err(error);
                    break;
                }
            }
            result
        };
        if let Err(error) = replay_result {
            drop(documents);
            session.terminate();
            self.handle_exit(
                key,
                epoch,
                format!("document replay failed: {error}"),
                false,
            );
            return;
        }
        let accepted = {
            let mut supervision = self
                .supervision
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if let Some(supervision) = supervision.get_mut(&key)
                && supervision.epoch == epoch
                && supervision.state == LanguageServerRuntimeState::Restarting
            {
                self.sessions
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .insert(key.clone(), Arc::clone(&session));
                let mut replaced = Vec::new();
                for document in documents
                    .values_mut()
                    .filter(|document| document.session_key == key)
                {
                    retain_replaced_arc(&mut document.session, Arc::clone(&session), &mut replaced);
                }
                supervision.state = LanguageServerRuntimeState::Running;
                supervision.running_since = Some(Instant::now());
                drop(documents);
                drop(replaced);
                true
            } else {
                false
            }
        };
        if !accepted {
            session.terminate();
            return;
        }
        self.logs.append(
            key.server_id,
            LanguageServerLogKind::Runtime,
            "language server restarted and open documents replayed".to_owned(),
        );
        self.emit_status(key.server_id);
    }

    fn publish_diagnostics(
        &self,
        key: &SessionKey,
        epoch: u64,
        uri: &str,
        version: Option<i64>,
        revision: DocumentRevision,
        diagnostics: Vec<LanguageServerDiagnostic>,
    ) {
        let current = self
            .supervision
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(key)
            .is_some_and(|session| session.epoch == epoch);
        if !current {
            return;
        }
        let documents = self
            .documents
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for (document_key, document) in documents
            .iter()
            .filter(|(_, document)| document.session_key == *key && document.uri == uri)
        {
            if !diagnostic_matches_revision(
                document.generation,
                document.version,
                version,
                revision,
            ) {
                continue;
            }
            self.events
                .emit(CoreEvent::LanguageServerDiagnosticsChanged(
                    LanguageServerDiagnosticsChanged {
                        workspace_id: document_key.workspace_id,
                        path: document_key.path.clone(),
                        server_id: document_key.server_id.to_owned(),
                        generation: document.generation,
                        version: revision.version,
                        diagnostics: diagnostics.clone(),
                    },
                ));
        }
    }

    fn publish_pending_diagnostics(&self) {
        let pending = std::mem::take(
            &mut *self
                .pending_diagnostics
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        );
        for (key, diagnostic) in pending {
            self.publish_diagnostics(
                &key.session,
                key.epoch,
                &key.uri,
                diagnostic.version,
                diagnostic.revision,
                diagnostic.diagnostics,
            );
        }
    }
}

fn spawn_supervisor(
    runtime: Weak<LanguageServerRuntime>,
    commands: mpsc::Receiver<SupervisorCommand>,
) {
    thread::Builder::new()
        .name("kosmos-language-server-supervisor".to_owned())
        .spawn(move || {
            while let Ok(command) = commands.recv() {
                let Some(runtime) = runtime.upgrade() else {
                    break;
                };
                runtime.handle_supervisor_command(command);
                runtime.publish_pending_diagnostics();
            }
        })
        .expect("language server supervisor should start");
}

fn restart_delay(completed_attempts: u32) -> Option<Duration> {
    if completed_attempts >= MAX_RESTART_ATTEMPTS {
        return None;
    }
    Some(std::cmp::min(
        INITIAL_RESTART_DELAY.saturating_mul(1_u32 << completed_attempts.min(30)),
        MAX_RESTART_DELAY,
    ))
}

fn runtime_state_name(state: LanguageServerRuntimeState) -> &'static str {
    match state {
        LanguageServerRuntimeState::Inactive => "inactive",
        LanguageServerRuntimeState::Restarting => "restarting",
        LanguageServerRuntimeState::Running => "running",
        LanguageServerRuntimeState::Degraded => "degraded",
        LanguageServerRuntimeState::Crashed => "crashed",
    }
}

#[derive(Debug, Eq, PartialEq)]
struct DocumentReplay<'a> {
    generation: u64,
    version: i64,
    uri: &'a str,
    language_id: &'a str,
    text: &'a str,
}

fn document_replay<'a>(
    generation: u64,
    version: i64,
    uri: &'a str,
    language_id: &'a str,
    text: &'a str,
) -> DocumentReplay<'a> {
    DocumentReplay {
        generation,
        version,
        uri,
        language_id,
        text,
    }
}

fn retain_replaced_arc<T>(target: &mut Arc<T>, replacement: Arc<T>, replaced: &mut Vec<Arc<T>>) {
    replaced.push(std::mem::replace(target, replacement));
}

fn diagnostic_matches_revision(
    generation: u64,
    document_version: i64,
    published_version: Option<i64>,
    revision: DocumentRevision,
) -> bool {
    generation == revision.generation
        && document_version == revision.version
        && published_version.is_none_or(|version| version == revision.version)
}

fn completion_matches(item: &LanguageServerCompletionItem, filter: &str) -> bool {
    let candidate = item.filter_text.as_deref().unwrap_or(&item.label);
    let mut candidate = candidate.chars().flat_map(char::to_lowercase);
    filter
        .chars()
        .flat_map(char::to_lowercase)
        .all(|expected| candidate.any(|actual| actual == expected))
}

fn run_for_bindings<T: Send>(
    bindings: Vec<ActiveDocumentBinding>,
    operation: impl Fn(ActiveDocumentBinding) -> T + Sync,
) -> Vec<T> {
    thread::scope(|scope| {
        bindings
            .into_iter()
            .map(|binding| scope.spawn(|| operation(binding)))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|worker| {
                worker
                    .join()
                    .expect("language server feature worker panicked")
            })
            .collect()
    })
}

fn aggregate_vectors<T>(
    results: Vec<Result<Vec<T>, LanguageServerError>>,
    cancellation: &LanguageServerRequestCancellation,
) -> Result<Vec<T>, LanguageServerError> {
    let mut values = Vec::new();
    let mut succeeded = false;
    let mut first_error = None;
    for result in results {
        match result {
            Ok(result) => {
                succeeded = true;
                values.extend(result);
            }
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    if cancellation.is_cancelled() {
        Err(LanguageServerError::RequestCancelled)
    } else if succeeded {
        Ok(values)
    } else {
        Err(first_error.unwrap_or(LanguageServerError::DocumentNotOpen))
    }
}

impl LanguageServerSession {
    #[allow(clippy::too_many_arguments)]
    fn spawn(
        executable: &Path,
        launch_args: &[&str],
        project_root: &Path,
        workspace_root: &Path,
        server_id: &'static str,
        logs: RuntimeLogs,
        workspace_id: WorkspaceId,
        documents: Arc<Mutex<HashMap<DocumentKey, DocumentBinding>>>,
        workspace_edits: Arc<WorkspaceEditTransactions>,
        events: CoreEventDispatcher,
        exit_target: SessionExitTarget,
    ) -> Result<Self, LanguageServerError> {
        let mut child = Command::new(executable)
            .args(launch_args)
            .current_dir(project_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| LanguageServerError::ServerStart(error.to_string()))?;
        let stdin = child.stdin.take().ok_or_else(|| {
            LanguageServerError::ServerStart("language server stdin was unavailable".to_owned())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            LanguageServerError::ServerStart("language server stdout was unavailable".to_owned())
        })?;
        if let Some(stderr) = child.stderr.take() {
            spawn_stderr(stderr, server_id, logs.clone())?;
        }

        let writer = spawn_writer(stdin)?;
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let alive = Arc::new(AtomicBool::new(true));
        let intentional_shutdown = Arc::new(AtomicBool::new(false));
        let exit_reporter = SessionExitReporter {
            alive: Arc::clone(&alive),
            intentional: Arc::clone(&intentional_shutdown),
            reported: Arc::new(AtomicBool::new(false)),
            supervisor: exit_target.supervisor,
            pending_diagnostics: exit_target.pending_diagnostics,
            key: exit_target.key,
            epoch: exit_target.epoch,
        };
        let diagnostics = Arc::new(Mutex::new(HashMap::new()));
        let dynamic_capabilities = Arc::new(Mutex::new(DynamicCapabilityState::default()));
        spawn_reader(
            stdout,
            writer.clone(),
            Arc::clone(&pending),
            Arc::clone(&diagnostics),
            project_root.to_path_buf(),
            workspace_root.to_path_buf(),
            Arc::clone(&dynamic_capabilities),
            workspace_id,
            documents,
            workspace_edits,
            events,
            server_id,
            logs.clone(),
            exit_reporter.clone(),
        )?;
        let child = Arc::new(Mutex::new(child));
        spawn_process_watcher(Arc::clone(&child), exit_reporter.clone())?;

        let mut session = Self {
            writer,
            child,
            pending,
            next_request_id: AtomicI64::new(1),
            alive,
            intentional_shutdown,
            exit_reporter,
            diagnostics,
            text_document_sync: TextDocumentSyncKind::Full,
            text_document_save: None,
            static_capabilities: HashSet::new(),
            static_workspace_symbol_resolve: false,
            static_code_action_resolve: false,
            execute_commands: HashSet::new(),
            dynamic_capabilities,
            server_id,
            logs,
        };
        let initialize_result = session.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "clientInfo": { "name": "Kosmos" },
                "rootUri": file_uri(project_root),
                "workspaceFolders": [{
                    "uri": file_uri(project_root),
                    "name": project_root.file_name().and_then(|name| name.to_str()).unwrap_or("workspace")
                }],
                "capabilities": {
                    "general": { "positionEncodings": ["utf-16"] },
                    "textDocument": {
                        "hover": { "dynamicRegistration": true, "contentFormat": ["markdown", "plaintext"] },
                        "publishDiagnostics": { "versionSupport": true },
                        "formatting": { "dynamicRegistration": true },
                        "signatureHelp": {
                            "dynamicRegistration": true,
                            "signatureInformation": {
                                "documentationFormat": ["markdown", "plaintext"],
                                "parameterInformation": { "labelOffsetSupport": true },
                                "activeParameterSupport": true
                            }
                        },
                        "definition": { "dynamicRegistration": true, "linkSupport": true },
                        "declaration": { "dynamicRegistration": true, "linkSupport": true },
                        "typeDefinition": { "dynamicRegistration": true, "linkSupport": true },
                        "implementation": { "dynamicRegistration": true, "linkSupport": true },
                        "references": { "dynamicRegistration": true },
                        "documentSymbol": {
                            "dynamicRegistration": true,
                            "hierarchicalDocumentSymbolSupport": true,
                            "tagSupport": { "valueSet": [1] }
                        },
                        "completion": {
                            "completionItem": {
                                "snippetSupport": true,
                                "documentationFormat": ["markdown", "plaintext"],
                                "resolveSupport": {
                                    "properties": ["detail", "documentation", "additionalTextEdits"]
                                }
                            },
                            "completionList": {
                                "itemDefaults": [
                                    "commitCharacters",
                                    "editRange",
                                    "insertTextFormat",
                                    "insertTextMode",
                                    "data"
                                ]
                            }
                        },
                        "rename": { "dynamicRegistration": true, "prepareSupport": true },
                        "codeAction": {
                            "dynamicRegistration": true,
                            "isPreferredSupport": true,
                            "disabledSupport": true,
                            "dataSupport": true,
                            "resolveSupport": { "properties": ["edit", "command"] },
                            "codeActionLiteralSupport": {
                                "codeActionKind": { "valueSet": ["", "quickfix", "refactor", "source"] }
                            }
                        }
                    },
                    "workspace": {
                        "applyEdit": true,
                        "workspaceEdit": {
                            "documentChanges": true,
                            "resourceOperations": [],
                            "failureHandling": "transactional",
                            "normalizesLineEndings": false
                        },
                        "configuration": true,
                        "workspaceFolders": true,
                        "didChangeWatchedFiles": { "dynamicRegistration": true },
                        "symbol": {
                            "dynamicRegistration": true,
                            "resolveSupport": { "properties": ["location.range"] },
                            "tagSupport": { "valueSet": [1] }
                        }
                    },
                    "window": { "workDoneProgress": true }
                }
            }),
            REQUEST_TIMEOUT,
        )?;
        session.text_document_sync = parse_text_document_sync(&initialize_result);
        session.text_document_save = parse_text_document_save(&initialize_result);
        session.static_capabilities = parse_static_capabilities(&initialize_result);
        session.static_workspace_symbol_resolve = initialize_result
            .pointer("/capabilities/workspaceSymbolProvider/resolveProvider")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        session.static_code_action_resolve = initialize_result
            .pointer("/capabilities/codeActionProvider/resolveProvider")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        session.execute_commands = initialize_result
            .pointer("/capabilities/executeCommandProvider/commands")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|command| !command.is_empty() && command.len() <= MAX_CAPABILITY_STRING_BYTES)
            .map(str::to_owned)
            .collect();
        session.notify("initialized", json!({}))?;
        Ok(session)
    }

    fn open_document(
        &self,
        uri: &str,
        language_id: &str,
        version: i64,
        text: &str,
    ) -> Result<(), LanguageServerError> {
        self.clear_diagnostics(uri);
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": language_id,
                    "version": version,
                    "text": text
                }
            }),
        )
    }

    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
            && self
                .child
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .try_wait()
                .ok()
                .flatten()
                .is_none()
    }

    fn mark_intentional(&self) {
        self.intentional_shutdown.store(true, Ordering::Release);
        self.clear_dynamic_capabilities();
    }

    fn terminate(&self) {
        self.mark_intentional();
        self.alive.store(false, Ordering::Release);
        terminate_child(&self.child);
    }

    fn supports(&self, method: &str) -> bool {
        capability_is_supported(
            &self.static_capabilities,
            &self
                .dynamic_capabilities
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
            method,
        )
    }

    fn supports_workspace_symbol_resolve(&self) -> bool {
        self.static_workspace_symbol_resolve
            || self
                .dynamic_capabilities
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .registrations
                .values()
                .any(|registration| {
                    registration.method == "workspace/symbol" && registration.resolve_provider
                })
    }

    fn supports_code_action_resolve(&self) -> bool {
        self.static_code_action_resolve
            || self
                .dynamic_capabilities
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .registrations
                .values()
                .any(|registration| {
                    registration.method == "textDocument/codeAction"
                        && registration.resolve_provider
                })
    }

    fn advertises_command(&self, command: &str) -> bool {
        self.execute_commands.contains(command)
            || self
                .dynamic_capabilities
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .registrations
                .values()
                .any(|registration| {
                    registration.method == "workspace/executeCommand"
                        && registration.commands.contains(command)
                })
    }

    fn clear_dynamic_capabilities(&self) {
        let mut capabilities = self
            .dynamic_capabilities
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        capabilities.registrations.clear();
        capabilities.progress_tokens.clear();
    }

    fn change_document(
        &self,
        uri: &str,
        version: i64,
        changes: &[LanguageServerChange],
        text: &str,
    ) -> Result<(), LanguageServerError> {
        self.clear_diagnostics(uri);
        let content_changes = match self.text_document_sync {
            TextDocumentSyncKind::Full => vec![json!({ "text": text })],
            TextDocumentSyncKind::Incremental => changes
                .iter()
                .map(|change| {
                    json!({
                        "range": range_json(change.range),
                        "text": change.text
                    })
                })
                .collect::<Vec<_>>(),
        };
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri, "version": version },
                "contentChanges": content_changes
            }),
        )
    }

    fn close_document(&self, uri: &str) -> Result<(), LanguageServerError> {
        self.clear_diagnostics(uri);
        self.notify(
            "textDocument/didClose",
            json!({ "textDocument": { "uri": uri } }),
        )
    }

    fn save_document(&self, uri: &str, text: &str) -> Result<(), LanguageServerError> {
        let Some(include_text) = self.text_document_save else {
            return Ok(());
        };
        let mut params = json!({ "textDocument": { "uri": uri } });
        if include_text {
            params["text"] = Value::String(text.to_owned());
        }
        self.notify("textDocument/didSave", params)
    }

    fn diagnostics(&self, uri: &str, version: i64) -> Option<Vec<LanguageServerDiagnostic>> {
        let diagnostics = self
            .diagnostics
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let snapshot = diagnostics.get(uri)?;
        if snapshot.revision.version != version {
            return None;
        }
        if snapshot
            .version
            .is_some_and(|published| published != version)
        {
            return None;
        }
        if snapshot.published_at.elapsed() < DIAGNOSTIC_SETTLE_DELAY {
            return None;
        }
        Some(snapshot.diagnostics.clone())
    }

    fn clear_diagnostics(&self, uri: &str) {
        self.diagnostics
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(uri);
    }

    fn hover(
        &self,
        uri: &str,
        position: LanguageServerPosition,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Option<LanguageServerHover>, LanguageServerError> {
        let result = self.request_cancellable(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": uri },
                "position": position_json(position)
            }),
            FEATURE_TIMEOUT,
            cancellation,
        )?;
        parse_hover(result)
    }

    fn signature_help(
        &self,
        uri: &str,
        position: LanguageServerPosition,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Option<LanguageServerSignatureHelp>, LanguageServerError> {
        let result = self.request_cancellable(
            "textDocument/signatureHelp",
            json!({
                "textDocument": { "uri": uri },
                "position": position_json(position),
                "context": { "triggerKind": 1, "isRetrigger": false }
            }),
            FEATURE_TIMEOUT,
            cancellation,
        )?;
        parse_signature_help(result)
    }

    fn locations(
        &self,
        method: &str,
        uri: &str,
        position: LanguageServerPosition,
        include_declaration: Option<bool>,
        roots: &[WorkspaceRoot],
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerLocation>, LanguageServerError> {
        let mut params = json!({
            "textDocument": { "uri": uri },
            "position": position_json(position)
        });
        if let Some(include_declaration) = include_declaration {
            params["context"] = json!({ "includeDeclaration": include_declaration });
        }
        let result = self.request_cancellable(method, params, FEATURE_TIMEOUT, cancellation)?;
        parse_locations(&result, roots)
    }

    fn document_symbols(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        uri: &str,
        roots: &[WorkspaceRoot],
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerDocumentSymbol>, LanguageServerError> {
        let result = self.request_cancellable(
            "textDocument/documentSymbol",
            json!({ "textDocument": { "uri": uri } }),
            FEATURE_TIMEOUT,
            cancellation,
        )?;
        parse_document_symbols(&result, workspace_id, path, roots)
    }

    fn workspace_symbols(
        &self,
        workspace_id: WorkspaceId,
        query: &str,
        roots: &[WorkspaceRoot],
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerWorkspaceSymbol>, LanguageServerError> {
        let result = self.request_cancellable(
            "workspace/symbol",
            json!({ "query": query }),
            FEATURE_TIMEOUT,
            cancellation,
        )?;
        parse_workspace_symbols(
            &result,
            self.server_id,
            workspace_id,
            self.supports_workspace_symbol_resolve(),
            roots,
        )
    }

    fn resolve_workspace_symbol(
        &self,
        request: LanguageServerWorkspaceSymbolResolveRequest,
        roots: &[WorkspaceRoot],
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<LanguageServerWorkspaceSymbol, LanguageServerError> {
        let result = self.request_cancellable(
            "workspaceSymbol/resolve",
            request.raw,
            FEATURE_TIMEOUT,
            cancellation,
        )?;
        parse_workspace_symbol(
            &result,
            &request.server_id,
            request.workspace_id,
            true,
            roots,
        )
        .ok_or_else(|| {
            LanguageServerError::Protocol("invalid workspace symbol resolve response".to_owned())
        })
    }

    fn completion(
        &self,
        server_id: &str,
        uri: &str,
        completion: &LanguageServerCompletionRequest,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<LanguageServerCompletionList, LanguageServerError> {
        let result = self.request_cancellable(
            "textDocument/completion",
            json!({
                "textDocument": { "uri": uri },
                "position": position_json(completion.position),
                "context": {
                    "triggerKind": completion.trigger_kind,
                    "triggerCharacter": completion.trigger_character
                }
            }),
            FEATURE_TIMEOUT,
            cancellation,
        )?;
        parse_completion_list(result, server_id)
    }

    fn resolve_completion(
        &self,
        server_id: &str,
        raw: Value,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<LanguageServerCompletionItem, LanguageServerError> {
        let result =
            self.request_cancellable("completionItem/resolve", raw, FEATURE_TIMEOUT, cancellation)?;
        parse_completion_item(&result, None, server_id).ok_or_else(|| {
            LanguageServerError::Protocol("invalid completion item response".to_owned())
        })
    }

    fn document_colors(
        &self,
        server_id: &str,
        uri: &str,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerColorInformation>, LanguageServerError> {
        let result = self.request_cancellable(
            "textDocument/documentColor",
            json!({ "textDocument": { "uri": uri } }),
            FEATURE_TIMEOUT,
            cancellation,
        )?;
        Ok(result
            .as_array()
            .map(|colors| {
                colors
                    .iter()
                    .filter_map(|color| parse_color_information(color, server_id))
                    .collect()
            })
            .unwrap_or_default())
    }

    fn color_presentations(
        &self,
        uri: &str,
        range: LanguageServerRange,
        color: LanguageServerColor,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerColorPresentation>, LanguageServerError> {
        let result = self.request_cancellable(
            "textDocument/colorPresentation",
            json!({
                "textDocument": { "uri": uri },
                "range": range_json(range),
                "color": {
                    "red": color.red,
                    "green": color.green,
                    "blue": color.blue,
                    "alpha": color.alpha
                }
            }),
            FEATURE_TIMEOUT,
            cancellation,
        )?;
        Ok(result
            .as_array()
            .map(|presentations| {
                presentations
                    .iter()
                    .filter_map(parse_color_presentation)
                    .collect()
            })
            .unwrap_or_default())
    }

    fn formatting(
        &self,
        uri: &str,
        options: LanguageServerFormattingOptions,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerTextEdit>, LanguageServerError> {
        let result = self.request_cancellable(
            "textDocument/formatting",
            json!({
                "textDocument": { "uri": uri },
                "options": {
                    "tabSize": options.tab_size,
                    "insertSpaces": options.insert_spaces
                }
            }),
            FEATURE_TIMEOUT,
            cancellation,
        )?;
        parse_text_edits(result)
    }

    fn prepare_rename(
        &self,
        uri: &str,
        position: LanguageServerPosition,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<PrepareRenameResult, LanguageServerError> {
        let result = self.request_cancellable(
            "textDocument/prepareRename",
            json!({ "textDocument": { "uri": uri }, "position": position_json(position) }),
            FEATURE_TIMEOUT,
            cancellation,
        )?;
        if result.is_null() {
            return Ok(PrepareRenameResult::Rejected);
        }
        if result.get("defaultBehavior").and_then(Value::as_bool) == Some(true) {
            return Ok(PrepareRenameResult::Default);
        }
        let range_value = result.get("range").unwrap_or(&result);
        let range = parse_range(range_value).ok_or_else(|| {
            LanguageServerError::Protocol(
                "prepare rename response contained an invalid range".to_owned(),
            )
        })?;
        Ok(PrepareRenameResult::Range(
            range,
            result
                .get("placeholder")
                .and_then(Value::as_str)
                .map(str::to_owned),
        ))
    }

    fn rename(
        &self,
        uri: &str,
        position: LanguageServerPosition,
        new_name: &str,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Value, LanguageServerError> {
        let result = self.request_cancellable(
            "textDocument/rename",
            json!({
                "textDocument": { "uri": uri },
                "position": position_json(position),
                "newName": new_name
            }),
            REQUEST_TIMEOUT,
            cancellation,
        )?;
        if result.is_null() {
            Ok(json!({ "changes": {} }))
        } else if result.is_object() {
            Ok(result)
        } else {
            Err(LanguageServerError::Protocol(
                "rename response must be a workspace edit or null".to_owned(),
            ))
        }
    }

    fn code_actions(
        &self,
        server_id: &str,
        uri: &str,
        request: &LanguageServerCodeActionRequest,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerCodeAction>, LanguageServerError> {
        let result = self.request_cancellable(
            "textDocument/codeAction",
            json!({
                "textDocument": { "uri": uri },
                "range": range_json(request.range),
                "context": request.context
            }),
            FEATURE_TIMEOUT,
            cancellation,
        )?;
        let Some(actions) = result.as_array() else {
            return if result.is_null() {
                Ok(Vec::new())
            } else {
                Err(LanguageServerError::Protocol(
                    "code action response must be an array or null".to_owned(),
                ))
            };
        };
        Ok(actions
            .iter()
            .filter_map(|action| {
                parse_code_action(action, server_id, self.supports_code_action_resolve())
            })
            .collect())
    }

    fn resolve_code_action(
        &self,
        request: LanguageServerCodeActionResolveRequest,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<LanguageServerCodeAction, LanguageServerError> {
        let result = self.request_cancellable(
            "codeAction/resolve",
            request.raw,
            FEATURE_TIMEOUT,
            cancellation,
        )?;
        let mut action = parse_code_action(&result, &request.server_id, true).ok_or_else(|| {
            LanguageServerError::Protocol("invalid code action resolve response".to_owned())
        })?;
        action.action_id = request.action_id;
        Ok(action)
    }

    fn execute_command(
        &self,
        command: &str,
        arguments: Vec<Value>,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Value, LanguageServerError> {
        self.request_cancellable(
            "workspace/executeCommand",
            json!({ "command": command, "arguments": arguments }),
            REQUEST_TIMEOUT,
            cancellation,
        )
    }

    fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, LanguageServerError> {
        self.request_cancellable(
            method,
            params,
            timeout,
            &LanguageServerRequestCancellation::new(),
        )
    }

    fn request_cancellable(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Value, LanguageServerError> {
        let result = request_with_transport(
            &self.writer,
            &self.pending,
            &self.next_request_id,
            &self.alive,
            method,
            params,
            timeout,
            cancellation,
        );
        if let Err(error) = &result
            && !matches!(error, LanguageServerError::RequestCancelled)
        {
            self.logs.append(
                self.server_id,
                LanguageServerLogKind::Runtime,
                format!("{method}: {error}"),
            );
        }
        if let Err(error @ (LanguageServerError::Protocol(_) | LanguageServerError::ServerExited)) =
            &result
        {
            self.exit_reporter
                .report(format!("{method} transport failed: {error}"));
        }
        result
    }

    fn notify(&self, method: &str, params: Value) -> Result<(), LanguageServerError> {
        let result = send_json(
            &self.writer,
            &json!({ "jsonrpc": "2.0", "method": method, "params": params }),
        );
        if let Err(error) = &result {
            self.exit_reporter
                .report(format!("{method} notification failed: {error}"));
        }
        result
    }
}

fn capability_is_supported(
    static_capabilities: &HashSet<String>,
    capabilities: &DynamicCapabilityState,
    method: &str,
) -> bool {
    static_capabilities.contains(method)
        || (method == "textDocument/prepareRename"
            && capabilities.registrations.values().any(|registration| {
                registration.method == "textDocument/rename" && registration.prepare_provider
            }))
        || capabilities
            .registrations
            .values()
            .any(|registration| registration.method == method)
}

#[allow(clippy::too_many_arguments)]
fn request_with_transport(
    writer: &MessageWriter,
    pending: &PendingRequests,
    next_request_id: &AtomicI64,
    alive: &AtomicBool,
    method: &str,
    params: Value,
    timeout: Duration,
    cancellation: &LanguageServerRequestCancellation,
) -> Result<Value, LanguageServerError> {
    if !alive.load(Ordering::Acquire) {
        return Err(LanguageServerError::ServerExited);
    }
    if cancellation.is_cancelled() {
        return Err(LanguageServerError::RequestCancelled);
    }
    let id = next_request_id.fetch_add(1, Ordering::Relaxed);
    let (sender, receiver) = mpsc::sync_channel(1);
    pending
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(id, sender);
    if let Err(error) = send_json(
        writer,
        &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
    ) {
        pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&id);
        return Err(error);
    }

    let pending_for_cancel = Arc::clone(pending);
    let writer_for_cancel = writer.clone();
    cancellation
        .on_cancel(move || cancel_pending_request(&pending_for_cancel, &writer_for_cancel, id));

    match receiver.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            let removed = pending
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(&id);
            if removed.is_some() {
                let _ = send_json(
                    writer,
                    &json!({
                        "jsonrpc": "2.0",
                        "method": "$/cancelRequest",
                        "params": { "id": id }
                    }),
                );
                Err(LanguageServerError::RequestTimeout)
            } else {
                receiver
                    .recv()
                    .unwrap_or(Err(LanguageServerError::ServerExited))
            }
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(LanguageServerError::ServerExited),
    }
}

fn cancel_pending_request(pending: &PendingRequests, writer: &MessageWriter, id: i64) {
    let sender = pending
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .remove(&id);
    let Some(sender) = sender else {
        return;
    };
    let _ = send_json(
        writer,
        &json!({ "jsonrpc": "2.0", "method": "$/cancelRequest", "params": { "id": id } }),
    );
    let _ = sender.send(Err(LanguageServerError::RequestCancelled));
}

impl Drop for LanguageServerSession {
    fn drop(&mut self) {
        self.mark_intentional();
        if self.alive.load(Ordering::Acquire) {
            let _ = self.request("shutdown", Value::Null, SHUTDOWN_TIMEOUT);
            let _ = self.notify("exit", Value::Null);
        }
        terminate_child(&self.child);
    }
}

fn terminate_child(child: &Mutex<Child>) {
    let mut child = child.lock().unwrap_or_else(|error| error.into_inner());
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_reader(
    stdout: impl Read + Send + 'static,
    writer: MessageWriter,
    pending: PendingRequests,
    diagnostics: PublishedDiagnostics,
    project_root: PathBuf,
    workspace_root: PathBuf,
    dynamic_capabilities: Arc<Mutex<DynamicCapabilityState>>,
    workspace_id: WorkspaceId,
    documents: Arc<Mutex<HashMap<DocumentKey, DocumentBinding>>>,
    workspace_edits: Arc<WorkspaceEditTransactions>,
    events: CoreEventDispatcher,
    server_id: &'static str,
    logs: RuntimeLogs,
    exit_reporter: SessionExitReporter,
) -> Result<(), LanguageServerError> {
    thread::Builder::new()
        .name("kosmos-language-server-reader".to_owned())
        .spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut exit_reason = "process output closed".to_owned();
            loop {
                let message = match read_message(&mut reader) {
                    Ok(Some(message)) => message,
                    Ok(None) => break,
                    Err(error) => {
                        exit_reason = format!("protocol reader failed: {error}");
                        logs.append(
                            server_id,
                            LanguageServerLogKind::Runtime,
                            format!("protocol reader stopped: {error}"),
                        );
                        break;
                    }
                };
                if let Some(id) = message.get("id").and_then(Value::as_i64)
                    && message.get("method").is_none()
                {
                    let sender = pending
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .remove(&id);
                    if let Some(sender) = sender {
                        let result = client_response_result(&message);
                        let _ = sender.send(result);
                    }
                    continue;
                }

                if message.get("method").and_then(Value::as_str)
                    == Some("textDocument/publishDiagnostics")
                {
                    let revision = published_document_revision(
                        &documents,
                        &exit_reporter.key,
                        message.pointer("/params/uri").and_then(Value::as_str),
                    );
                    if let Some((uri, version, revision, published)) =
                        store_published_diagnostics(&diagnostics, &message, revision)
                    {
                        exit_reporter.publish_diagnostics(uri, version, revision, published);
                    }
                    continue;
                }

                if message.get("method").and_then(Value::as_str) == Some("$/progress") {
                    finish_progress(&message, &dynamic_capabilities);
                    continue;
                }

                if message.get("method").is_some() && message.get("id").is_some() {
                    let response = server_request_response(
                        &message,
                        &ServerRequestContext {
                            writer: &writer,
                            project_root: &project_root,
                            workspace_root: &workspace_root,
                            dynamic_capabilities: &dynamic_capabilities,
                            workspace_id,
                            documents: &documents,
                            workspace_edits: &workspace_edits,
                            events: &events,
                        },
                    );
                    let _ = send_json(&writer, &response);
                }
            }

            logs.append(
                server_id,
                LanguageServerLogKind::Runtime,
                "language server output closed".to_owned(),
            );
            exit_reporter.report(exit_reason);
            let pending =
                std::mem::take(&mut *pending.lock().unwrap_or_else(|error| error.into_inner()));
            for sender in pending.into_values() {
                let _ = sender.send(Err(LanguageServerError::ServerExited));
            }
        })
        .map(|_| ())
        .map_err(|error| LanguageServerError::ServerStart(error.to_string()))
}

fn spawn_stderr(
    mut stderr: impl Read + Send + 'static,
    server_id: &'static str,
    logs: RuntimeLogs,
) -> Result<(), LanguageServerError> {
    thread::Builder::new()
        .name("kosmos-language-server-stderr".to_owned())
        .spawn(move || {
            let mut buffer = [0_u8; 4 * 1024];
            loop {
                match stderr.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        let message = String::from_utf8_lossy(&buffer[..count]).trim().to_owned();
                        if !message.is_empty() {
                            logs.append(server_id, LanguageServerLogKind::Stderr, message);
                        }
                    }
                    Err(error) => {
                        logs.append(
                            server_id,
                            LanguageServerLogKind::Runtime,
                            format!("stderr reader stopped: {error}"),
                        );
                        break;
                    }
                }
            }
        })
        .map(|_| ())
        .map_err(|error| LanguageServerError::ServerStart(error.to_string()))
}

fn spawn_process_watcher(
    child: Arc<Mutex<Child>>,
    exit_reporter: SessionExitReporter,
) -> Result<(), LanguageServerError> {
    thread::Builder::new()
        .name("kosmos-language-server-process".to_owned())
        .spawn(move || {
            while !exit_reporter.reported.load(Ordering::Acquire)
                && !exit_reporter.intentional.load(Ordering::Acquire)
            {
                let status = child
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .try_wait();
                match status {
                    Ok(Some(status)) => {
                        exit_reporter.report(format!("process exited with {status}"));
                        break;
                    }
                    Ok(None) => thread::sleep(Duration::from_millis(100)),
                    Err(error) => {
                        exit_reporter.report(format!("process status failed: {error}"));
                        break;
                    }
                }
            }
        })
        .map(|_| ())
        .map_err(|error| LanguageServerError::ServerStart(error.to_string()))
}

impl RuntimeLogs {
    fn append(&self, server_id: &str, kind: LanguageServerLogKind, message: String) {
        let message = truncate_utf8(message, MAX_SERVER_LOG_ENTRY_BYTES);
        let mut buffers = self
            .buffers
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let buffer = buffers.entry(server_id.to_owned()).or_default();
        buffer.bytes += message.len();
        buffer
            .entries
            .push_back(LanguageServerLog { kind, message });
        while buffer.entries.len() > MAX_SERVER_LOG_ENTRIES || buffer.bytes > MAX_SERVER_LOG_BYTES {
            let Some(removed) = buffer.entries.pop_front() else {
                break;
            };
            buffer.bytes = buffer.bytes.saturating_sub(removed.message.len());
        }
        drop(buffers);
        self.events.emit(CoreEvent::LanguageServerLogAvailable {
            server_id: server_id.to_owned(),
        });
    }

    fn entries(&self, server_id: &str) -> Vec<LanguageServerLog> {
        self.buffers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(server_id)
            .map(|buffer| buffer.entries.iter().cloned().collect())
            .unwrap_or_default()
    }
}

fn truncate_utf8(mut value: String, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    value
}

fn spawn_writer(mut stdin: ChildStdin) -> Result<MessageWriter, LanguageServerError> {
    let (sender, receiver) = mpsc::sync_channel::<OutboundMessage>(64);
    thread::Builder::new()
        .name("kosmos-language-server-writer".to_owned())
        .spawn(move || {
            while let Ok(message) = receiver.recv() {
                let result = write!(stdin, "Content-Length: {}\r\n\r\n", message.body.len())
                    .and_then(|()| stdin.write_all(&message.body))
                    .and_then(|()| stdin.flush())
                    .map_err(|error| error.to_string());
                let failed = result.is_err();
                let _ = message.completion.send(result);
                if failed {
                    break;
                }
            }
        })
        .map_err(|error| LanguageServerError::ServerStart(error.to_string()))?;
    Ok(MessageWriter { sender })
}

fn store_published_diagnostics(
    diagnostics: &PublishedDiagnostics,
    message: &Value,
    revision: Option<DocumentRevision>,
) -> Option<(
    String,
    Option<i64>,
    DocumentRevision,
    Vec<LanguageServerDiagnostic>,
)> {
    let uri = message.pointer("/params/uri").and_then(Value::as_str)?;
    let revision = revision?;
    let version = message.pointer("/params/version").and_then(Value::as_i64);
    if version.is_some_and(|version| version != revision.version) {
        return None;
    }
    let parsed: Vec<LanguageServerDiagnostic> = message
        .pointer("/params/diagnostics")
        .and_then(Value::as_array)
        .map(|diagnostics| diagnostics.iter().filter_map(parse_diagnostic).collect())
        .unwrap_or_default();
    diagnostics
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(
            uri.to_owned(),
            DiagnosticSnapshot {
                version,
                revision,
                diagnostics: parsed.clone(),
                published_at: Instant::now(),
            },
        );
    Some((uri.to_owned(), version, revision, parsed))
}

fn published_document_revision(
    documents: &Mutex<HashMap<DocumentKey, DocumentBinding>>,
    session_key: &SessionKey,
    uri: Option<&str>,
) -> Option<DocumentRevision> {
    let uri = uri?;
    documents
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .values()
        .find(|document| document.session_key == *session_key && document.uri == uri)
        .map(|document| DocumentRevision {
            generation: document.generation,
            version: document.version,
        })
}

fn client_response_result(message: &Value) -> PendingResponse {
    let Some(error) = message.get("error") else {
        return Ok(message.get("result").cloned().unwrap_or(Value::Null));
    };
    let code = error.get("code").and_then(Value::as_i64);
    match code {
        Some(-32800) | Some(-32802) => Err(LanguageServerError::RequestCancelled),
        Some(-32801) => Err(LanguageServerError::ContentModified),
        _ => Err(LanguageServerError::RequestFailed(
            error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("language server request failed")
                .to_owned(),
        )),
    }
}

fn parse_diagnostic(value: &Value) -> Option<LanguageServerDiagnostic> {
    let range = value.get("range").and_then(parse_range)?;
    let message = value.get("message").and_then(Value::as_str)?.to_owned();
    let severity = match value.get("severity").and_then(Value::as_u64) {
        Some(1) => Some(LanguageServerDiagnosticSeverity::Error),
        Some(2) => Some(LanguageServerDiagnosticSeverity::Warning),
        Some(3) => Some(LanguageServerDiagnosticSeverity::Information),
        Some(4) => Some(LanguageServerDiagnosticSeverity::Hint),
        _ => None,
    };
    let code = value.get("code").and_then(|code| match code {
        Value::String(code) => Some(code.clone()),
        Value::Number(code) => Some(code.to_string()),
        _ => None,
    });
    Some(LanguageServerDiagnostic {
        range,
        severity,
        message,
        source: value
            .get("source")
            .and_then(Value::as_str)
            .map(str::to_owned),
        code,
    })
}

fn read_message(reader: &mut impl BufRead) -> Result<Option<Value>, LanguageServerError> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let count = reader
            .read_line(&mut line)
            .map_err(|error| LanguageServerError::Protocol(error.to_string()))?;
        if count == 0 {
            return Ok(None);
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("Content-Length")
        {
            content_length = value.trim().parse::<usize>().ok();
        }
    }
    let content_length = content_length.ok_or_else(|| {
        LanguageServerError::Protocol("language server message omitted Content-Length".to_owned())
    })?;
    if content_length > MAX_LSP_MESSAGE_BYTES {
        return Err(LanguageServerError::Protocol(
            "language server message exceeded the size limit".to_owned(),
        ));
    }
    let mut body = vec![0_u8; content_length];
    reader
        .read_exact(&mut body)
        .map_err(|error| LanguageServerError::Protocol(error.to_string()))?;
    serde_json::from_slice(&body).map(Some).map_err(|error| {
        LanguageServerError::Protocol(format!("invalid language server JSON: {error}"))
    })
}

fn send_json(writer: &MessageWriter, message: &Value) -> Result<(), LanguageServerError> {
    let body = serde_json::to_vec(message)
        .map_err(|error| LanguageServerError::Protocol(error.to_string()))?;
    if body.len() > MAX_LSP_MESSAGE_BYTES {
        return Err(LanguageServerError::Protocol(
            "language server message exceeded the size limit".to_owned(),
        ));
    }
    let (completion, result) = mpsc::sync_channel(1);
    writer
        .sender
        .try_send(OutboundMessage { body, completion })
        .map_err(|error| {
            LanguageServerError::Protocol(format!("language server writer is unavailable: {error}"))
        })?;
    result
        .recv_timeout(WRITE_TIMEOUT)
        .map_err(|_| LanguageServerError::Protocol("language server write timed out".to_owned()))?
        .map_err(LanguageServerError::Protocol)
}

fn server_request_response(message: &Value, context: &ServerRequestContext<'_>) -> Value {
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    let result = match message.get("method").and_then(Value::as_str) {
        Some("workspace/configuration") => configuration_result(message),
        Some("workspace/workspaceFolders") => Ok(json!([{
            "uri": file_uri(context.project_root),
            "name": context
                .project_root
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("workspace")
        }])),
        Some("client/registerCapability") => register_capabilities(message, context),
        Some("client/unregisterCapability") => unregister_capabilities(message, context),
        Some("window/workDoneProgress/create") => create_progress_token(message, context),
        Some("workspace/applyEdit") => apply_server_workspace_edit(message, context),
        Some(method) => Err(JsonRpcError {
            code: -32601,
            message: format!("method not found: {method}"),
        }),
        None => Err(invalid_params("request method must be a string")),
    };
    match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(error) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": error.code, "message": error.message }
        }),
    }
}

fn apply_server_workspace_edit(
    message: &Value,
    context: &ServerRequestContext<'_>,
) -> Result<Value, JsonRpcError> {
    let edit = message
        .pointer("/params/edit")
        .ok_or_else(|| invalid_params("workspace/applyEdit requires an edit"))?;
    let roots = [WorkspaceEditRoot {
        workspace_id: context.workspace_id,
        path: context.workspace_root.to_path_buf(),
    }];
    let open_documents = open_document_snapshots(context.documents);
    let staged = match context.workspace_edits.stage(edit, &roots, &open_documents) {
        Ok(staged) => staged,
        Err(error) => {
            return Ok(json!({
                "applied": false,
                "failureReason": error.to_string()
            }));
        }
    };
    let result = context.events.apply_workspace_edit(staged);
    let mut response = json!({ "applied": result.applied });
    if let Some(reason) = result.failure_reason {
        response["failureReason"] = Value::String(reason);
    }
    Ok(response)
}

fn open_document_snapshots(
    documents: &Mutex<HashMap<DocumentKey, DocumentBinding>>,
) -> Vec<WorkspaceEditOpenDocument> {
    let mut open = HashMap::new();
    for (key, document) in documents
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .iter()
    {
        open.entry((key.workspace_id, key.path.clone()))
            .or_insert_with(|| WorkspaceEditOpenDocument {
                workspace_id: key.workspace_id,
                path: key.path.clone(),
                generation: document.generation,
                version: document.version,
                text: document.text.clone(),
            });
    }
    open.into_values().collect()
}

fn configuration_result(message: &Value) -> Result<Value, JsonRpcError> {
    let items = message
        .pointer("/params/items")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_params("workspace/configuration requires an items array"))?;
    if items.len() > MAX_CONFIGURATION_ITEMS {
        return Err(invalid_params(
            "workspace/configuration requested too many items",
        ));
    }
    if items.iter().any(|item| !item.is_object()) {
        return Err(invalid_params(
            "workspace/configuration items must be objects",
        ));
    }
    Ok(Value::Array(vec![Value::Null; items.len()]))
}

fn register_capabilities(
    message: &Value,
    context: &ServerRequestContext<'_>,
) -> Result<Value, JsonRpcError> {
    let registrations = message
        .pointer("/params/registrations")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_params("client/registerCapability requires registrations"))?;
    if registrations.is_empty() {
        return Err(invalid_params("registrations must not be empty"));
    }
    if registrations.len() > MAX_DYNAMIC_REGISTRATIONS {
        return Err(invalid_params("dynamic capability limit exceeded"));
    }

    let mut prepared = Vec::with_capacity(registrations.len());
    let mut request_ids = HashSet::new();
    for registration in registrations {
        let id = registration
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && id.len() <= MAX_CAPABILITY_STRING_BYTES)
            .ok_or_else(|| invalid_params("capability registration requires a non-empty id"))?;
        if !request_ids.insert(id.to_owned()) {
            return Err(invalid_params("capability registration ids must be unique"));
        }
        let method = registration
            .get("method")
            .and_then(Value::as_str)
            .filter(|method| !method.is_empty() && method.len() <= MAX_CAPABILITY_STRING_BYTES)
            .ok_or_else(|| invalid_params("capability registration requires a method"))?;
        let watch_patterns = if method == "workspace/didChangeWatchedFiles" {
            Some(prepare_watch_patterns(
                registration.get("registerOptions"),
                context,
            )?)
        } else {
            None
        };
        prepared.push(PreparedRegistration {
            id: id.to_owned(),
            method: method.to_owned(),
            resolve_provider: registration
                .pointer("/registerOptions/resolveProvider")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            prepare_provider: registration
                .pointer("/registerOptions/prepareProvider")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            commands: registration
                .pointer("/registerOptions/commands")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|command| {
                    !command.is_empty() && command.len() <= MAX_CAPABILITY_STRING_BYTES
                })
                .map(str::to_owned)
                .collect(),
            watch_patterns,
        });
    }

    {
        let capabilities = context
            .dynamic_capabilities
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if capabilities.registrations.len() + prepared.len() > MAX_DYNAMIC_REGISTRATIONS {
            return Err(invalid_params("dynamic capability limit exceeded"));
        }
        if prepared
            .iter()
            .any(|registration| capabilities.registrations.contains_key(&registration.id))
        {
            return Err(invalid_params("capability registration id already exists"));
        }
        let existing_patterns = capabilities
            .registrations
            .values()
            .map(|registration| registration.watch_pattern_count)
            .sum::<usize>();
        let new_patterns = prepared
            .iter()
            .filter_map(|registration| registration.watch_patterns.as_ref())
            .map(Vec::len)
            .sum::<usize>();
        if existing_patterns + new_patterns > MAX_WATCH_PATTERNS {
            return Err(invalid_params("watched file pattern limit exceeded"));
        }
    }

    let mut started = Vec::with_capacity(prepared.len());
    for registration in prepared {
        let watch_pattern_count = registration.watch_patterns.as_ref().map_or(0, Vec::len);
        let watched_files = registration
            .watch_patterns
            .map(|patterns| WatchedFilesRegistration::start(context.writer.clone(), patterns))
            .transpose()
            .map_err(|error| JsonRpcError {
                code: -32603,
                message: format!("failed to register watched files: {error}"),
            })?;
        started.push((
            registration.id,
            RegisteredCapability {
                method: registration.method,
                resolve_provider: registration.resolve_provider,
                prepare_provider: registration.prepare_provider,
                commands: registration.commands,
                _watched_files: watched_files,
                watch_pattern_count,
            },
        ));
    }

    let mut capabilities = context
        .dynamic_capabilities
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    for (id, registration) in started {
        capabilities.registrations.insert(id, registration);
    }
    Ok(Value::Null)
}

fn unregister_capabilities(
    message: &Value,
    context: &ServerRequestContext<'_>,
) -> Result<Value, JsonRpcError> {
    let unregisterations = message
        .pointer("/params/unregisterations")
        .or_else(|| message.pointer("/params/unregistrations"))
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_params("client/unregisterCapability requires unregisterations"))?;
    if unregisterations.is_empty() {
        return Err(invalid_params("unregisterations must not be empty"));
    }
    if unregisterations.len() > MAX_DYNAMIC_REGISTRATIONS {
        return Err(invalid_params("too many capability unregisterations"));
    }
    let mut requested = Vec::with_capacity(unregisterations.len());
    let mut ids = HashSet::new();
    for unregistration in unregisterations {
        let id = unregistration
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && id.len() <= MAX_CAPABILITY_STRING_BYTES)
            .ok_or_else(|| invalid_params("capability unregistration requires an id"))?;
        let method = unregistration
            .get("method")
            .and_then(Value::as_str)
            .filter(|method| !method.is_empty() && method.len() <= MAX_CAPABILITY_STRING_BYTES)
            .ok_or_else(|| invalid_params("capability unregistration requires a method"))?;
        if !ids.insert(id.to_owned()) {
            return Err(invalid_params(
                "capability unregistration ids must be unique",
            ));
        }
        requested.push((id.to_owned(), method.to_owned()));
    }

    let removed = {
        let mut capabilities = context
            .dynamic_capabilities
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for (id, method) in &requested {
            let Some(registration) = capabilities.registrations.get(id) else {
                return Err(invalid_params("capability registration id does not exist"));
            };
            if registration.method != *method {
                return Err(invalid_params(
                    "capability unregistration method does not match",
                ));
            }
        }
        requested
            .into_iter()
            .filter_map(|(id, _)| capabilities.registrations.remove(&id))
            .collect::<Vec<_>>()
    };
    drop(removed);
    Ok(Value::Null)
}

fn create_progress_token(
    message: &Value,
    context: &ServerRequestContext<'_>,
) -> Result<Value, JsonRpcError> {
    let token = message
        .pointer("/params/token")
        .filter(|token| token.is_string() || token.is_number())
        .ok_or_else(|| invalid_params("work done progress token must be a string or number"))?;
    let token = progress_token_key(token)?;
    if token.len() > MAX_CAPABILITY_STRING_BYTES {
        return Err(invalid_params("work done progress token is too large"));
    }
    let mut capabilities = context
        .dynamic_capabilities
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if capabilities.progress_tokens.contains(&token) {
        return Err(invalid_params("work done progress token already exists"));
    }
    if capabilities.progress_tokens.len() >= MAX_PROGRESS_TOKENS {
        return Err(invalid_params("work done progress token limit exceeded"));
    }
    capabilities.progress_tokens.insert(token);
    Ok(Value::Null)
}

fn finish_progress(message: &Value, capabilities: &Arc<Mutex<DynamicCapabilityState>>) {
    if message
        .pointer("/params/value/kind")
        .and_then(Value::as_str)
        != Some("end")
    {
        return;
    }
    let Some(token) = message
        .pointer("/params/token")
        .and_then(|token| progress_token_key(token).ok())
    else {
        return;
    };
    capabilities
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .progress_tokens
        .remove(&token);
}

fn progress_token_key(token: &Value) -> Result<String, JsonRpcError> {
    if !token.is_string() && !token.is_number() {
        return Err(invalid_params(
            "work done progress token must be a string or number",
        ));
    }
    serde_json::to_string(token).map_err(|_| invalid_params("work done progress token is invalid"))
}

fn prepare_watch_patterns(
    options: Option<&Value>,
    context: &ServerRequestContext<'_>,
) -> Result<Vec<WatchPattern>, JsonRpcError> {
    let watchers = options
        .and_then(|options| options.get("watchers"))
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_params("watched files registration requires watchers"))?;
    if watchers.is_empty() || watchers.len() > MAX_WATCH_PATTERNS {
        return Err(invalid_params("invalid watched file pattern count"));
    }
    let allowed_roots = canonical_session_roots(context)?;
    watchers
        .iter()
        .map(|watcher| prepare_watch_pattern(watcher, context.project_root, &allowed_roots))
        .collect()
}

fn canonical_session_roots(
    context: &ServerRequestContext<'_>,
) -> Result<Vec<PathBuf>, JsonRpcError> {
    let mut roots = Vec::new();
    for root in [context.project_root, context.workspace_root] {
        let canonical = fs::canonicalize(root)
            .map_err(|_| invalid_params("language server session root is unavailable"))?;
        if !roots.contains(&canonical) {
            roots.push(canonical);
        }
    }
    Ok(roots)
}

fn prepare_watch_pattern(
    watcher: &Value,
    default_root: &Path,
    allowed_roots: &[PathBuf],
) -> Result<WatchPattern, JsonRpcError> {
    let glob_pattern = watcher
        .get("globPattern")
        .ok_or_else(|| invalid_params("file watcher requires globPattern"))?;
    let (root, pattern) = match glob_pattern {
        Value::String(pattern) => (
            canonical_watch_root(default_root, allowed_roots)?,
            pattern.as_str(),
        ),
        Value::Object(pattern) => {
            let base_uri = pattern
                .get("baseUri")
                .and_then(|base| {
                    base.as_str()
                        .or_else(|| base.get("uri").and_then(Value::as_str))
                })
                .ok_or_else(|| invalid_params("relative glob pattern requires baseUri"))?;
            let base_path = file_uri_path(base_uri)?;
            let pattern = pattern
                .get("pattern")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid_params("relative glob pattern requires pattern"))?;
            (canonical_watch_root(&base_path, allowed_roots)?, pattern)
        }
        _ => {
            return Err(invalid_params(
                "globPattern must be a string or relative pattern",
            ));
        }
    };
    validate_glob_pattern(pattern)?;
    let matcher = GlobBuilder::new(pattern)
        .literal_separator(true)
        .backslash_escape(false)
        .build()
        .map_err(|_| invalid_params("globPattern is malformed"))?
        .compile_matcher();
    let kind = watcher.get("kind").and_then(Value::as_u64).unwrap_or(7);
    if kind == 0 || kind > 7 {
        return Err(invalid_params(
            "file watcher kind must use create/change/delete bits",
        ));
    }
    Ok(WatchPattern {
        root,
        matcher,
        kind: kind as u8,
    })
}

fn validate_glob_pattern(pattern: &str) -> Result<(), JsonRpcError> {
    if pattern.is_empty()
        || pattern.len() > MAX_GLOB_PATTERN_BYTES
        || pattern.starts_with('/')
        || pattern.contains('\0')
        || pattern.split('/').any(|component| component == "..")
    {
        return Err(invalid_params(
            "globPattern must be a bounded relative pattern",
        ));
    }
    Ok(())
}

fn canonical_watch_root(root: &Path, allowed_roots: &[PathBuf]) -> Result<PathBuf, JsonRpcError> {
    let root = fs::canonicalize(root)
        .map_err(|_| invalid_params("watched file base directory does not exist"))?;
    if !root.is_dir()
        || !allowed_roots
            .iter()
            .any(|allowed| root.starts_with(allowed))
    {
        return Err(invalid_params(
            "watched file base directory is outside the session roots",
        ));
    }
    Ok(root)
}

fn file_uri_path(uri: &str) -> Result<PathBuf, JsonRpcError> {
    decode_file_uri(uri)
        .ok_or_else(|| invalid_params("watched file baseUri must be a strict absolute file URI"))
}

impl WatchedFilesRegistration {
    fn start(writer: MessageWriter, patterns: Vec<WatchPattern>) -> notify::Result<Self> {
        let (events, received) = mpsc::sync_channel(WATCH_EVENT_QUEUE_CAPACITY);
        let dirty = Arc::new(AtomicBool::new(false));
        let callback_dirty = Arc::clone(&dirty);
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
            queue_watched_file_event(&events, &callback_dirty, event);
        })?;
        let roots = patterns
            .iter()
            .map(|pattern| pattern.root.clone())
            .collect::<HashSet<_>>();
        for root in roots {
            watcher.watch(&root, RecursiveMode::Recursive)?;
        }
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stopped = Arc::clone(&stopped);
        let worker = thread::Builder::new()
            .name("kosmos-lsp-watched-files".to_owned())
            .spawn(move || watched_file_worker(writer, patterns, received, worker_stopped, dirty))
            .map_err(notify::Error::io)?;
        Ok(Self {
            watcher: Some(watcher),
            stopped,
            worker: Some(worker),
        })
    }
}

fn queue_watched_file_event(
    events: &mpsc::SyncSender<notify::Result<Event>>,
    dirty: &AtomicBool,
    event: notify::Result<Event>,
) {
    if event.is_err() {
        dirty.store(true, Ordering::Release);
    }
    if matches!(events.try_send(event), Err(mpsc::TrySendError::Full(_))) {
        dirty.store(true, Ordering::Release);
    }
}

fn watched_file_worker(
    writer: MessageWriter,
    patterns: Vec<WatchPattern>,
    events: mpsc::Receiver<notify::Result<Event>>,
    stopped: Arc<AtomicBool>,
    dirty: Arc<AtomicBool>,
) {
    let mut pending = BTreeMap::new();
    let mut deadline = None;
    while !stopped.load(Ordering::Acquire) {
        if dirty.swap(false, Ordering::AcqRel) {
            for (path, kind) in watched_file_resync(&patterns) {
                pending
                    .entry(path)
                    .and_modify(|previous| *previous = merge_watched_file_kinds(*previous, kind))
                    .or_insert(kind);
            }
            deadline = Some(Instant::now() + WATCHED_FILE_DEBOUNCE);
        }
        let wait = deadline
            .map(|deadline: Instant| deadline.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_millis(50));
        match events.recv_timeout(wait) {
            Ok(Ok(event)) => {
                for change in watched_file_changes(&event, &patterns) {
                    pending
                        .entry(change.path)
                        .and_modify(|kind| *kind = merge_watched_file_kinds(*kind, change.kind))
                        .or_insert(change.kind);
                }
                if !pending.is_empty() && deadline.is_none() {
                    deadline = Some(Instant::now() + WATCHED_FILE_DEBOUNCE);
                }
            }
            Ok(Err(_)) => {
                dirty.store(true, Ordering::Release);
            }
            Err(mpsc::RecvTimeoutError::Timeout) if deadline.is_some() => {
                if send_watched_file_changes(&writer, std::mem::take(&mut pending)).is_err() {
                    break;
                }
                deadline = None;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn watched_file_resync(patterns: &[WatchPattern]) -> BTreeMap<PathBuf, u8> {
    let roots = patterns
        .iter()
        .map(|pattern| pattern.root.clone())
        .collect::<HashSet<_>>();
    let mut changes = roots
        .iter()
        .cloned()
        .map(|root| (root, 2))
        .collect::<BTreeMap<_, _>>();
    let mut pending = roots.into_iter().collect::<Vec<_>>();

    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() && !file_type.is_symlink() {
                pending.push(path.clone());
            }
            if patterns.iter().any(|pattern| {
                pattern.kind & 2 != 0
                    && secure_relative_path(&path, &pattern.root)
                        .is_some_and(|relative| pattern.matcher.is_match(relative))
            }) {
                changes.insert(path, 2);
                if changes.len() >= MAX_WATCHED_FILE_RESYNC_FILES {
                    return patterns
                        .iter()
                        .map(|pattern| (pattern.root.clone(), 2))
                        .collect();
                }
            }
        }
    }
    changes
}

fn watched_file_changes(event: &Event, patterns: &[WatchPattern]) -> Vec<WatchedFileChange> {
    event_path_kinds(event)
        .into_iter()
        .filter(|(path, kind)| {
            patterns.iter().any(|pattern| {
                pattern.kind & *kind != 0
                    && secure_relative_path(path, &pattern.root)
                        .is_some_and(|relative| pattern.matcher.is_match(relative))
            })
        })
        .map(|(path, kind)| WatchedFileChange {
            path: path.to_path_buf(),
            kind,
        })
        .collect()
}

fn event_path_kinds(event: &Event) -> Vec<(&Path, u8)> {
    match &event.kind {
        EventKind::Create(_) => event.paths.iter().map(|path| (path.as_path(), 1)).collect(),
        EventKind::Remove(_) => event.paths.iter().map(|path| (path.as_path(), 4)).collect(),
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if event.paths.len() >= 2 => {
            vec![(event.paths[0].as_path(), 4), (event.paths[1].as_path(), 1)]
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
            event.paths.iter().map(|path| (path.as_path(), 4)).collect()
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
            event.paths.iter().map(|path| (path.as_path(), 1)).collect()
        }
        EventKind::Modify(_) | EventKind::Other => {
            event.paths.iter().map(|path| (path.as_path(), 2)).collect()
        }
        _ => Vec::new(),
    }
}

fn secure_relative_path<'a>(path: &'a Path, root: &Path) -> Option<&'a Path> {
    if !path.is_absolute() || !path.starts_with(root) {
        return None;
    }
    let mut existing = path;
    while !existing.exists() {
        existing = existing.parent()?;
    }
    let canonical = fs::canonicalize(existing).ok()?;
    if !canonical.starts_with(root) {
        return None;
    }
    path.strip_prefix(root).ok()
}

fn merge_watched_file_kinds(previous: u8, next: u8) -> u8 {
    match (previous, next) {
        (1, 2) => 1,
        (4, 1) => 2,
        (_, 4) => 4,
        _ => next,
    }
}

fn send_watched_file_changes(
    writer: &MessageWriter,
    changes: BTreeMap<PathBuf, u8>,
) -> Result<(), LanguageServerError> {
    if changes.is_empty() {
        return Ok(());
    }
    send_json(
        writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "workspace/didChangeWatchedFiles",
            "params": {
                "changes": changes
                    .into_iter()
                    .map(|(path, kind)| json!({ "uri": file_uri(&path), "type": watched_file_lsp_kind(kind) }))
                    .collect::<Vec<_>>()
            }
        }),
    )
}

fn watched_file_lsp_kind(kind: u8) -> u8 {
    match kind {
        1 => 1,
        2 => 2,
        4 => 3,
        _ => unreachable!("watched file kind is validated"),
    }
}

fn invalid_params(message: impl Into<String>) -> JsonRpcError {
    JsonRpcError {
        code: -32602,
        message: message.into(),
    }
}

fn parse_text_document_sync(initialize_result: &Value) -> TextDocumentSyncKind {
    let sync = initialize_result.pointer("/capabilities/textDocumentSync");
    let change = sync.and_then(|sync| {
        sync.as_u64()
            .or_else(|| sync.get("change").and_then(Value::as_u64))
    });
    if change == Some(2) {
        TextDocumentSyncKind::Incremental
    } else {
        TextDocumentSyncKind::Full
    }
}

fn parse_text_document_save(initialize_result: &Value) -> Option<bool> {
    match initialize_result.pointer("/capabilities/textDocumentSync/save") {
        Some(Value::Bool(true)) => Some(false),
        Some(Value::Object(options)) => Some(
            options
                .get("includeText")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        ),
        _ => None,
    }
}

fn parse_static_capabilities(initialize_result: &Value) -> HashSet<String> {
    let mut capabilities = [
        ("hoverProvider", "textDocument/hover"),
        ("completionProvider", "textDocument/completion"),
        ("signatureHelpProvider", "textDocument/signatureHelp"),
        ("definitionProvider", "textDocument/definition"),
        ("declarationProvider", "textDocument/declaration"),
        ("typeDefinitionProvider", "textDocument/typeDefinition"),
        ("implementationProvider", "textDocument/implementation"),
        ("referencesProvider", "textDocument/references"),
        ("documentSymbolProvider", "textDocument/documentSymbol"),
        ("colorProvider", "textDocument/documentColor"),
        ("documentFormattingProvider", "textDocument/formatting"),
        ("renameProvider", "textDocument/rename"),
        ("codeActionProvider", "textDocument/codeAction"),
        ("executeCommandProvider", "workspace/executeCommand"),
        ("workspaceSymbolProvider", "workspace/symbol"),
    ]
    .into_iter()
    .filter(|(capability, _)| {
        matches!(
            initialize_result.pointer(&format!("/capabilities/{capability}")),
            Some(Value::Bool(true) | Value::Object(_))
        )
    })
    .map(|(_, method)| method.to_owned())
    .collect::<HashSet<_>>();
    if initialize_result
        .pointer("/capabilities/renameProvider/prepareProvider")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        capabilities.insert("textDocument/prepareRename".to_owned());
    }
    capabilities
}

fn parse_code_action(
    value: &Value,
    server_id: &str,
    resolve_supported: bool,
) -> Option<LanguageServerCodeAction> {
    let title = value.get("title")?.as_str()?.to_owned();
    if title.is_empty() || title.len() > 4 * 1024 {
        return None;
    }
    Some(LanguageServerCodeAction {
        action_id: 0,
        server_id: server_id.to_owned(),
        title,
        kind: value.get("kind").and_then(Value::as_str).map(str::to_owned),
        is_preferred: value
            .get("isPreferred")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        disabled_reason: value
            .pointer("/disabled/reason")
            .and_then(Value::as_str)
            .map(str::to_owned),
        resolve_supported,
        command_authorization: None,
        raw: value.clone(),
    })
}

fn code_action_command(value: &Value) -> Option<(&str, &[Value])> {
    let command = if value.get("command")?.is_string() {
        value
    } else {
        value.get("command")?
    };
    Some((
        command.get("command")?.as_str()?,
        command
            .get("arguments")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default(),
    ))
}

fn authorize_code_action_command(
    value: &Value,
    version: i64,
) -> Result<Option<AuthorizedCommand>, LanguageServerError> {
    let Some((command, arguments)) = code_action_command(value) else {
        return Ok(None);
    };
    Ok(Some(AuthorizedCommand {
        token: random_token().map_err(LanguageServerError::Io)?,
        command: command.to_owned(),
        arguments: arguments.to_vec(),
        version,
    }))
}

fn take_authorized_command(
    command: &mut Option<AuthorizedCommand>,
    token: &str,
) -> Result<AuthorizedCommand, LanguageServerError> {
    if !command
        .as_ref()
        .is_some_and(|command| command.token == token)
    {
        return Err(LanguageServerError::InvalidDocument(
            "execute-command authorization is invalid, stale, or already used".to_owned(),
        ));
    }
    command.take().ok_or(LanguageServerError::ContentModified)
}

fn mark_code_action_resolving(resolved: &mut bool) -> Result<(), LanguageServerError> {
    if *resolved {
        return Err(LanguageServerError::InvalidDocument(
            "code action was already resolved".to_owned(),
        ));
    }
    *resolved = true;
    Ok(())
}

fn parse_signature_help(
    value: Value,
) -> Result<Option<LanguageServerSignatureHelp>, LanguageServerError> {
    if value.is_null() {
        return Ok(None);
    }
    let signatures = value
        .get("signatures")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            LanguageServerError::Protocol(
                "signature help response must contain signatures".to_owned(),
            )
        })?
        .iter()
        .filter_map(parse_signature_information)
        .collect::<Vec<_>>();
    Ok(Some(LanguageServerSignatureHelp {
        signatures,
        active_signature: value
            .get("activeSignature")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        active_parameter: value
            .get("activeParameter")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
    }))
}

fn parse_signature_information(value: &Value) -> Option<LanguageServerSignatureInformation> {
    Some(LanguageServerSignatureInformation {
        label: value.get("label")?.as_str()?.to_owned(),
        documentation: value
            .get("documentation")
            .and_then(parse_completion_documentation),
        parameters: value
            .get("parameters")
            .and_then(Value::as_array)
            .map(|parameters| {
                parameters
                    .iter()
                    .filter_map(parse_parameter_information)
                    .collect()
            })
            .unwrap_or_default(),
        active_parameter: value
            .get("activeParameter")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
    })
}

fn parse_parameter_information(value: &Value) -> Option<LanguageServerParameterInformation> {
    let label = match value.get("label")? {
        Value::String(label) => LanguageServerParameterLabel::Text(label.clone()),
        Value::Array(offsets) if offsets.len() == 2 => LanguageServerParameterLabel::Utf16Offsets(
            u32::try_from(offsets[0].as_u64()?).ok()?,
            u32::try_from(offsets[1].as_u64()?).ok()?,
        ),
        _ => return None,
    };
    Some(LanguageServerParameterInformation {
        label,
        documentation: value
            .get("documentation")
            .and_then(parse_completion_documentation),
    })
}

fn parse_locations(
    value: &Value,
    roots: &[WorkspaceRoot],
) -> Result<Vec<LanguageServerLocation>, LanguageServerError> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    let values = value
        .as_array()
        .map_or_else(|| vec![value], |values| values.iter().collect());
    Ok(values
        .into_iter()
        .filter_map(|value| parse_location(value, roots))
        .collect())
}

fn parse_location(value: &Value, roots: &[WorkspaceRoot]) -> Option<LanguageServerLocation> {
    let (uri, range, selection_range) = if let Some(uri) = value.get("targetUri") {
        (
            uri.as_str()?,
            value.get("targetRange").and_then(parse_range)?,
            value.get("targetSelectionRange").and_then(parse_range)?,
        )
    } else {
        let range = value.get("range").and_then(parse_range)?;
        (value.get("uri")?.as_str()?, range, range)
    };
    let (workspace_id, path) = workspace_path_from_uri(uri, roots)?;
    Some(LanguageServerLocation {
        workspace_id,
        path,
        range,
        selection_range,
    })
}

fn parse_document_symbols(
    value: &Value,
    workspace_id: WorkspaceId,
    path: &str,
    roots: &[WorkspaceRoot],
) -> Result<Vec<LanguageServerDocumentSymbol>, LanguageServerError> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    let values = value.as_array().ok_or_else(|| {
        LanguageServerError::Protocol(
            "document symbol response must be an array or null".to_owned(),
        )
    })?;
    Ok(values
        .iter()
        .filter_map(|symbol| {
            if symbol.get("location").is_some() {
                parse_symbol_information(symbol, workspace_id, path, roots)
            } else {
                parse_document_symbol(symbol)
            }
        })
        .collect())
}

fn parse_document_symbol(value: &Value) -> Option<LanguageServerDocumentSymbol> {
    Some(LanguageServerDocumentSymbol {
        name: value.get("name")?.as_str()?.to_owned(),
        detail: value
            .get("detail")
            .and_then(Value::as_str)
            .map(str::to_owned),
        kind: u32::try_from(value.get("kind")?.as_u64()?).ok()?,
        deprecated: symbol_is_deprecated(value),
        range: value.get("range").and_then(parse_range)?,
        selection_range: value.get("selectionRange").and_then(parse_range)?,
        children: value
            .get("children")
            .and_then(Value::as_array)
            .map(|children| children.iter().filter_map(parse_document_symbol).collect())
            .unwrap_or_default(),
    })
}

fn parse_symbol_information(
    value: &Value,
    workspace_id: WorkspaceId,
    path: &str,
    roots: &[WorkspaceRoot],
) -> Option<LanguageServerDocumentSymbol> {
    let location = parse_location(value.get("location")?, roots)?;
    if location.workspace_id != workspace_id || location.path != path {
        return None;
    }
    Some(LanguageServerDocumentSymbol {
        name: value.get("name")?.as_str()?.to_owned(),
        detail: value
            .get("containerName")
            .and_then(Value::as_str)
            .map(str::to_owned),
        kind: u32::try_from(value.get("kind")?.as_u64()?).ok()?,
        deprecated: symbol_is_deprecated(value),
        range: location.range,
        selection_range: location.selection_range,
        children: Vec::new(),
    })
}

fn parse_workspace_symbols(
    value: &Value,
    server_id: &str,
    workspace_id: WorkspaceId,
    resolve_supported: bool,
    roots: &[WorkspaceRoot],
) -> Result<Vec<LanguageServerWorkspaceSymbol>, LanguageServerError> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    let values = value.as_array().ok_or_else(|| {
        LanguageServerError::Protocol(
            "workspace symbol response must be an array or null".to_owned(),
        )
    })?;
    Ok(values
        .iter()
        .filter_map(|symbol| {
            parse_workspace_symbol(symbol, server_id, workspace_id, resolve_supported, roots)
        })
        .collect())
}

fn parse_workspace_symbol(
    value: &Value,
    server_id: &str,
    workspace_id: WorkspaceId,
    resolve_supported: bool,
    roots: &[WorkspaceRoot],
) -> Option<LanguageServerWorkspaceSymbol> {
    let location = value.get("location").and_then(|location| {
        if location.get("range").is_some() {
            parse_location(location, roots)
        } else {
            None
        }
    });
    let unresolved_uri_is_allowed = value
        .pointer("/location/uri")
        .and_then(Value::as_str)
        .is_some_and(|uri| workspace_path_from_uri(uri, roots).is_some());
    if location.is_none() && (!resolve_supported || !unresolved_uri_is_allowed) {
        return None;
    }
    Some(LanguageServerWorkspaceSymbol {
        server_id: server_id.to_owned(),
        workspace_id,
        name: value.get("name")?.as_str()?.to_owned(),
        kind: u32::try_from(value.get("kind")?.as_u64()?).ok()?,
        container_name: value
            .get("containerName")
            .and_then(Value::as_str)
            .map(str::to_owned),
        deprecated: symbol_is_deprecated(value),
        location,
        raw: value.clone(),
        resolve_supported,
    })
}

fn symbol_is_deprecated(value: &Value) -> bool {
    value
        .get("deprecated")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || value
            .get("tags")
            .and_then(Value::as_array)
            .is_some_and(|tags| tags.iter().any(|tag| tag.as_u64() == Some(1)))
}

fn workspace_path_from_uri(uri: &str, roots: &[WorkspaceRoot]) -> Option<(WorkspaceId, String)> {
    let path = decode_file_uri(uri)?;
    let path = fs::canonicalize(path).ok()?;
    if !path.is_file() {
        return None;
    }
    roots.iter().find_map(|root| {
        let relative = path.strip_prefix(&root.path).ok()?;
        let relative = relative.to_str()?;
        (!relative.is_empty()).then(|| (root.workspace_id, relative.to_owned()))
    })
}

fn decode_file_uri(uri: &str) -> Option<PathBuf> {
    let encoded = uri.strip_prefix("file://")?;
    if !encoded.starts_with('/') || encoded.contains(['?', '#', '\0']) {
        return None;
    }
    let bytes = encoded.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return None;
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    let decoded = percent_decode_str(encoded).decode_utf8().ok()?;
    if decoded.contains('\0') {
        return None;
    }
    Some(PathBuf::from(decoded.as_ref()))
}

fn parse_text_edits(value: Value) -> Result<Vec<LanguageServerTextEdit>, LanguageServerError> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    let values = value.as_array().ok_or_else(|| {
        LanguageServerError::Protocol("formatting response must be an array or null".to_owned())
    })?;
    values
        .iter()
        .map(|value| {
            let range = value.get("range").and_then(parse_range).ok_or_else(|| {
                LanguageServerError::Protocol("formatting edit omitted a valid range".to_owned())
            })?;
            let new_text = value
                .get("newText")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    LanguageServerError::Protocol(
                        "formatting edit omitted replacement text".to_owned(),
                    )
                })?;
            Ok(LanguageServerTextEdit {
                range,
                new_text: new_text.to_owned(),
            })
        })
        .collect()
}

fn parse_completion_list(
    value: Value,
    server_id: &str,
) -> Result<LanguageServerCompletionList, LanguageServerError> {
    if value.is_null() {
        return Ok(LanguageServerCompletionList {
            items: Vec::new(),
            is_incomplete: false,
        });
    }
    let (items, defaults, is_incomplete) = if let Some(items) = value.as_array() {
        (items, None, false)
    } else {
        let items = value
            .get("items")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                LanguageServerError::Protocol("invalid completion list response".to_owned())
            })?;
        (
            items,
            value.get("itemDefaults").and_then(Value::as_object),
            value
                .get("isIncomplete")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        )
    };
    Ok(LanguageServerCompletionList {
        items: items
            .iter()
            .filter_map(|item| parse_completion_item(item, defaults, server_id))
            .collect(),
        is_incomplete,
    })
}

fn parse_color_information(
    value: &Value,
    server_id: &str,
) -> Option<LanguageServerColorInformation> {
    Some(LanguageServerColorInformation {
        server_id: server_id.to_owned(),
        range: value.get("range").and_then(parse_range)?,
        color: parse_color(value.get("color")?)?,
    })
}

fn parse_color(value: &Value) -> Option<LanguageServerColor> {
    let color = LanguageServerColor {
        red: value.get("red")?.as_f64()?,
        green: value.get("green")?.as_f64()?,
        blue: value.get("blue")?.as_f64()?,
        alpha: value.get("alpha")?.as_f64()?,
    };
    [color.red, color.green, color.blue, color.alpha]
        .iter()
        .all(|component| component.is_finite() && (0.0..=1.0).contains(component))
        .then_some(color)
}

fn parse_color_presentation(value: &Value) -> Option<LanguageServerColorPresentation> {
    let label = value.get("label")?.as_str()?.to_owned();
    let text_edit = value.get("textEdit").and_then(|edit| {
        let new_text = edit.get("newText")?.as_str()?;
        parse_completion_text_edit(edit, new_text)
    });
    let additional_text_edits = value
        .get("additionalTextEdits")
        .and_then(Value::as_array)
        .map(|edits| {
            edits
                .iter()
                .filter_map(|edit| {
                    let new_text = edit.get("newText")?.as_str()?;
                    parse_completion_text_edit(edit, new_text)
                })
                .collect()
        })
        .unwrap_or_default();
    Some(LanguageServerColorPresentation {
        label,
        text_edit,
        additional_text_edits,
    })
}

fn parse_completion_item(
    value: &Value,
    defaults: Option<&serde_json::Map<String, Value>>,
    server_id: &str,
) -> Option<LanguageServerCompletionItem> {
    let object = value.as_object()?;
    let label = object.get("label")?.as_str()?.to_owned();
    let raw = completion_item_with_defaults(value, defaults);
    let raw_object = raw.as_object()?;
    let insert_text = object
        .get("textEdit")
        .and_then(|edit| edit.get("newText"))
        .or_else(|| object.get("textEditText"))
        .or_else(|| object.get("insertText"))
        .and_then(Value::as_str)
        .unwrap_or(&label)
        .to_owned();
    let text_edit = if let Some(edit) = object.get("textEdit") {
        Some(parse_completion_text_edit(edit, &insert_text)?)
    } else if let Some(edit_range) = defaults.and_then(|defaults| defaults.get("editRange")) {
        Some(parse_completion_text_edit(edit_range, &insert_text)?)
    } else {
        None
    };
    let documentation = raw_object
        .get("documentation")
        .and_then(parse_completion_documentation);
    let label_details = raw_object.get("labelDetails");
    Some(LanguageServerCompletionItem {
        server_id: server_id.to_owned(),
        label,
        label_detail: label_details
            .and_then(|details| details.get("detail"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        label_description: label_details
            .and_then(|details| details.get("description"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        kind: raw_object
            .get("kind")
            .and_then(Value::as_u64)
            .and_then(|kind| u32::try_from(kind).ok()),
        detail: raw_object
            .get("detail")
            .and_then(Value::as_str)
            .map(str::to_owned),
        documentation,
        sort_text: raw_object
            .get("sortText")
            .and_then(Value::as_str)
            .map(str::to_owned),
        filter_text: raw_object
            .get("filterText")
            .and_then(Value::as_str)
            .map(str::to_owned),
        insert_text,
        insert_text_is_snippet: raw_object.get("insertTextFormat").and_then(Value::as_u64)
            == Some(2),
        text_edit,
        additional_text_edits: raw_object
            .get("additionalTextEdits")
            .and_then(Value::as_array)
            .map(|edits| {
                edits
                    .iter()
                    .filter_map(|edit| {
                        let new_text = edit.get("newText")?.as_str()?;
                        parse_completion_text_edit(edit, new_text)
                    })
                    .collect()
            })
            .unwrap_or_default(),
        commit_characters: raw_object
            .get("commitCharacters")
            .and_then(Value::as_array)
            .map(|characters| {
                characters
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        preselect: raw_object
            .get("preselect")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        deprecated: raw_object
            .get("deprecated")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || raw_object
                .get("tags")
                .and_then(Value::as_array)
                .is_some_and(|tags| tags.iter().any(|tag| tag.as_u64() == Some(1))),
        raw: completion_resolve_payload(&raw),
    })
}

fn completion_resolve_payload(value: &Value) -> Value {
    let mut payload = serde_json::Map::new();
    if let Some(object) = value.as_object() {
        for name in ["label", "kind", "data"] {
            if let Some(field) = object.get(name) {
                payload.insert(name.to_owned(), field.clone());
            }
        }
    }
    Value::Object(payload)
}

fn completion_item_with_defaults(
    value: &Value,
    defaults: Option<&serde_json::Map<String, Value>>,
) -> Value {
    let mut value = value.clone();
    let Some(object) = value.as_object_mut() else {
        return value;
    };
    if let Some(defaults) = defaults {
        for name in [
            "commitCharacters",
            "data",
            "insertTextFormat",
            "insertTextMode",
        ] {
            if !object.contains_key(name)
                && let Some(default) = defaults.get(name)
            {
                object.insert(name.to_owned(), default.clone());
            }
        }
    }
    value
}

fn parse_completion_text_edit(
    value: &Value,
    new_text: &str,
) -> Option<LanguageServerCompletionTextEdit> {
    let (insert, replace) = if let Some(range) = value.get("range").and_then(parse_range) {
        (range, range)
    } else if value.get("start").is_some() {
        let range = parse_range(value)?;
        (range, range)
    } else {
        (
            value.get("insert").and_then(parse_range)?,
            value.get("replace").and_then(parse_range)?,
        )
    };
    Some(LanguageServerCompletionTextEdit {
        insert,
        replace,
        new_text: new_text.to_owned(),
    })
}

fn parse_completion_documentation(value: &Value) -> Option<LanguageServerHoverContent> {
    match value {
        Value::String(value) => Some(plain_content(value.clone())),
        Value::Object(object) => {
            let value = object.get("value").and_then(Value::as_str)?.to_owned();
            let kind = if object.get("kind").and_then(Value::as_str) == Some("markdown") {
                LanguageServerMarkupKind::Markdown
            } else {
                LanguageServerMarkupKind::PlainText
            };
            Some(LanguageServerHoverContent { kind, value })
        }
        _ => None,
    }
}

fn parse_hover(value: Value) -> Result<Option<LanguageServerHover>, LanguageServerError> {
    if value.is_null() {
        return Ok(None);
    }
    let contents = value
        .get("contents")
        .map(parse_hover_contents)
        .unwrap_or_default();
    if contents.is_empty() {
        return Ok(None);
    }
    let range = value.get("range").and_then(parse_range);
    Ok(Some(LanguageServerHover { contents, range }))
}

fn parse_hover_contents(value: &Value) -> Vec<LanguageServerHoverContent> {
    match value {
        Value::String(value) => vec![plain_content(value.clone())],
        Value::Array(values) => values.iter().flat_map(parse_hover_contents).collect(),
        Value::Object(object) => {
            if let Some(value) = object.get("value").and_then(Value::as_str) {
                if let Some(language) = object.get("language").and_then(Value::as_str) {
                    return vec![LanguageServerHoverContent {
                        kind: LanguageServerMarkupKind::Markdown,
                        value: format!("```{language}\n{value}\n```"),
                    }];
                }
                let kind = match object.get("kind").and_then(Value::as_str) {
                    Some("markdown") => LanguageServerMarkupKind::Markdown,
                    _ => LanguageServerMarkupKind::PlainText,
                };
                return vec![LanguageServerHoverContent {
                    kind,
                    value: value.to_owned(),
                }];
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn plain_content(value: String) -> LanguageServerHoverContent {
    LanguageServerHoverContent {
        kind: LanguageServerMarkupKind::PlainText,
        value,
    }
}

fn parse_range(value: &Value) -> Option<LanguageServerRange> {
    Some(LanguageServerRange {
        start: parse_position(value.get("start")?)?,
        end: parse_position(value.get("end")?)?,
    })
}

fn parse_position(value: &Value) -> Option<LanguageServerPosition> {
    Some(LanguageServerPosition {
        line: u32::try_from(value.get("line")?.as_u64()?).ok()?,
        character: u32::try_from(value.get("character")?.as_u64()?).ok()?,
    })
}

fn range_json(range: LanguageServerRange) -> Value {
    json!({ "start": position_json(range.start), "end": position_json(range.end) })
}

fn position_json(position: LanguageServerPosition) -> Value {
    json!({ "line": position.line, "character": position.character })
}

fn project_root(
    definition: &LanguageServerDefinition,
    workspace_root: &Path,
    absolute_path: &Path,
) -> PathBuf {
    let mut directory = absolute_path.parent();
    while let Some(candidate) = directory {
        if !candidate.starts_with(workspace_root) {
            break;
        }
        if definition
            .root_markers
            .iter()
            .any(|marker| candidate.join(marker).is_file())
        {
            return candidate.to_path_buf();
        }
        if candidate == workspace_root {
            break;
        }
        directory = candidate.parent();
    }
    workspace_root.to_path_buf()
}

fn file_uri(path: &Path) -> String {
    let encoded = percent_encode(path.as_os_str().as_bytes(), URI_PATH_ENCODE_SET);
    format!("file://{encoded}")
}

fn remove_sessions(
    sessions: &Mutex<HashMap<SessionKey, Arc<LanguageServerSession>>>,
    mut should_remove: impl FnMut(&SessionKey) -> bool,
) -> Vec<Arc<LanguageServerSession>> {
    let mut sessions = sessions.lock().unwrap_or_else(|error| error.into_inner());
    let keys = sessions
        .keys()
        .filter(|key| should_remove(key))
        .cloned()
        .collect::<Vec<_>>();
    keys.into_iter()
        .filter_map(|key| sessions.remove(&key))
        .collect()
}

fn mark_intentional(sessions: &[Arc<LanguageServerSession>]) {
    for session in sessions {
        session.mark_intentional();
    }
}

fn dispose_runtime_resources(
    documents: Vec<DocumentBinding>,
    sessions: Vec<Arc<LanguageServerSession>>,
) {
    if documents.is_empty() && sessions.is_empty() {
        return;
    }
    let _ = thread::Builder::new()
        .name("kosmos-language-server-shutdown".to_owned())
        .spawn(move || {
            for document in &documents {
                let _ = document.session.close_document(&document.uri);
            }
            drop((documents, sessions));
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_REQUEST_TIMEOUT: Duration = Duration::from_secs(1);

    #[test]
    fn json_rpc_request_errors_do_not_become_protocol_failures() {
        assert_eq!(
            client_response_result(&json!({
                "error": { "code": -32602, "message": "invalid request input" }
            })),
            Err(LanguageServerError::RequestFailed(
                "invalid request input".to_owned()
            ))
        );
        for code in [-32800, -32802] {
            assert_eq!(
                client_response_result(&json!({
                    "error": { "code": code, "message": "cancelled" }
                })),
                Err(LanguageServerError::RequestCancelled)
            );
        }
        assert_eq!(
            client_response_result(&json!({
                "error": { "code": -32801, "message": "modified" }
            })),
            Err(LanguageServerError::ContentModified)
        );
    }

    #[test]
    fn unexpected_session_termination_kills_process_with_retained_bindings() {
        let child = Arc::new(Mutex::new(
            Command::new("sh")
                .args(["-c", "sleep 30"])
                .spawn()
                .expect("test process should start"),
        ));
        let retained_binding = Arc::clone(&child);

        terminate_child(&child);

        assert!(
            retained_binding
                .lock()
                .unwrap()
                .try_wait()
                .expect("process status should be available")
                .is_some()
        );
    }

    #[test]
    fn supervisor_diagnostics_coalesce_even_when_wakeup_queue_is_full() {
        let (supervisor, commands) = mpsc::sync_channel(1);
        let pending_diagnostics = Arc::new(Mutex::new(HashMap::new()));
        let key = test_session_key("diagnostic-coalescing");
        supervisor
            .send(SupervisorCommand::Restart {
                key: key.clone(),
                epoch: 1,
            })
            .unwrap();
        let reporter = SessionExitReporter {
            alive: Arc::new(AtomicBool::new(true)),
            intentional: Arc::new(AtomicBool::new(false)),
            reported: Arc::new(AtomicBool::new(false)),
            supervisor,
            pending_diagnostics: Arc::clone(&pending_diagnostics),
            key,
            epoch: 1,
        };
        let revision = DocumentRevision {
            generation: 2,
            version: 3,
        };

        reporter.publish_diagnostics("file:///test.rs".to_owned(), None, revision, Vec::new());
        reporter.publish_diagnostics(
            "file:///test.rs".to_owned(),
            None,
            revision,
            vec![test_diagnostic("latest")],
        );

        assert!(matches!(
            commands.try_recv(),
            Ok(SupervisorCommand::Restart { .. })
        ));
        let pending = pending_diagnostics.lock().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(
            pending.values().next().unwrap().diagnostics[0].message,
            "latest"
        );
    }

    #[test]
    fn replayed_session_arcs_are_retained_until_after_document_unlock() {
        struct DropCount(Arc<AtomicU64>);
        impl Drop for DropCount {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }

        let drops = Arc::new(AtomicU64::new(0));
        let document = Mutex::new(Arc::new(DropCount(Arc::clone(&drops))));
        let mut replaced = Vec::new();
        {
            let mut document = document.lock().unwrap();
            retain_replaced_arc(
                &mut document,
                Arc::new(DropCount(Arc::new(AtomicU64::new(0)))),
                &mut replaced,
            );
            assert_eq!(drops.load(Ordering::Relaxed), 0);
        }
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        drop(replaced);
        assert_eq!(drops.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn versionless_diagnostics_are_bound_to_the_observed_document_revision() {
        let observed = DocumentRevision {
            generation: 7,
            version: 11,
        };
        assert!(diagnostic_matches_revision(7, 11, None, observed));
        assert!(!diagnostic_matches_revision(7, 12, None, observed));
        assert!(!diagnostic_matches_revision(8, 11, None, observed));

        let snapshots = Arc::new(Mutex::new(HashMap::new()));
        let message = json!({
            "params": {
                "uri": "file:///workspace/main.rs",
                "diagnostics": []
            }
        });
        store_published_diagnostics(&snapshots, &message, Some(observed))
            .expect("versionless diagnostics should retain their observed revision");
        assert_eq!(
            snapshots
                .lock()
                .unwrap()
                .get("file:///workspace/main.rs")
                .unwrap()
                .revision,
            observed
        );
    }

    #[test]
    fn file_uris_preserve_path_separators_and_encode_spaces() {
        assert_eq!(
            file_uri(Path::new("/tmp/rust project/src/lib.rs")),
            "file:///tmp/rust%20project/src/lib.rs"
        );
    }

    #[test]
    fn parses_markdown_hover_content() {
        let hover = parse_hover(json!({
            "contents": { "kind": "markdown", "value": "`usize`" },
            "range": {
                "start": { "line": 1, "character": 2 },
                "end": { "line": 1, "character": 7 }
            }
        }))
        .expect("hover should parse")
        .expect("hover should exist");

        assert_eq!(hover.contents[0].kind, LanguageServerMarkupKind::Markdown);
        assert_eq!(hover.range.expect("range should exist").start.line, 1);
    }

    #[test]
    fn parses_diagnostic_severity_and_numeric_code() {
        let diagnostic = parse_diagnostic(&json!({
            "range": {
                "start": { "line": 2, "character": 3 },
                "end": { "line": 2, "character": 8 }
            },
            "severity": 1,
            "code": 2307,
            "source": "ts",
            "message": "Cannot find module"
        }))
        .expect("diagnostic should parse");

        assert_eq!(
            diagnostic.severity,
            Some(LanguageServerDiagnosticSeverity::Error)
        );
        assert_eq!(diagnostic.code.as_deref(), Some("2307"));
        assert_eq!(diagnostic.range.start.character, 3);
    }

    #[test]
    fn parses_signature_utf16_parameter_offsets() {
        let help = parse_signature_help(json!({
            "signatures": [{
                "label": "paint(🎨, color)",
                "parameters": [{ "label": [6, 8] }, { "label": "color" }],
                "activeParameter": 0
            }],
            "activeSignature": 0,
            "activeParameter": 0
        }))
        .expect("signature help should parse")
        .expect("signature help should exist");

        assert_eq!(
            help.signatures[0].parameters[0].label,
            LanguageServerParameterLabel::Utf16Offsets(6, 8)
        );
    }

    #[test]
    fn location_links_are_normalized_and_outside_or_malformed_uris_are_omitted() {
        let workspace = TestDirectory::new("locations-workspace");
        let outside = TestDirectory::new("locations-outside");
        let target = workspace.path().join("src/naïve.rs");
        fs::create_dir(workspace.path().join("src")).unwrap();
        fs::write(&target, "fn naïve() {}\n").unwrap();
        let outside_target = outside.path().join("secret.rs");
        fs::write(&outside_target, "secret\n").unwrap();
        let roots = vec![WorkspaceRoot {
            workspace_id: WorkspaceId::new(7),
            path: fs::canonicalize(workspace.path()).unwrap(),
        }];
        let range = json!({
            "start": { "line": 0, "character": 3 },
            "end": { "line": 0, "character": 8 }
        });
        let locations = parse_locations(
            &json!([
                {
                    "targetUri": file_uri(&target),
                    "targetRange": range,
                    "targetSelectionRange": range
                },
                { "uri": file_uri(&outside_target), "range": range },
                { "uri": "file:///tmp/bad%2", "range": range },
                { "uri": "https://example.com/file.rs", "range": range }
            ]),
            &roots,
        )
        .unwrap();

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].workspace_id, WorkspaceId::new(7));
        assert_eq!(locations[0].path, "src/naïve.rs");
        assert_eq!(locations[0].selection_range.start.character, 3);
    }

    #[test]
    fn workspace_symbol_parser_preserves_server_identity_for_resolve() {
        let workspace = TestDirectory::new("workspace-symbol");
        let target = workspace.path().join("lib.rs");
        fs::write(&target, "fn item() {}\n").unwrap();
        let roots = vec![WorkspaceRoot {
            workspace_id: WorkspaceId::new(4),
            path: fs::canonicalize(workspace.path()).unwrap(),
        }];
        let symbols = parse_workspace_symbols(
            &json!([{
                "name": "item",
                "kind": 12,
                "location": { "uri": file_uri(&target) },
                "data": { "key": 9 }
            }]),
            "rust-analyzer",
            WorkspaceId::new(4),
            true,
            &roots,
        )
        .unwrap();

        assert_eq!(symbols[0].server_id, "rust-analyzer");
        assert_eq!(symbols[0].workspace_id, WorkspaceId::new(4));
        assert!(symbols[0].location.is_none());
        assert_eq!(symbols[0].raw["data"]["key"], 9);
    }

    #[test]
    fn parses_completion_list_defaults_for_tailwind_items() {
        let completion = parse_completion_list(
            json!({
                "isIncomplete": false,
                "itemDefaults": {
                    "data": { "_projectKey": "0" },
                    "editRange": {
                        "start": { "line": 0, "character": 12 },
                        "end": { "line": 0, "character": 14 }
                    }
                },
                "items": [{ "label": "flex", "kind": 21 }]
            }),
            "tailwindcss-language-server",
        )
        .expect("completion should parse");
        let item = &completion.items[0];

        assert_eq!(item.insert_text, "flex");
        assert_eq!(
            item.text_edit
                .as_ref()
                .expect("edit should exist")
                .replace
                .start
                .character,
            12
        );
        assert_eq!(
            item.raw
                .pointer("/data/_projectKey")
                .and_then(Value::as_str),
            Some("0")
        );
        assert_eq!(item.raw.get("textEdit"), None);
    }

    #[test]
    fn negotiates_full_and_incremental_document_sync() {
        assert_eq!(
            parse_text_document_sync(&json!({ "capabilities": { "textDocumentSync": 1 } })),
            TextDocumentSyncKind::Full
        );
        assert_eq!(
            parse_text_document_sync(&json!({
                "capabilities": { "textDocumentSync": { "change": 2 } }
            })),
            TextDocumentSyncKind::Incremental
        );
    }

    #[test]
    fn detects_static_read_only_capabilities() {
        let capabilities = parse_static_capabilities(&json!({
            "capabilities": {
                "documentFormattingProvider": true,
                "definitionProvider": {},
                "referencesProvider": false
            }
        }));
        assert!(capabilities.contains("textDocument/formatting"));
        assert!(capabilities.contains("textDocument/definition"));
        assert!(!capabilities.contains("textDocument/references"));
    }

    #[test]
    fn code_action_command_authorization_captures_exact_command_and_arguments() {
        let arguments = vec![json!({ "uri": "file:///workspace/a.rs" })];
        let action = json!({
            "title": "Fix",
            "command": { "title": "Fix", "command": "server.fix", "arguments": arguments }
        });
        let authorized = authorize_code_action_command(&action, 7)
            .unwrap()
            .expect("command should be authorized");
        assert_eq!(authorized.command, "server.fix");
        assert_eq!(
            authorized.arguments,
            vec![json!({ "uri": "file:///workspace/a.rs" })]
        );
        assert_eq!(authorized.token.len(), 64);
        assert_eq!(authorized.version, 7);

        let token = authorized.token.clone();
        let mut command = Some(authorized);
        assert!(take_authorized_command(&mut command, "forged").is_err());
        assert!(command.is_some());
        assert_eq!(
            take_authorized_command(&mut command, &token)
                .unwrap()
                .command,
            "server.fix"
        );
        assert!(take_authorized_command(&mut command, &token).is_err());
    }

    #[test]
    fn code_action_resolution_is_single_use() {
        let mut resolved = false;
        mark_code_action_resolving(&mut resolved).unwrap();
        assert!(mark_code_action_resolving(&mut resolved).is_err());
    }

    #[test]
    fn forged_or_expired_code_actions_are_not_accepted_for_edits() {
        let runtime = LanguageServerRuntime::new(Arc::new(WorkspaceEditTransactions::new()));
        let action = LanguageServerCodeAction {
            action_id: 999,
            server_id: "rust-analyzer".to_owned(),
            title: "Forged".to_owned(),
            kind: None,
            is_preferred: false,
            disabled_reason: None,
            resolve_supported: false,
            command_authorization: None,
            raw: json!({ "title": "Forged", "edit": { "changes": {} } }),
        };

        assert!(matches!(
            runtime.validate_code_action(&action),
            Err(LanguageServerError::InvalidDocument(_))
        ));
    }

    #[test]
    fn server_requests_return_protocol_specific_response_shapes() {
        let root = TestDirectory::new("request-responses");
        let (writer, _outbound) = test_writer();
        let capabilities = Arc::new(Mutex::new(DynamicCapabilityState::default()));
        let context = test_request_context(&writer, root.path(), root.path(), &capabilities);

        let configuration = server_request_response(
            &json!({
                "jsonrpc": "2.0",
                "id": "configuration",
                "method": "workspace/configuration",
                "params": { "items": [{ "section": "typescript" }, {}] }
            }),
            &context,
        );
        assert_eq!(configuration["id"], "configuration");
        assert_eq!(configuration["result"], json!([null, null]));
        assert!(configuration.get("error").is_none());

        let folders = server_request_response(
            &json!({ "id": 2, "method": "workspace/workspaceFolders" }),
            &context,
        );
        assert_eq!(folders["result"][0]["uri"], file_uri(root.path()));

        let progress = server_request_response(
            &json!({
                "id": 3,
                "method": "window/workDoneProgress/create",
                "params": { "token": "indexing" }
            }),
            &context,
        );
        assert_eq!(progress["result"], Value::Null);

        let apply_edit = server_request_response(
            &json!({ "id": 4, "method": "workspace/applyEdit", "params": { "edit": {} } }),
            &context,
        );
        assert_eq!(apply_edit["result"]["applied"], false);
        assert!(
            apply_edit["result"]["failureReason"]
                .as_str()
                .unwrap()
                .contains("renderer")
        );

        let unsupported = server_request_response(
            &json!({ "id": 5, "method": "workspace/unsupported" }),
            &context,
        );
        assert_eq!(unsupported["error"]["code"], -32601);
        assert!(unsupported.get("result").is_none());
    }

    #[test]
    fn register_and_unregister_update_dynamic_feature_support() {
        let root = TestDirectory::new("dynamic-capability");
        let (writer, _outbound) = test_writer();
        let capabilities = Arc::new(Mutex::new(DynamicCapabilityState::default()));
        let context = test_request_context(&writer, root.path(), root.path(), &capabilities);
        let registration = json!({
            "id": 1,
            "method": "client/registerCapability",
            "params": {
                "registrations": [{
                    "id": "formatting",
                    "method": "textDocument/formatting",
                    "registerOptions": {}
                }]
            }
        });

        assert_eq!(
            server_request_response(&registration, &context)["result"],
            Value::Null
        );
        assert!(capability_is_supported(
            &HashSet::new(),
            &capabilities.lock().unwrap(),
            "textDocument/formatting"
        ));

        let unregistration = json!({
            "id": 2,
            "method": "client/unregisterCapability",
            "params": {
                "unregisterations": [{
                    "id": "formatting",
                    "method": "textDocument/formatting"
                }]
            }
        });
        assert_eq!(
            server_request_response(&unregistration, &context)["result"],
            Value::Null
        );
        assert!(!capability_is_supported(
            &HashSet::new(),
            &capabilities.lock().unwrap(),
            "textDocument/formatting"
        ));
    }

    #[test]
    fn watched_file_patterns_reject_malformed_and_excessive_input() {
        let root = TestDirectory::new("watch-pattern-validation");
        let (writer, _outbound) = test_writer();
        let capabilities = Arc::new(Mutex::new(DynamicCapabilityState::default()));
        let context = test_request_context(&writer, root.path(), root.path(), &capabilities);

        for pattern in [json!("../outside/**"), json!("[malformed")] {
            let response =
                server_request_response(&watch_registration_request("invalid", pattern), &context);
            assert_eq!(response["error"]["code"], -32602);
            assert!(capabilities.lock().unwrap().registrations.is_empty());
        }

        let patterns = (0..=MAX_WATCH_PATTERNS)
            .map(|index| json!({ "globPattern": format!("src/{index}/**") }))
            .collect::<Vec<_>>();
        let response = server_request_response(
            &json!({
                "id": 1,
                "method": "client/registerCapability",
                "params": {
                    "registrations": [{
                        "id": "excessive",
                        "method": "workspace/didChangeWatchedFiles",
                        "registerOptions": { "watchers": patterns }
                    }]
                }
            }),
            &context,
        );
        assert_eq!(response["error"]["code"], -32602);
        assert!(capabilities.lock().unwrap().registrations.is_empty());
    }

    #[test]
    fn watched_file_roots_and_events_are_contained_without_following_symlinks() {
        let workspace = TestDirectory::new("watch-containment-workspace");
        let outside = TestDirectory::new("watch-containment-outside");
        let project = workspace.path().join("project");
        fs::create_dir(&project).unwrap();
        let (writer, _outbound) = test_writer();
        let capabilities = Arc::new(Mutex::new(DynamicCapabilityState::default()));
        let context = test_request_context(&writer, &project, workspace.path(), &capabilities);

        let outside_response = server_request_response(
            &watch_registration_request(
                "outside",
                json!({
                    "baseUri": file_uri(outside.path()),
                    "pattern": "**/*.rs"
                }),
            ),
            &context,
        );
        assert_eq!(outside_response["error"]["code"], -32602);

        let symlink = project.join("external");
        std::os::unix::fs::symlink(outside.path(), &symlink).unwrap();
        let symlink_response = server_request_response(
            &watch_registration_request(
                "symlink",
                json!({
                    "baseUri": file_uri(&symlink),
                    "pattern": "**/*.rs"
                }),
            ),
            &context,
        );
        assert_eq!(symlink_response["error"]["code"], -32602);

        let matcher = GlobBuilder::new("**/*.rs")
            .build()
            .unwrap()
            .compile_matcher();
        let patterns = vec![WatchPattern {
            root: fs::canonicalize(&project).unwrap(),
            matcher,
            kind: 7,
        }];
        let outside_file = outside.path().join("outside.rs");
        fs::write(&outside_file, "").unwrap();
        let linked_file = symlink.join("outside.rs");
        let event = Event::new(EventKind::Modify(ModifyKind::Data(
            notify::event::DataChange::Any,
        )))
        .add_path(linked_file);
        assert!(watched_file_changes(&event, &patterns).is_empty());
    }

    #[test]
    fn watched_file_worker_debounces_events_and_maps_event_kinds() {
        let root = TestDirectory::new("watch-debounce");
        let root = fs::canonicalize(root.path()).unwrap();
        let file = root.join("main.rs");
        fs::write(&file, "fn main() {}").unwrap();
        let patterns = vec![WatchPattern {
            root: root.clone(),
            matcher: GlobBuilder::new("**/*.rs")
                .build()
                .unwrap()
                .compile_matcher(),
            kind: 7,
        }];
        let (writer, outbound) = test_writer();
        let (events, received) = mpsc::sync_channel(8);
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stopped = Arc::clone(&stopped);
        let dirty = Arc::new(AtomicBool::new(false));
        let worker_patterns = patterns.clone();
        let worker = thread::spawn(move || {
            watched_file_worker(writer, worker_patterns, received, worker_stopped, dirty)
        });

        events
            .send(Ok(Event::new(EventKind::Create(
                notify::event::CreateKind::File,
            ))
            .add_path(file.clone())))
            .unwrap();
        events
            .send(Ok(Event::new(EventKind::Modify(ModifyKind::Data(
                notify::event::DataChange::Content,
            )))
            .add_path(file.clone())))
            .unwrap();

        let notification = receive_outbound(&outbound);
        assert_eq!(notification["method"], "workspace/didChangeWatchedFiles");
        assert_eq!(
            notification["params"]["changes"].as_array().unwrap().len(),
            1
        );
        assert_eq!(notification["params"]["changes"][0]["type"], 1);

        fs::remove_file(&file).unwrap();
        let removed =
            Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(file.clone());
        assert_eq!(watched_file_changes(&removed, &patterns)[0].kind, 4);
        fs::write(&file, "fn main() { println!(\"changed\"); }").unwrap();
        let changed = Event::new(EventKind::Modify(ModifyKind::Data(
            notify::event::DataChange::Content,
        )))
        .add_path(file.clone());
        assert_eq!(watched_file_changes(&changed, &patterns)[0].kind, 2);
        let renamed = root.join("renamed.rs");
        fs::write(&renamed, "").unwrap();
        let rename = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(file)
            .add_path(renamed);
        assert_eq!(
            watched_file_changes(&rename, &patterns)
                .into_iter()
                .map(|change| change.kind)
                .collect::<Vec<_>>(),
            vec![4, 1]
        );

        stopped.store(true, Ordering::Release);
        drop(events);
        worker.join().unwrap();
    }

    #[test]
    fn watched_file_dirty_signal_sends_a_bounded_full_refresh() {
        let root = TestDirectory::new("watch-resync");
        let root = fs::canonicalize(root.path()).unwrap();
        let file = root.join("main.rs");
        fs::write(&file, "fn main() {}").unwrap();
        let patterns = vec![WatchPattern {
            root: root.clone(),
            matcher: GlobBuilder::new("**/*.rs")
                .build()
                .unwrap()
                .compile_matcher(),
            kind: 7,
        }];
        let (writer, outbound) = test_writer();
        let (events, received) = mpsc::sync_channel(1);
        let stopped = Arc::new(AtomicBool::new(false));
        let dirty = Arc::new(AtomicBool::new(true));
        let worker_stopped = Arc::clone(&stopped);
        let worker = thread::spawn(move || {
            watched_file_worker(writer, patterns, received, worker_stopped, dirty)
        });

        let notification = receive_outbound(&outbound);
        let changes = notification["params"]["changes"].as_array().unwrap();
        assert!(changes.len() <= MAX_WATCHED_FILE_RESYNC_FILES);
        assert!(
            changes
                .iter()
                .any(|change| change["uri"] == file_uri(&root))
        );
        assert!(
            changes
                .iter()
                .any(|change| change["uri"] == file_uri(&file))
        );

        stopped.store(true, Ordering::Release);
        drop(events);
        worker.join().unwrap();
    }

    #[test]
    fn watched_file_overflow_and_backend_errors_mark_the_registration_dirty() {
        let (events, received) = mpsc::sync_channel(1);
        let dirty = AtomicBool::new(false);
        events
            .send(Ok(Event::new(EventKind::Other)))
            .expect("event queue should accept its first event");

        queue_watched_file_event(&events, &dirty, Ok(Event::new(EventKind::Other)));
        assert!(dirty.swap(false, Ordering::AcqRel));

        received.recv().unwrap().unwrap();
        queue_watched_file_event(
            &events,
            &dirty,
            Err(notify::Error::io(std::io::Error::other(
                "watch backend failed",
            ))),
        );
        assert!(dirty.load(Ordering::Acquire));
    }

    #[test]
    fn watched_file_unregister_stops_and_joins_the_watcher_worker() {
        let root = TestDirectory::new("watch-teardown");
        let (writer, _outbound) = test_writer();
        let capabilities = Arc::new(Mutex::new(DynamicCapabilityState::default()));
        let context = test_request_context(&writer, root.path(), root.path(), &capabilities);
        let response = server_request_response(
            &watch_registration_request("watcher", json!("**/*.rs")),
            &context,
        );
        assert_eq!(response["result"], Value::Null);
        let stopped = capabilities
            .lock()
            .unwrap()
            .registrations
            .get("watcher")
            .unwrap()
            ._watched_files
            .as_ref()
            .unwrap()
            .stopped
            .clone();

        let response = server_request_response(
            &json!({
                "id": 2,
                "method": "client/unregisterCapability",
                "params": {
                    "unregisterations": [{
                        "id": "watcher",
                        "method": "workspace/didChangeWatchedFiles"
                    }]
                }
            }),
            &context,
        );
        assert_eq!(response["result"], Value::Null);
        assert!(stopped.load(Ordering::Acquire));
        assert!(capabilities.lock().unwrap().registrations.is_empty());
    }

    #[test]
    fn detects_document_save_notification_options() {
        assert_eq!(
            parse_text_document_save(&json!({
                "capabilities": { "textDocumentSync": { "save": true } }
            })),
            Some(false)
        );
        assert_eq!(
            parse_text_document_save(&json!({
                "capabilities": {
                    "textDocumentSync": { "save": { "includeText": true } }
                }
            })),
            Some(true)
        );
        assert_eq!(
            parse_text_document_save(&json!({
                "capabilities": { "textDocumentSync": { "save": false } }
            })),
            None
        );
    }

    #[test]
    fn parses_formatting_edits_strictly() {
        let edits = parse_text_edits(json!([{
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 2 }
            },
            "newText": "  "
        }]))
        .expect("formatting edits should parse");

        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "  ");
        assert!(parse_text_edits(Value::Null).unwrap().is_empty());
        assert!(parse_text_edits(json!([{"newText": "missing range"}])).is_err());
        assert!(parse_text_edits(json!({})).is_err());
    }

    #[test]
    fn completion_filter_supports_tailwind_prefixes_and_fuzzy_matches() {
        let item = LanguageServerCompletionItem {
            server_id: "tailwindcss-language-server".to_owned(),
            label: "hover:items-center".to_owned(),
            label_detail: None,
            label_description: None,
            kind: None,
            detail: None,
            documentation: None,
            sort_text: None,
            filter_text: None,
            insert_text: "hover:items-center".to_owned(),
            insert_text_is_snippet: false,
            text_edit: None,
            additional_text_edits: Vec::new(),
            commit_characters: Vec::new(),
            preselect: false,
            deprecated: false,
            raw: Value::Null,
        };

        assert!(completion_matches(&item, "hover:items-cen"));
        assert!(completion_matches(&item, "hitcen"));
        assert!(!completion_matches(&item, "justify"));
    }

    #[test]
    fn runtime_logs_are_bounded_by_entry_count_and_bytes() {
        let logs = RuntimeLogs::default();
        for index in 0..MAX_SERVER_LOG_ENTRIES + 10 {
            logs.append(
                "test",
                LanguageServerLogKind::Stderr,
                format!("entry-{index}"),
            );
        }
        let entries = logs.entries("test");
        assert_eq!(entries.len(), MAX_SERVER_LOG_ENTRIES);
        assert_eq!(entries.first().unwrap().message, "entry-10");

        logs.append(
            "large",
            LanguageServerLogKind::Runtime,
            "x".repeat(MAX_SERVER_LOG_BYTES * 2),
        );
        let entries = logs.entries("large");
        assert_eq!(entries.len(), 1);
        assert!(entries[0].message.len() <= MAX_SERVER_LOG_ENTRY_BYTES);
    }

    #[test]
    fn restart_backoff_progresses_caps_and_resets_deterministically() {
        let mut breaker = RestartBreaker::default();
        let delays = (0..MAX_RESTART_ATTEMPTS)
            .map(|_| breaker.next_delay().expect("attempt should be allowed").0)
            .collect::<Vec<_>>();
        assert_eq!(
            delays,
            vec![
                Duration::from_millis(250),
                Duration::from_millis(500),
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(4),
            ]
        );
        assert_eq!(breaker.next_delay(), None);

        breaker.reset();
        assert_eq!(breaker.next_delay(), Some((INITIAL_RESTART_DELAY, 1)));
    }

    #[test]
    fn restart_breaker_resets_after_a_stable_running_interval() {
        let mut breaker = RestartBreaker {
            attempts: MAX_RESTART_ATTEMPTS - 1,
        };

        assert_eq!(
            breaker.next_delay_after_run(Some(Instant::now() - STABLE_RUNNING_INTERVAL)),
            Some((INITIAL_RESTART_DELAY, 1))
        );
    }

    #[test]
    fn intentional_shutdown_does_not_advance_the_restart_breaker() {
        let runtime = LanguageServerRuntime::new(Arc::new(WorkspaceEditTransactions::new()));
        let definition =
            super::super::catalog::language_server_definition("typescript-language-server")
                .expect("test server should exist");
        let key = SessionKey {
            workspace_id: WorkspaceId::new(1),
            server_id: definition.id,
            project_root: PathBuf::from("/tmp/kosmos-intentional-shutdown"),
            workspace_root: PathBuf::from("/tmp/kosmos-intentional-shutdown"),
        };
        runtime.supervision.lock().unwrap().insert(
            key.clone(),
            SessionSupervision {
                definition,
                executable: PathBuf::from("unused"),
                epoch: 7,
                restart_breaker: RestartBreaker { attempts: 2 },
                state: LanguageServerRuntimeState::Running,
                running_since: Some(Instant::now()),
            },
        );

        runtime.handle_exit(key.clone(), 7, "intentional".to_owned(), true);

        let supervision = runtime.supervision.lock().unwrap();
        let session = supervision
            .get(&key)
            .expect("supervision should remain unchanged");
        assert_eq!(session.restart_breaker.attempts, 2);
        assert_eq!(session.state, LanguageServerRuntimeState::Running);
    }

    #[test]
    fn document_replay_uses_the_latest_generation_version_and_text() {
        let replay = document_replay(
            42,
            9,
            "file:///workspace/src/main.ts",
            "typescript",
            "const latest = true;",
        );

        assert_eq!(replay.generation, 42);
        assert_eq!(replay.version, 9);
        assert_eq!(replay.language_id, "typescript");
        assert_eq!(replay.text, "const latest = true;");
    }

    #[test]
    fn cancellation_removes_pending_request_and_notifies_exactly_once() {
        let fake = FakeSession::new();
        let cancellation = LanguageServerRequestCancellation::new();
        let repeated_cancellation = cancellation.clone();
        let request = fake.spawn_request("test/cancel", TEST_REQUEST_TIMEOUT, cancellation.clone());
        let initial = fake.receive_message();
        let request_id = initial["id"].as_i64().expect("request should have an ID");

        let cancel = thread::spawn(move || cancellation.cancel());
        let notification = fake.receive_message();
        cancel.join().expect("cancellation should finish");
        repeated_cancellation.cancel();

        assert_eq!(notification["method"], "$/cancelRequest");
        assert_eq!(notification["params"]["id"], request_id);
        assert_eq!(
            request.join().expect("request should finish"),
            Err(LanguageServerError::RequestCancelled)
        );
        assert!(fake.pending.lock().unwrap().is_empty());
        assert!(
            fake.outbound
                .recv_timeout(Duration::from_millis(25))
                .is_err()
        );
    }

    #[test]
    fn response_winning_cancellation_race_does_not_send_cancel_notification() {
        let fake = FakeSession::new();
        let cancellation = LanguageServerRequestCancellation::new();
        let request = fake.spawn_request("test/race", TEST_REQUEST_TIMEOUT, cancellation.clone());
        let initial = fake.receive_message();
        let request_id = initial["id"].as_i64().expect("request should have an ID");
        fake.respond(request_id, json!({ "winner": "response" }));

        assert_eq!(
            request.join().expect("request should finish"),
            Ok(json!({ "winner": "response" }))
        );
        cancellation.cancel();
        assert!(
            fake.outbound
                .recv_timeout(Duration::from_millis(25))
                .is_err()
        );
    }

    #[test]
    fn cancelling_one_request_does_not_affect_an_unrelated_request() {
        let fake = FakeSession::new();
        let first_cancellation = LanguageServerRequestCancellation::new();
        let second_cancellation = LanguageServerRequestCancellation::new();
        let first = fake.spawn_request(
            "test/first",
            TEST_REQUEST_TIMEOUT,
            first_cancellation.clone(),
        );
        let second = fake.spawn_request("test/second", TEST_REQUEST_TIMEOUT, second_cancellation);
        let messages = [fake.receive_message(), fake.receive_message()];
        let first_id = messages
            .iter()
            .find(|message| message["method"] == "test/first")
            .and_then(|message| message["id"].as_i64())
            .expect("first request should have an ID");
        let second_id = messages
            .iter()
            .find(|message| message["method"] == "test/second")
            .and_then(|message| message["id"].as_i64())
            .expect("second request should have an ID");

        let cancel = thread::spawn(move || first_cancellation.cancel());
        let notification = fake.receive_message();
        cancel.join().expect("cancellation should finish");
        assert_eq!(notification["params"]["id"], first_id);
        fake.respond(second_id, json!("still active"));

        assert_eq!(
            first.join().expect("first request should finish"),
            Err(LanguageServerError::RequestCancelled)
        );
        assert_eq!(
            second.join().expect("second request should finish"),
            Ok(json!("still active"))
        );
    }

    #[test]
    fn timeout_removes_pending_request_and_sends_cancel_fallback() {
        let fake = FakeSession::new();
        let request = fake.spawn_request(
            "test/timeout",
            Duration::from_millis(25),
            LanguageServerRequestCancellation::new(),
        );
        let initial = fake.receive_message();
        let request_id = initial["id"].as_i64().expect("request should have an ID");
        let notification = fake.receive_message();

        assert_eq!(notification["method"], "$/cancelRequest");
        assert_eq!(notification["params"]["id"], request_id);
        assert_eq!(
            request.join().expect("request should finish"),
            Err(LanguageServerError::RequestTimeout)
        );
        assert!(fake.pending.lock().unwrap().is_empty());
        assert!(
            fake.outbound
                .recv_timeout(Duration::from_millis(25))
                .is_err()
        );
    }

    fn test_writer() -> (MessageWriter, mpsc::Receiver<OutboundMessage>) {
        let (sender, outbound) = mpsc::sync_channel(16);
        (MessageWriter { sender }, outbound)
    }

    fn test_session_key(name: &str) -> SessionKey {
        let root = PathBuf::from(format!("/tmp/kosmos-{name}"));
        SessionKey {
            workspace_id: WorkspaceId::new(1),
            server_id: "rust-analyzer",
            project_root: root.clone(),
            workspace_root: root,
        }
    }

    fn test_diagnostic(message: &str) -> LanguageServerDiagnostic {
        LanguageServerDiagnostic {
            range: LanguageServerRange {
                start: LanguageServerPosition {
                    line: 0,
                    character: 0,
                },
                end: LanguageServerPosition {
                    line: 0,
                    character: 1,
                },
            },
            severity: None,
            message: message.to_owned(),
            source: None,
            code: None,
        }
    }

    fn receive_outbound(outbound: &mpsc::Receiver<OutboundMessage>) -> Value {
        let message = outbound
            .recv_timeout(Duration::from_secs(1))
            .expect("outbound message should arrive");
        let value =
            serde_json::from_slice(&message.body).expect("outbound message should contain JSON");
        message
            .completion
            .send(Ok(()))
            .expect("writer completion should be received");
        value
    }

    fn test_request_context<'a>(
        writer: &'a MessageWriter,
        project_root: &'a Path,
        workspace_root: &'a Path,
        capabilities: &'a Arc<Mutex<DynamicCapabilityState>>,
    ) -> ServerRequestContext<'a> {
        ServerRequestContext {
            writer,
            project_root,
            workspace_root,
            dynamic_capabilities: capabilities,
            workspace_id: WorkspaceId::new(1),
            documents: test_request_documents(),
            workspace_edits: test_workspace_edits(),
            events: test_events(),
        }
    }

    fn test_request_documents() -> &'static Arc<Mutex<HashMap<DocumentKey, DocumentBinding>>> {
        static DOCUMENTS: std::sync::OnceLock<Arc<Mutex<HashMap<DocumentKey, DocumentBinding>>>> =
            std::sync::OnceLock::new();
        DOCUMENTS.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
    }

    fn test_workspace_edits() -> &'static Arc<WorkspaceEditTransactions> {
        static EDITS: std::sync::OnceLock<Arc<WorkspaceEditTransactions>> =
            std::sync::OnceLock::new();
        EDITS.get_or_init(|| Arc::new(WorkspaceEditTransactions::new()))
    }

    fn test_events() -> &'static CoreEventDispatcher {
        static EVENTS: std::sync::OnceLock<CoreEventDispatcher> = std::sync::OnceLock::new();
        EVENTS.get_or_init(CoreEventDispatcher::default)
    }

    fn watch_registration_request(id: &str, glob_pattern: Value) -> Value {
        json!({
            "id": 1,
            "method": "client/registerCapability",
            "params": {
                "registrations": [{
                    "id": id,
                    "method": "workspace/didChangeWatchedFiles",
                    "registerOptions": {
                        "watchers": [{ "globPattern": glob_pattern }]
                    }
                }]
            }
        })
    }

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(name: &str) -> Self {
            static NEXT_ID: AtomicI64 = AtomicI64::new(1);
            let path = std::env::temp_dir().join(format!(
                "kosmos-lsp-{name}-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).expect("test directory should be created");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct FakeSession {
        writer: MessageWriter,
        outbound: mpsc::Receiver<OutboundMessage>,
        pending: PendingRequests,
        next_request_id: Arc<AtomicI64>,
        alive: Arc<AtomicBool>,
    }

    impl FakeSession {
        fn new() -> Self {
            let (sender, outbound) = mpsc::sync_channel(16);
            Self {
                writer: MessageWriter { sender },
                outbound,
                pending: Arc::new(Mutex::new(HashMap::new())),
                next_request_id: Arc::new(AtomicI64::new(1)),
                alive: Arc::new(AtomicBool::new(true)),
            }
        }

        fn spawn_request(
            &self,
            method: &'static str,
            timeout: Duration,
            cancellation: LanguageServerRequestCancellation,
        ) -> thread::JoinHandle<Result<Value, LanguageServerError>> {
            let writer = self.writer.clone();
            let pending = Arc::clone(&self.pending);
            let next_request_id = Arc::clone(&self.next_request_id);
            let alive = Arc::clone(&self.alive);
            thread::spawn(move || {
                request_with_transport(
                    &writer,
                    &pending,
                    &next_request_id,
                    &alive,
                    method,
                    Value::Null,
                    timeout,
                    &cancellation,
                )
            })
        }

        fn receive_message(&self) -> Value {
            let message = self
                .outbound
                .recv_timeout(Duration::from_secs(1))
                .expect("outbound message should arrive");
            let value = serde_json::from_slice(&message.body)
                .expect("outbound message should contain JSON");
            message
                .completion
                .send(Ok(()))
                .expect("writer completion should be received");
            value
        }

        fn respond(&self, request_id: i64, value: Value) {
            let sender = self
                .pending
                .lock()
                .unwrap()
                .remove(&request_id)
                .expect("request should still be pending");
            sender
                .send(Ok(value))
                .expect("request response should be received");
        }
    }
}
