use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::thread;
use std::time::Duration;

use registry::{RegistryEntry, ToolKind};
use serde_json::{Value, json};
use url::Url;

const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(10);
const HOVER_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub struct HoverRequest {
    pub root: PathBuf,
    pub path: PathBuf,
    pub language_id: String,
    pub content: String,
    pub position: Position,
}

#[derive(Clone, Copy, Debug)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Clone, Debug)]
pub struct Hover {
    pub contents: String,
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Json(serde_json::Error),
    Url(PathBuf),
    Start {
        server: &'static str,
        message: String,
    },
    Timeout {
        server: &'static str,
        method: &'static str,
    },
    Response {
        server: &'static str,
        message: String,
    },
    AllFailed(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::Json(err) => write!(f, "json error: {err}"),
            Self::Url(path) => write!(f, "could not convert path to file URI: {}", path.display()),
            Self::Start { server, message } => write!(f, "{server} failed to start: {message}"),
            Self::Timeout { server, method } => write!(f, "{server} timed out during {method}"),
            Self::Response { server, message } => {
                write!(f, "{server} returned an error: {message}")
            }
            Self::AllFailed(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}

pub fn has_installed_server(language_id: &str) -> bool {
    registry::for_language(language_id, ToolKind::Lsp).any(installer::is_installed)
}

pub fn hover(request: HoverRequest) -> Result<Option<Hover>, Error> {
    let mut errors = Vec::new();
    let mut attempted = false;
    let mut answered = false;

    for entry in registry::for_language(&request.language_id, ToolKind::Lsp) {
        if !installer::is_installed(entry) {
            continue;
        }
        attempted = true;

        match session(entry, &request.root).and_then(|client| client.hover(entry, &request)) {
            Ok(Some(contents)) => return Ok(Some(Hover { contents })),
            Ok(None) => answered = true,
            Err(err) => errors.push(format!("{}: {err}", entry.id)),
        }
    }

    if attempted && !answered && !errors.is_empty() {
        return Err(Error::AllFailed(errors.join("; ")));
    }

    Ok(None)
}

#[derive(Clone, Eq, PartialEq, Hash)]
struct SessionKey {
    server_id: &'static str,
    root: PathBuf,
}

static SESSIONS: OnceLock<Mutex<HashMap<SessionKey, Client>>> = OnceLock::new();

fn session(entry: &'static RegistryEntry, root: &Path) -> Result<Client, Error> {
    let root = canonical_or_original(root);
    let key = SessionKey {
        server_id: entry.id,
        root: root.clone(),
    };

    if let Some(existing) = SESSIONS
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .get(&key)
        .cloned()
    {
        return Ok(existing);
    }

    let client = Client::start(entry, root)?;
    let mut sessions = SESSIONS.get_or_init(Default::default).lock().unwrap();
    Ok(sessions.entry(key).or_insert_with(|| client).clone())
}

fn canonical_or_original(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[derive(Clone)]
struct Client {
    inner: Arc<ClientInner>,
}

struct ClientInner {
    server_id: &'static str,
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    pending: Mutex<HashMap<u64, SyncSender<Value>>>,
    documents: Mutex<HashMap<String, DocumentState>>,
    next_id: AtomicU64,
}

#[derive(Clone)]
struct DocumentState {
    version: i32,
    content: String,
}

impl Drop for ClientInner {
    fn drop(&mut self) {
        let _ = self.child.lock().unwrap().kill();
    }
}

impl Client {
    fn start(entry: &'static RegistryEntry, root: PathBuf) -> Result<Self, Error> {
        let bin_path = installer::bin_path(entry).ok_or_else(|| Error::Start {
            server: entry.id,
            message: "no binary for the current platform".to_string(),
        })?;

        let mut command = Command::new(&bin_path);
        command
            .args(entry.launch.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .current_dir(&root);
        for &(key, value) in entry.launch.env {
            command.env(key, value);
        }

        let mut child = command.spawn().map_err(|err| Error::Start {
            server: entry.id,
            message: format!("{}: {err}", bin_path.display()),
        })?;
        let stdin = child.stdin.take().ok_or_else(|| Error::Start {
            server: entry.id,
            message: "missing stdin pipe".to_string(),
        })?;
        let stdout = child.stdout.take().ok_or_else(|| Error::Start {
            server: entry.id,
            message: "missing stdout pipe".to_string(),
        })?;

        let inner = Arc::new(ClientInner {
            server_id: entry.id,
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            pending: Mutex::new(HashMap::new()),
            documents: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        });

        spawn_reader(stdout, Arc::downgrade(&inner));

        let client = Self { inner };
        client.initialize(entry, &root)?;
        Ok(client)
    }

    fn initialize(&self, entry: &'static RegistryEntry, root: &Path) -> Result<(), Error> {
        let root_uri = file_uri(root)?;
        let root_path = root.to_string_lossy().to_string();
        self.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "clientInfo": {
                    "name": "Kosmos",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "rootUri": root_uri.as_str(),
                "rootPath": root_path.as_str(),
                "workspaceFolders": [{
                    "uri": root_uri.as_str(),
                    "name": root.file_name().and_then(|name| name.to_str()).unwrap_or("workspace"),
                }],
                "capabilities": {
                    "textDocument": {
                        "hover": {
                            "dynamicRegistration": false,
                            "contentFormat": ["markdown", "plaintext"],
                        },
                        "synchronization": {
                            "dynamicRegistration": false,
                            "willSave": false,
                            "willSaveWaitUntil": false,
                            "didSave": false,
                        },
                    },
                    "workspace": {
                        "configuration": false,
                        "workspaceFolders": true,
                    },
                },
            }),
            INITIALIZE_TIMEOUT,
        )?;
        self.notify("initialized", json!({}))?;

        eprintln!("kosmos: started LSP {} for {}", entry.id, root.display());
        Ok(())
    }

    fn hover(
        &self,
        entry: &'static RegistryEntry,
        request: &HoverRequest,
    ) -> Result<Option<String>, Error> {
        let uri = file_uri(&request.path)?;
        self.ensure_document(entry, request, &uri)?;

        let result = self.request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": uri },
                "position": {
                    "line": request.position.line,
                    "character": request.position.character,
                },
            }),
            HOVER_TIMEOUT,
        )?;

        Ok(hover_text(&result))
    }

    fn ensure_document(
        &self,
        entry: &'static RegistryEntry,
        request: &HoverRequest,
        uri: &str,
    ) -> Result<(), Error> {
        enum SyncAction {
            Open { version: i32 },
            Change { version: i32 },
            None,
        }

        let action = {
            let mut documents = self.inner.documents.lock().unwrap();
            match documents.get_mut(uri) {
                Some(state) if state.content == request.content => SyncAction::None,
                Some(state) => {
                    state.version += 1;
                    state.content = request.content.clone();
                    SyncAction::Change {
                        version: state.version,
                    }
                }
                None => {
                    documents.insert(
                        uri.to_string(),
                        DocumentState {
                            version: 1,
                            content: request.content.clone(),
                        },
                    );
                    SyncAction::Open { version: 1 }
                }
            }
        };

        match action {
            SyncAction::Open { version } => self.notify(
                "textDocument/didOpen",
                json!({
                    "textDocument": {
                        "uri": uri,
                        "languageId": request.language_id.as_str(),
                        "version": version,
                        "text": request.content.as_str(),
                    },
                }),
            ),
            SyncAction::Change { version } => self.notify(
                "textDocument/didChange",
                json!({
                    "textDocument": {
                        "uri": uri,
                        "version": version,
                    },
                    "contentChanges": [{ "text": request.content.as_str() }],
                }),
            ),
            SyncAction::None => Ok(()),
        }
        .map_err(|err| Error::Response {
            server: entry.id,
            message: err.to_string(),
        })
    }

    fn request(
        &self,
        method: &'static str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, Error> {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::sync_channel(1);
        self.inner.pending.lock().unwrap().insert(id, tx);

        let send_result = self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }));
        if let Err(err) = send_result {
            self.inner.pending.lock().unwrap().remove(&id);
            return Err(err);
        }

        let response = match rx.recv_timeout(timeout) {
            Ok(response) => response,
            Err(_) => {
                self.inner.pending.lock().unwrap().remove(&id);
                return Err(Error::Timeout {
                    server: self.inner.server_id,
                    method,
                });
            }
        };

        if let Some(error) = response.get("error") {
            return Err(Error::Response {
                server: self.inner.server_id,
                message: response_error_message(error),
            });
        }

        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    }

    fn notify(&self, method: &'static str, params: Value) -> Result<(), Error> {
        self.send(json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    fn send(&self, value: Value) -> Result<(), Error> {
        self.inner.send(value)
    }
}

impl ClientInner {
    fn send(&self, value: Value) -> Result<(), Error> {
        let body = serde_json::to_vec(&value)?;
        let mut stdin = self.stdin.lock().unwrap();
        write!(stdin, "Content-Length: {}\r\n\r\n", body.len())?;
        stdin.write_all(&body)?;
        stdin.flush()?;
        Ok(())
    }

    fn respond(&self, id: Value, result: Value) {
        let _ = self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        }));
    }
}

fn spawn_reader(stdout: ChildStdout, client: Weak<ClientInner>) {
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let message = match read_message(&mut reader) {
                Ok(Some(message)) => message,
                Ok(None) => break,
                Err(err) => {
                    eprintln!("kosmos: LSP read error: {err}");
                    break;
                }
            };

            let value: Value = match serde_json::from_str(&message) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("kosmos: LSP JSON parse error: {err}");
                    continue;
                }
            };

            let Some(client) = client.upgrade() else {
                break;
            };

            if let Some(method) = value.get("method").and_then(Value::as_str)
                && let Some(id) = value.get("id").cloned()
            {
                let result = match method {
                    "workspace/configuration" => json!([]),
                    _ => Value::Null,
                };
                client.respond(id, result);
                continue;
            }

            let Some(id) = value.get("id").and_then(Value::as_u64) else {
                continue;
            };
            if let Some(tx) = client.pending.lock().unwrap().remove(&id) {
                let _ = tx.send(value);
            }
        }
    });
}

fn read_message(reader: &mut BufReader<ChildStdout>) -> io::Result<Option<String>> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }

        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }

        if let Some(value) = line.strip_prefix("Content-Length:") {
            content_length = value.trim().parse::<usize>().ok();
        }
    }

    let Some(content_length) = content_length else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing Content-Length header",
        ));
    };

    let mut body = vec![0; content_length];
    reader.read_exact(&mut body)?;
    Ok(Some(String::from_utf8_lossy(&body).into_owned()))
}

fn file_uri(path: &Path) -> Result<String, Error> {
    Url::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|_| Error::Url(path.to_path_buf()))
}

fn response_error_message(error: &Value) -> String {
    error
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| error.to_string())
}

fn hover_text(result: &Value) -> Option<String> {
    if result.is_null() {
        return None;
    }

    let contents = result.get("contents")?;
    let mut parts = Vec::new();
    collect_hover_parts(contents, &mut parts);
    let text = normalize_hover_text(parts.join("\n\n"));
    (!text.is_empty()).then_some(text)
}

fn collect_hover_parts(value: &Value, parts: &mut Vec<String>) {
    match value {
        Value::String(text) => parts.push(text.clone()),
        Value::Array(items) => {
            for item in items {
                collect_hover_parts(item, parts);
            }
        }
        Value::Object(map) => {
            if let (Some(language), Some(value)) = (
                map.get("language").and_then(Value::as_str),
                map.get("value").and_then(Value::as_str),
            ) {
                parts.push(format!("```{language}\n{value}\n```"));
            } else if let Some(value) = map.get("value").and_then(Value::as_str) {
                parts.push(value.to_string());
            }
        }
        _ => {}
    }
}

fn normalize_hover_text(text: String) -> String {
    let mut text = text.replace("\r\n", "\n").replace('\r', "\n");
    while text.contains("\n\n\n") {
        text = text.replace("\n\n\n", "\n\n");
    }
    text.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_markup_content_hover() {
        let result = json!({
            "contents": {
                "kind": "markdown",
                "value": "```rust\nfn main()\n```",
            },
        });

        assert_eq!(hover_text(&result).unwrap(), "```rust\nfn main()\n```");
    }

    #[test]
    fn extracts_marked_string_arrays() {
        let result = json!({
            "contents": [
                { "language": "rust", "value": "struct Foo" },
                "docs",
            ],
        });

        assert_eq!(
            hover_text(&result).unwrap(),
            "```rust\nstruct Foo\n```\n\ndocs"
        );
    }

    #[test]
    fn ignores_empty_hover() {
        let result = json!({ "contents": [] });

        assert!(hover_text(&result).is_none());
    }
}
