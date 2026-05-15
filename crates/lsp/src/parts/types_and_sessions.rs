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

