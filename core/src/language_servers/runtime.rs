use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_encode};
use serde_json::{Value, json};

use crate::tree::WorkspaceId;

use super::catalog::{LanguageServerDefinition, language_server_catalog};
use super::edits::validate_text_edits;
use super::{
    LanguageServerChange, LanguageServerColor, LanguageServerColorInformation,
    LanguageServerColorPresentation, LanguageServerColorPresentationRequest,
    LanguageServerCompletionItem, LanguageServerCompletionList, LanguageServerCompletionRequest,
    LanguageServerCompletionTextEdit, LanguageServerDiagnostic, LanguageServerDiagnosticSeverity,
    LanguageServerDocumentOpen, LanguageServerError, LanguageServerFormattingOptions,
    LanguageServerHover, LanguageServerHoverContent, LanguageServerMarkupKind,
    LanguageServerPosition, LanguageServerRange, LanguageServerRuntimeState,
    LanguageServerRuntimeStatus, LanguageServerTextEdit,
};

const MAX_LSP_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const FEATURE_TIMEOUT: Duration = Duration::from_secs(3);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
const DIAGNOSTIC_SETTLE_DELAY: Duration = Duration::from_millis(200);
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
    diagnostics: Vec<LanguageServerDiagnostic>,
    published_at: Instant,
}

#[derive(Debug, Default)]
pub(crate) struct LanguageServerRuntime {
    sessions: Mutex<HashMap<SessionKey, Arc<LanguageServerSession>>>,
    documents: Mutex<HashMap<DocumentKey, DocumentBinding>>,
    open_workspaces: Mutex<std::collections::HashSet<WorkspaceId>>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct SessionKey {
    workspace_id: WorkspaceId,
    server_id: &'static str,
    project_root: PathBuf,
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
    text: String,
    session: Arc<LanguageServerSession>,
}

struct ActiveDocumentBinding {
    server_id: &'static str,
    session: Arc<LanguageServerSession>,
    uri: String,
}

#[derive(Debug)]
struct LanguageServerSession {
    writer: MessageWriter,
    child: Mutex<Child>,
    pending: PendingRequests,
    next_request_id: AtomicI64,
    alive: Arc<AtomicBool>,
    diagnostics: PublishedDiagnostics,
    text_document_sync: TextDocumentSyncKind,
    text_document_save: Option<bool>,
    document_formatting: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TextDocumentSyncKind {
    Full,
    Incremental,
}

impl LanguageServerRuntime {
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
        };
        let (existing_session, dead_session) = {
            let mut sessions = self
                .sessions
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if let Some(session) = sessions.get(&key) {
                if session.is_alive() {
                    (Some(Arc::clone(session)), None)
                } else {
                    let dead_session = sessions.remove(&key);
                    (None, dead_session)
                }
            } else {
                (None, None)
            }
        };
        dispose_runtime_resources(Vec::new(), dead_session.into_iter().collect());
        let session = match existing_session {
            Some(session) => session,
            None => {
                let started = Arc::new(LanguageServerSession::spawn(
                    executable,
                    definition.launch_args,
                    &project_root,
                )?);
                let mut sessions = self
                    .sessions
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                if let Some(existing) = sessions.get(&key).filter(|session| session.is_alive()) {
                    let existing = Arc::clone(existing);
                    drop(sessions);
                    dispose_runtime_resources(Vec::new(), vec![started]);
                    existing
                } else {
                    sessions.insert(key, Arc::clone(&started));
                    started
                }
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

        session.open_document(
            &uri,
            definition.protocol_language_id(document.language_id, document.relative_path),
            document.version,
            document.text,
        )?;
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
                    text: document.text.to_owned(),
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

        let mut first_error = None;
        let mut changed = Vec::new();
        for (key, session, uri) in bindings {
            match session.change_document(&uri, version, changes, text) {
                Ok(()) => changed.push(key),
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        let mut documents = self
            .documents
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for key in changed {
            if let Some(document) = documents.get_mut(&key)
                && document.generation == generation
                && document.version + 1 == version
            {
                document.version = version;
                document.text = text.to_owned();
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
                .filter_map(|key| documents.remove(&key))
                .collect::<Vec<_>>()
        };
        let mut first_error = None;
        for document in documents {
            if let Err(error) = document.session.close_document(&document.uri) {
                first_error.get_or_insert(error);
            }
        }
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
            if let Err(error) = binding.session.save_document(&binding.uri, text) {
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
    ) -> Result<Option<LanguageServerHover>, LanguageServerError> {
        let bindings = self.document_bindings(workspace_id, path, generation, version)?;
        let results = run_for_bindings(bindings, |binding| {
            binding.session.hover(&binding.uri, position)
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

    pub(crate) fn diagnostics(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
    ) -> Result<Option<Vec<LanguageServerDiagnostic>>, LanguageServerError> {
        let bindings = {
            let documents = self
                .documents
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            documents
                .iter()
                .filter(|(key, _)| key.workspace_id == workspace_id && key.path == path)
                .map(|(_, document)| {
                    if document.generation != generation || document.version != version {
                        return Err(LanguageServerError::StaleDocument);
                    }
                    Ok((Arc::clone(&document.session), document.uri.clone()))
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        if bindings.is_empty() {
            return Err(LanguageServerError::DocumentNotOpen);
        }
        let mut published = false;
        let mut diagnostics = Vec::new();
        for (session, uri) in bindings {
            if let Some(server_diagnostics) = session.diagnostics(&uri, version) {
                published = true;
                diagnostics.extend(server_diagnostics);
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
    ) -> Result<LanguageServerCompletionList, LanguageServerError> {
        let bindings = self.document_bindings(workspace_id, path, generation, version)?;
        let mut items = Vec::new();
        let mut is_incomplete = false;
        let mut succeeded = false;
        let mut first_error = None;
        let results = run_for_bindings(bindings, |binding| {
            binding
                .session
                .completion(binding.server_id, &binding.uri, request)
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
        server_id: &str,
        raw: Value,
    ) -> Result<LanguageServerCompletionItem, LanguageServerError> {
        let bindings = self.document_bindings(workspace_id, path, generation, version)?;
        let binding = bindings
            .into_iter()
            .find(|binding| binding.server_id == server_id)
            .ok_or(LanguageServerError::DocumentNotOpen)?;
        binding.session.resolve_completion(server_id, raw)
    }

    pub(crate) fn document_colors(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
    ) -> Result<Vec<LanguageServerColorInformation>, LanguageServerError> {
        let bindings = self.document_bindings(workspace_id, path, generation, version)?;
        let mut colors = Vec::new();
        let mut succeeded = false;
        let mut first_error = None;
        let results = run_for_bindings(bindings, |binding| {
            binding
                .session
                .document_colors(binding.server_id, &binding.uri)
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
    ) -> Result<Vec<LanguageServerColorPresentation>, LanguageServerError> {
        let bindings = self.document_bindings(workspace_id, path, generation, version)?;
        let binding = bindings
            .into_iter()
            .find(|binding| binding.server_id == request.server_id)
            .ok_or(LanguageServerError::DocumentNotOpen)?;
        binding
            .session
            .color_presentations(&binding.uri, request.range, request.color)
    }

    pub(crate) fn formatting(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        options: LanguageServerFormattingOptions,
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
                if !document.session.document_formatting {
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
        let edits = session.formatting(&uri, options)?;
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
                .filter_map(|key| document_map.remove(&key))
                .collect::<Vec<_>>()
        };
        let sessions = remove_sessions(&self.sessions, |key| key.server_id == server_id);
        dispose_runtime_resources(documents, sessions);
    }

    pub(crate) fn server_status(&self, server_id: &str) -> LanguageServerRuntimeStatus {
        let sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let matching = sessions
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
            .filter(|(_, session)| session.is_alive())
            .count();
        let state = match (running, session_count) {
            (_, 0) => LanguageServerRuntimeState::Inactive,
            (0, _) => LanguageServerRuntimeState::Crashed,
            (running, total) if running == total => LanguageServerRuntimeState::Running,
            _ => LanguageServerRuntimeState::Degraded,
        };
        LanguageServerRuntimeStatus {
            state,
            session_count,
            workspace_count,
        }
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
        let sessions = remove_sessions(&self.sessions, |key| {
            !workspace_ids.contains(&key.workspace_id)
        });
        drop(open_workspaces);
        dispose_runtime_resources(documents, sessions);
    }
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

impl LanguageServerSession {
    fn spawn(
        executable: &Path,
        launch_args: &[&str],
        project_root: &Path,
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
        if let Some(mut stderr) = child.stderr.take() {
            let _ = thread::Builder::new()
                .name("kosmos-language-server-stderr".to_owned())
                .spawn(move || {
                    let _ = std::io::copy(&mut stderr, &mut std::io::sink());
                });
        }

        let writer = spawn_writer(stdin)?;
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let alive = Arc::new(AtomicBool::new(true));
        let diagnostics = Arc::new(Mutex::new(HashMap::new()));
        spawn_reader(
            stdout,
            writer.clone(),
            Arc::clone(&pending),
            Arc::clone(&alive),
            Arc::clone(&diagnostics),
        )?;

        let mut session = Self {
            writer,
            child: Mutex::new(child),
            pending,
            next_request_id: AtomicI64::new(1),
            alive,
            diagnostics,
            text_document_sync: TextDocumentSyncKind::Full,
            text_document_save: None,
            document_formatting: false,
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
                        "hover": { "contentFormat": ["markdown", "plaintext"] },
                        "publishDiagnostics": { "versionSupport": true },
                        "formatting": { "dynamicRegistration": false },
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
                        }
                    },
                    "workspace": {
                        "configuration": true,
                        "workspaceFolders": true
                    }
                }
            }),
            REQUEST_TIMEOUT,
        )?;
        session.text_document_sync = parse_text_document_sync(&initialize_result);
        session.text_document_save = parse_text_document_save(&initialize_result);
        session.document_formatting = parse_document_formatting(&initialize_result);
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
    ) -> Result<Option<LanguageServerHover>, LanguageServerError> {
        let result = self.request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": uri },
                "position": position_json(position)
            }),
            FEATURE_TIMEOUT,
        )?;
        parse_hover(result)
    }

    fn completion(
        &self,
        server_id: &str,
        uri: &str,
        completion: &LanguageServerCompletionRequest,
    ) -> Result<LanguageServerCompletionList, LanguageServerError> {
        let result = self.request(
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
        )?;
        parse_completion_list(result, server_id)
    }

    fn resolve_completion(
        &self,
        server_id: &str,
        raw: Value,
    ) -> Result<LanguageServerCompletionItem, LanguageServerError> {
        let result = self.request("completionItem/resolve", raw, FEATURE_TIMEOUT)?;
        parse_completion_item(&result, None, server_id).ok_or_else(|| {
            LanguageServerError::Protocol("invalid completion item response".to_owned())
        })
    }

    fn document_colors(
        &self,
        server_id: &str,
        uri: &str,
    ) -> Result<Vec<LanguageServerColorInformation>, LanguageServerError> {
        let result = self.request(
            "textDocument/documentColor",
            json!({ "textDocument": { "uri": uri } }),
            FEATURE_TIMEOUT,
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
    ) -> Result<Vec<LanguageServerColorPresentation>, LanguageServerError> {
        let result = self.request(
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
    ) -> Result<Vec<LanguageServerTextEdit>, LanguageServerError> {
        let result = self.request(
            "textDocument/formatting",
            json!({
                "textDocument": { "uri": uri },
                "options": {
                    "tabSize": options.tab_size,
                    "insertSpaces": options.insert_spaces
                }
            }),
            FEATURE_TIMEOUT,
        )?;
        parse_text_edits(result)
    }

    fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, LanguageServerError> {
        if !self.alive.load(Ordering::Acquire) {
            return Err(LanguageServerError::ServerExited);
        }
        let id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::sync_channel(1);
        self.pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(id, sender);
        if let Err(error) = send_json(
            &self.writer,
            &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
        ) {
            self.pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&id);
            return Err(error);
        }

        match receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.pending
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .remove(&id);
                let _ = self.notify("$/cancelRequest", json!({ "id": id }));
                Err(LanguageServerError::RequestTimeout)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(LanguageServerError::ServerExited),
        }
    }

    fn notify(&self, method: &str, params: Value) -> Result<(), LanguageServerError> {
        send_json(
            &self.writer,
            &json!({ "jsonrpc": "2.0", "method": method, "params": params }),
        )
    }
}

impl Drop for LanguageServerSession {
    fn drop(&mut self) {
        if self.alive.load(Ordering::Acquire) {
            let _ = self.request("shutdown", Value::Null, SHUTDOWN_TIMEOUT);
            let _ = self.notify("exit", Value::Null);
        }
        let mut child = self.child.lock().unwrap_or_else(|error| error.into_inner());
        if child.try_wait().ok().flatten().is_none() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn spawn_reader(
    stdout: impl Read + Send + 'static,
    writer: MessageWriter,
    pending: PendingRequests,
    alive: Arc<AtomicBool>,
    diagnostics: PublishedDiagnostics,
) -> Result<(), LanguageServerError> {
    thread::Builder::new()
        .name("kosmos-language-server-reader".to_owned())
        .spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Ok(Some(message)) = read_message(&mut reader) {
                if let Some(id) = message.get("id").and_then(Value::as_i64)
                    && message.get("method").is_none()
                {
                    let sender = pending
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .remove(&id);
                    if let Some(sender) = sender {
                        let result = if let Some(error) = message.get("error") {
                            if error.get("code").and_then(Value::as_i64) == Some(-32801) {
                                Err(LanguageServerError::ContentModified)
                            } else {
                                Err(LanguageServerError::Protocol(
                                    error
                                        .get("message")
                                        .and_then(Value::as_str)
                                        .unwrap_or("language server request failed")
                                        .to_owned(),
                                ))
                            }
                        } else {
                            Ok(message.get("result").cloned().unwrap_or(Value::Null))
                        };
                        let _ = sender.send(result);
                    }
                    continue;
                }

                if message.get("method").and_then(Value::as_str)
                    == Some("textDocument/publishDiagnostics")
                {
                    store_published_diagnostics(&diagnostics, &message);
                    continue;
                }

                if message.get("method").is_some() && message.get("id").is_some() {
                    let result = server_request_result(&message);
                    let _ = send_json(
                        &writer,
                        &json!({
                            "jsonrpc": "2.0",
                            "id": message.get("id").cloned().unwrap_or(Value::Null),
                            "result": result
                        }),
                    );
                }
            }

            alive.store(false, Ordering::Release);
            let pending =
                std::mem::take(&mut *pending.lock().unwrap_or_else(|error| error.into_inner()));
            for sender in pending.into_values() {
                let _ = sender.send(Err(LanguageServerError::ServerExited));
            }
        })
        .map(|_| ())
        .map_err(|error| LanguageServerError::ServerStart(error.to_string()))
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

fn store_published_diagnostics(diagnostics: &PublishedDiagnostics, message: &Value) {
    let Some(uri) = message.pointer("/params/uri").and_then(Value::as_str) else {
        return;
    };
    let version = message.pointer("/params/version").and_then(Value::as_i64);
    let parsed = message
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
                diagnostics: parsed,
                published_at: Instant::now(),
            },
        );
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

fn server_request_result(message: &Value) -> Value {
    match message.get("method").and_then(Value::as_str) {
        Some("workspace/configuration") => {
            let count = message
                .pointer("/params/items")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            Value::Array(vec![json!({}); count])
        }
        Some("workspace/workspaceFolders") => Value::Null,
        _ => Value::Null,
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

fn parse_document_formatting(initialize_result: &Value) -> bool {
    matches!(
        initialize_result.pointer("/capabilities/documentFormattingProvider"),
        Some(Value::Bool(true) | Value::Object(_))
    )
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
    fn detects_static_document_formatting_capabilities() {
        assert!(parse_document_formatting(&json!({
            "capabilities": { "documentFormattingProvider": true }
        })));
        assert!(parse_document_formatting(&json!({
            "capabilities": { "documentFormattingProvider": {} }
        })));
        for value in [Value::Null, Value::Bool(false), json!("invalid")] {
            assert!(!parse_document_formatting(&json!({
                "capabilities": { "documentFormattingProvider": value }
            })));
        }
        assert!(!parse_document_formatting(&json!({ "capabilities": {} })));
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
}
