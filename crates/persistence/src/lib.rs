use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use gpui::{Bounds, Point, Size, WindowBounds, px};
use serde::{Deserialize, Serialize};

use workspace::{Workspace, WorkspaceManager};

const SESSION_VERSION: u32 = 1;
const WORKSPACE_VERSION: u32 = 1;
const WINDOW_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct PersistedSession {
    version: u32,
    active: Option<usize>,
    workspace_ids: Vec<usize>,
    next_workspace_id: usize,
}

#[derive(Serialize)]
struct PersistedWorkspaceRef<'a> {
    version: u32,
    #[serde(flatten)]
    workspace: &'a Workspace,
}

#[derive(Deserialize)]
struct PersistedWorkspace {
    version: u32,
    #[serde(flatten)]
    workspace: Workspace,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
enum WindowState {
    Windowed,
    Maximized,
    Fullscreen,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
struct PersistedWindow {
    version: u32,
    state: WindowState,
    origin_x: f32,
    origin_y: f32,
    width: f32,
    height: f32,
}

impl PersistedWindow {
    fn from_bounds(bounds: WindowBounds) -> Self {
        let (state, b) = match bounds {
            WindowBounds::Windowed(b) => (WindowState::Windowed, b),
            WindowBounds::Maximized(b) => (WindowState::Maximized, b),
            WindowBounds::Fullscreen(b) => (WindowState::Fullscreen, b),
        };
        Self {
            version: WINDOW_VERSION,
            state,
            origin_x: f32::from(b.origin.x),
            origin_y: f32::from(b.origin.y),
            width: f32::from(b.size.width),
            height: f32::from(b.size.height),
        }
    }

    fn to_bounds(self) -> WindowBounds {
        let bounds = Bounds {
            origin: Point {
                x: px(self.origin_x),
                y: px(self.origin_y),
            },
            size: Size {
                width: px(self.width),
                height: px(self.height),
            },
        };
        match self.state {
            WindowState::Windowed => WindowBounds::Windowed(bounds),
            WindowState::Maximized => WindowBounds::Maximized(bounds),
            WindowState::Fullscreen => WindowBounds::Fullscreen(bounds),
        }
    }
}

pub fn load() -> WorkspaceManager {
    match try_load() {
        Some(manager) => manager,
        None => WorkspaceManager::new(),
    }
}

fn try_load() -> Option<WorkspaceManager> {
    let session: PersistedSession = read_json(&session_path()?)?;
    if session.version != SESSION_VERSION {
        return None;
    }

    let mut workspaces = Vec::with_capacity(session.workspace_ids.len());
    for id in &session.workspace_ids {
        let Some(path) = workspace_path(*id) else {
            continue;
        };
        let Some(persisted) = read_json::<PersistedWorkspace>(&path) else {
            eprintln!("kosmos: skipping workspace {id}: failed to read {}", path.display());
            continue;
        };
        if persisted.version != WORKSPACE_VERSION {
            continue;
        }
        workspaces.push(persisted.workspace);
    }

    let active = session
        .active
        .filter(|id| workspaces.iter().any(|w| w.id == *id))
        .or_else(|| workspaces.first().map(|w| w.id));

    Some(WorkspaceManager::from_parts(
        workspaces,
        active,
        session.next_workspace_id,
    ))
}

pub fn save_session(manager: &WorkspaceManager) {
    let Some(path) = session_path() else {
        return;
    };
    let session = PersistedSession {
        version: SESSION_VERSION,
        active: manager.active_id(),
        workspace_ids: manager.workspaces().iter().map(|w| w.id).collect(),
        next_workspace_id: manager.next_id(),
    };
    if let Err(err) = write_json(&path, &session) {
        eprintln!("kosmos: failed to write session: {err}");
    }
}

pub fn load_window_bounds() -> Option<WindowBounds> {
    let path = window_path()?;
    let persisted: PersistedWindow = read_json(&path)?;
    if persisted.version != WINDOW_VERSION {
        return None;
    }
    Some(persisted.to_bounds())
}

pub fn save_window_bounds(bounds: WindowBounds) {
    let Some(path) = window_path() else {
        return;
    };
    let persisted = PersistedWindow::from_bounds(bounds);
    if let Err(err) = write_json(&path, &persisted) {
        eprintln!("kosmos: failed to write window bounds: {err}");
    }
}

pub fn save_workspace(workspace: &Workspace) {
    let Some(path) = workspace_path(workspace.id) else {
        return;
    };
    let persisted = PersistedWorkspaceRef {
        version: WORKSPACE_VERSION,
        workspace,
    };
    if let Err(err) = write_json(&path, &persisted) {
        eprintln!(
            "kosmos: failed to write workspace {}: {err}",
            workspace.id
        );
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
    write_atomic(path, &bytes)
}

fn write_atomic(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let file_name = target
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing file name"))?;
    let mut tmp_name = file_name.to_owned();
    tmp_name.push(".tmp");
    let mut tmp_path = target.to_path_buf();
    tmp_path.set_file_name(tmp_name);

    fs::write(&tmp_path, bytes)?;
    fs::rename(&tmp_path, target)
}

fn kosmos_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".kosmos"))
}

fn session_path() -> Option<PathBuf> {
    Some(kosmos_dir()?.join("session.json"))
}

fn workspace_path(id: usize) -> Option<PathBuf> {
    Some(kosmos_dir()?.join("workspaces").join(format!("{id}.json")))
}

fn window_path() -> Option<PathBuf> {
    Some(kosmos_dir()?.join("window.json"))
}
