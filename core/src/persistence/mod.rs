use std::error::Error as StdError;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::State;
use crate::tree::{
    Pane, PaneId, PaneNode, SplitAxis, SplitPaneId, Tab, TabId, TabKind, Workspace, WorkspaceId,
};

pub type Result<T> = std::result::Result<T, PersistenceError>;

#[derive(Clone, Debug)]
pub struct StateStore {
    path: PathBuf,
}

impl StateStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let store = Self { path };
        store.connection()?;

        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<State> {
        let connection = self.connection()?;

        load_state(&connection)
    }

    pub fn save(&self, state: &State) -> Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;

        clear_state(&transaction)?;
        save_state(&transaction, state)?;
        transaction.commit()?;

        Ok(())
    }

    fn connection(&self) -> Result<Connection> {
        let connection = Connection::open(&self.path)?;
        migrate(&connection)?;

        Ok(connection)
    }
}

#[derive(Debug)]
pub enum PersistenceError {
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
    InvalidState(String),
}

impl fmt::Display for PersistenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Sqlite(error) => write!(formatter, "{error}"),
            Self::InvalidState(message) => formatter.write_str(message),
        }
    }
}

impl StdError for PersistenceError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Sqlite(error) => Some(error),
            Self::InvalidState(_) => None,
        }
    }
}

impl From<std::io::Error> for PersistenceError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rusqlite::Error> for PersistenceError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

fn migrate(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS metadata (
            key TEXT PRIMARY KEY NOT NULL,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS workspaces (
            id INTEGER PRIMARY KEY NOT NULL,
            position INTEGER NOT NULL UNIQUE,
            name TEXT NOT NULL,
            directory TEXT NOT NULL,
            active_pane_id INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS pane_nodes (
            workspace_id INTEGER NOT NULL,
            path TEXT NOT NULL,
            kind TEXT NOT NULL CHECK (kind IN ('leaf', 'split')),
            pane_id INTEGER,
            split_id INTEGER,
            axis TEXT,
            ratio REAL,
            PRIMARY KEY (workspace_id, path),
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS panes (
            workspace_id INTEGER NOT NULL,
            id INTEGER NOT NULL,
            active_tab_id INTEGER NOT NULL,
            PRIMARY KEY (workspace_id, id),
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS tabs (
            workspace_id INTEGER NOT NULL,
            pane_id INTEGER NOT NULL,
            position INTEGER NOT NULL,
            id INTEGER NOT NULL,
            title TEXT NOT NULL,
            kind TEXT NOT NULL,
            PRIMARY KEY (workspace_id, id),
            UNIQUE (workspace_id, pane_id, position),
            FOREIGN KEY (workspace_id, pane_id) REFERENCES panes(workspace_id, id) ON DELETE CASCADE
        );
        ",
    )?;

    Ok(())
}

fn clear_state(transaction: &Transaction<'_>) -> Result<()> {
    transaction.execute("DELETE FROM metadata", [])?;
    transaction.execute("DELETE FROM tabs", [])?;
    transaction.execute("DELETE FROM panes", [])?;
    transaction.execute("DELETE FROM pane_nodes", [])?;
    transaction.execute("DELETE FROM workspaces", [])?;

    Ok(())
}

fn save_state(transaction: &Transaction<'_>, state: &State) -> Result<()> {
    if let Some(active_workspace_id) = state.workspaces().active_workspace_id() {
        transaction.execute(
            "INSERT INTO metadata (key, value) VALUES ('active_workspace_id', ?1)",
            params![active_workspace_id.value().to_string()],
        )?;
    }

    for (position, workspace) in state.workspaces().workspaces().iter().enumerate() {
        let workspace_id = to_i64(workspace.id().value(), "workspace id")?;
        let active_pane_id = to_i64(workspace.active_pane_id().value(), "active pane id")?;

        transaction.execute(
            "
            INSERT INTO workspaces (id, position, name, directory, active_pane_id)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ",
            params![
                workspace_id,
                usize_to_i64(position, "workspace position")?,
                workspace.name(),
                workspace.directory().to_string_lossy(),
                active_pane_id,
            ],
        )?;

        save_node(transaction, workspace.id(), "", workspace.root())?;
    }

    Ok(())
}

fn save_node(
    transaction: &Transaction<'_>,
    workspace_id: WorkspaceId,
    path: &str,
    node: &PaneNode,
) -> Result<()> {
    match node {
        PaneNode::Leaf(pane) => {
            transaction.execute(
                "
                INSERT INTO pane_nodes (workspace_id, path, kind, pane_id)
                VALUES (?1, ?2, 'leaf', ?3)
                ",
                params![
                    to_i64(workspace_id.value(), "workspace id")?,
                    path,
                    to_i64(pane.id().value(), "pane id")?,
                ],
            )?;
            save_pane(transaction, workspace_id, pane)
        }
        PaneNode::Split(split) => {
            transaction.execute(
                "
                INSERT INTO pane_nodes (workspace_id, path, kind, split_id, axis, ratio)
                VALUES (?1, ?2, 'split', ?3, ?4, ?5)
                ",
                params![
                    to_i64(workspace_id.value(), "workspace id")?,
                    path,
                    to_i64(split.id().value(), "split id")?,
                    split_axis_name(split.axis()),
                    split.ratio(),
                ],
            )?;

            save_node(
                transaction,
                workspace_id,
                &child_path(path, 0),
                split.first(),
            )?;
            save_node(
                transaction,
                workspace_id,
                &child_path(path, 1),
                split.second(),
            )
        }
    }
}

fn save_pane(transaction: &Transaction<'_>, workspace_id: WorkspaceId, pane: &Pane) -> Result<()> {
    transaction.execute(
        "
        INSERT INTO panes (workspace_id, id, active_tab_id)
        VALUES (?1, ?2, ?3)
        ",
        params![
            to_i64(workspace_id.value(), "workspace id")?,
            to_i64(pane.id().value(), "pane id")?,
            to_i64(pane.active_tab_id().value(), "active tab id")?,
        ],
    )?;

    for (position, tab) in pane.tabs().iter().enumerate() {
        transaction.execute(
            "
            INSERT INTO tabs (workspace_id, pane_id, position, id, title, kind)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ",
            params![
                to_i64(workspace_id.value(), "workspace id")?,
                to_i64(pane.id().value(), "pane id")?,
                usize_to_i64(position, "tab position")?,
                to_i64(tab.id().value(), "tab id")?,
                tab.title(),
                tab_kind_name(tab.kind()),
            ],
        )?;
    }

    Ok(())
}

fn load_state(connection: &Connection) -> Result<State> {
    let active_workspace_id = load_active_workspace_id(connection)?;
    let workspace_rows = load_workspace_rows(connection)?;
    let mut workspaces = Vec::with_capacity(workspace_rows.len());

    for workspace_row in workspace_rows {
        let root = load_node(connection, workspace_row.id, "")?;
        let mut workspace = Workspace::from_root(
            workspace_row.id,
            workspace_row.directory,
            root,
            workspace_row.active_pane_id,
        )
        .ok_or_else(|| invalid_state("workspace active pane does not exist in its root"))?;

        workspace.rename(workspace_row.name);
        workspaces.push(workspace);
    }

    State::from_workspaces(workspaces, active_workspace_id)
        .ok_or_else(|| invalid_state("persisted workspaces are not internally consistent"))
}

fn load_active_workspace_id(connection: &Connection) -> Result<Option<WorkspaceId>> {
    let value = connection
        .query_row(
            "SELECT value FROM metadata WHERE key = 'active_workspace_id'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;

    value
        .map(|value| {
            value
                .parse::<u64>()
                .map(WorkspaceId::new)
                .map_err(|_| invalid_state("active workspace id is not a valid integer"))
        })
        .transpose()
}

#[derive(Debug)]
struct WorkspaceRow {
    id: WorkspaceId,
    name: String,
    directory: PathBuf,
    active_pane_id: PaneId,
}

fn load_workspace_rows(connection: &Connection) -> Result<Vec<WorkspaceRow>> {
    let mut statement = connection.prepare(
        "
        SELECT id, name, directory, active_pane_id
        FROM workspaces
        ORDER BY position
        ",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;

    let mut workspace_rows = Vec::new();

    for row in rows {
        let (id, name, directory, active_pane_id) = row?;

        workspace_rows.push(WorkspaceRow {
            id: WorkspaceId::new(to_u64(id, "workspace id")?),
            name,
            directory: PathBuf::from(directory),
            active_pane_id: PaneId::new(to_u64(active_pane_id, "active pane id")?),
        });
    }

    Ok(workspace_rows)
}

#[derive(Debug)]
struct NodeRow {
    kind: String,
    pane_id: Option<i64>,
    split_id: Option<i64>,
    axis: Option<String>,
    ratio: Option<f64>,
}

fn load_node(connection: &Connection, workspace_id: WorkspaceId, path: &str) -> Result<PaneNode> {
    let node = connection
        .query_row(
            "
            SELECT kind, pane_id, split_id, axis, ratio
            FROM pane_nodes
            WHERE workspace_id = ?1 AND path = ?2
            ",
            params![to_i64(workspace_id.value(), "workspace id")?, path],
            |row| {
                Ok(NodeRow {
                    kind: row.get(0)?,
                    pane_id: row.get(1)?,
                    split_id: row.get(2)?,
                    axis: row.get(3)?,
                    ratio: row.get(4)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| invalid_state(format!("missing pane node at path {path:?}")))?;

    match node.kind.as_str() {
        "leaf" => {
            let pane_id = required_i64(node.pane_id, "leaf pane id")?;
            let pane = load_pane(
                connection,
                workspace_id,
                PaneId::new(to_u64(pane_id, "pane id")?),
            )?;

            Ok(PaneNode::leaf(pane))
        }
        "split" => {
            let split_id = required_i64(node.split_id, "split id")?;
            let axis = parse_split_axis(required_string(node.axis, "split axis")?)?;
            let ratio = parse_split_ratio(required_f64(node.ratio, "split ratio")?)?;
            let first = load_node(connection, workspace_id, &child_path(path, 0))?;
            let second = load_node(connection, workspace_id, &child_path(path, 1))?;

            Ok(PaneNode::split(
                SplitPaneId::new(to_u64(split_id, "split id")?),
                axis,
                ratio,
                first,
                second,
            ))
        }
        _ => Err(invalid_state(format!(
            "unknown pane node kind {:?}",
            node.kind
        ))),
    }
}

fn load_pane(connection: &Connection, workspace_id: WorkspaceId, pane_id: PaneId) -> Result<Pane> {
    let active_tab_id = connection
        .query_row(
            "
            SELECT active_tab_id
            FROM panes
            WHERE workspace_id = ?1 AND id = ?2
            ",
            params![
                to_i64(workspace_id.value(), "workspace id")?,
                to_i64(pane_id.value(), "pane id")?,
            ],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .ok_or_else(|| invalid_state(format!("missing pane {}", pane_id.value())))?;
    let active_tab_id = TabId::new(to_u64(active_tab_id, "active tab id")?);
    let mut tabs = load_tabs(connection, workspace_id, pane_id)?;

    if tabs.is_empty() {
        return Err(invalid_state(format!(
            "pane {} has no tabs",
            pane_id.value()
        )));
    }

    let first_tab = tabs.remove(0);
    let mut pane = Pane::new(pane_id, first_tab);

    for tab in tabs {
        pane.add_tab(tab);
    }

    if !pane.activate_tab(active_tab_id) {
        return Err(invalid_state(format!(
            "pane {} active tab does not exist",
            pane_id.value()
        )));
    }

    Ok(pane)
}

fn load_tabs(
    connection: &Connection,
    workspace_id: WorkspaceId,
    pane_id: PaneId,
) -> Result<Vec<Tab>> {
    let mut statement = connection.prepare(
        "
        SELECT id, title, kind
        FROM tabs
        WHERE workspace_id = ?1 AND pane_id = ?2
        ORDER BY position
        ",
    )?;
    let rows = statement.query_map(
        params![
            to_i64(workspace_id.value(), "workspace id")?,
            to_i64(pane_id.value(), "pane id")?,
        ],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        },
    )?;
    let mut tabs = Vec::new();

    for row in rows {
        let (id, title, kind) = row?;

        tabs.push(Tab::new(
            TabId::new(to_u64(id, "tab id")?),
            title,
            parse_tab_kind(&kind)?,
        ));
    }

    Ok(tabs)
}

fn child_path(path: &str, index: u8) -> String {
    if path.is_empty() {
        index.to_string()
    } else {
        format!("{path}/{index}")
    }
}

fn split_axis_name(axis: SplitAxis) -> &'static str {
    match axis {
        SplitAxis::Horizontal => "horizontal",
        SplitAxis::Vertical => "vertical",
    }
}

fn parse_split_axis(axis: String) -> Result<SplitAxis> {
    match axis.as_str() {
        "horizontal" => Ok(SplitAxis::Horizontal),
        "vertical" => Ok(SplitAxis::Vertical),
        _ => Err(invalid_state(format!("unknown split axis {axis:?}"))),
    }
}

fn tab_kind_name(kind: &TabKind) -> &'static str {
    match kind {
        TabKind::Blank => "blank",
        TabKind::FileTree => "file_tree",
        TabKind::Editor => "editor",
        TabKind::Git => "git",
        TabKind::Search => "search",
        TabKind::Terminal => "terminal",
        TabKind::Settings => "settings",
    }
}

fn parse_tab_kind(kind: &str) -> Result<TabKind> {
    match kind {
        "blank" => Ok(TabKind::Blank),
        "file_tree" => Ok(TabKind::FileTree),
        "editor" => Ok(TabKind::Editor),
        "git" => Ok(TabKind::Git),
        "search" => Ok(TabKind::Search),
        "terminal" => Ok(TabKind::Terminal),
        "settings" => Ok(TabKind::Settings),
        _ => Err(invalid_state(format!("unknown tab kind {kind:?}"))),
    }
}

fn parse_split_ratio(ratio: f64) -> Result<f32> {
    let ratio = ratio as f32;

    if ratio.is_finite() && ratio > 0.0 && ratio < 1.0 {
        Ok(ratio)
    } else {
        Err(invalid_state("split ratio must be between 0.0 and 1.0"))
    }
}

fn required_i64(value: Option<i64>, field: &str) -> Result<i64> {
    value.ok_or_else(|| invalid_state(format!("missing {field}")))
}

fn required_f64(value: Option<f64>, field: &str) -> Result<f64> {
    value.ok_or_else(|| invalid_state(format!("missing {field}")))
}

fn required_string(value: Option<String>, field: &str) -> Result<String> {
    value.ok_or_else(|| invalid_state(format!("missing {field}")))
}

fn to_i64(value: u64, field: &str) -> Result<i64> {
    i64::try_from(value).map_err(|_| invalid_state(format!("{field} is too large for SQLite")))
}

fn usize_to_i64(value: usize, field: &str) -> Result<i64> {
    i64::try_from(value).map_err(|_| invalid_state(format!("{field} is too large for SQLite")))
}

fn to_u64(value: i64, field: &str) -> Result<u64> {
    u64::try_from(value).map_err(|_| invalid_state(format!("{field} must not be negative")))
}

fn invalid_state(message: impl Into<String>) -> PersistenceError {
    PersistenceError::InvalidState(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn loading_empty_store_returns_empty_state() {
        let path = test_db_path("empty");
        let store = StateStore::open(&path).expect("store should open");

        let state = store.load().expect("state should load");

        assert!(state.workspaces().is_empty());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn saves_and_loads_workspace_tree() {
        let path = test_db_path("round-trip");
        let store = StateStore::open(&path).expect("store should open");
        let mut state = State::new();

        state.open_workspace("/workspaces/main");
        assert!(state.open_tab(None, None, "Search", TabKind::Search));
        assert!(state.split_pane(None, None, SplitAxis::Horizontal, false));
        assert!(state.resize_split(None, SplitPaneId::new(1), 0.65));
        assert!(state.activate_pane(None, PaneId::new(1)));
        assert!(state.activate_tab(None, PaneId::new(1), TabId::new(2)));

        store.save(&state).expect("state should save");

        let mut loaded = store.load().expect("state should load");

        assert_eq!(loaded.workspaces(), state.workspaces());

        assert!(loaded.open_tab(None, None, "Terminal", TabKind::Terminal));
        let workspace = loaded
            .workspaces()
            .active_workspace()
            .expect("workspace should be active");
        let pane = workspace
            .root()
            .find_pane(PaneId::new(1))
            .expect("pane should exist");

        assert!(pane.contains_tab(TabId::new(4)));

        let _ = fs::remove_file(path);
    }

    fn test_db_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();

        std::env::temp_dir().join(format!(
            "kosmos-core-persistence-{}-{name}-{nanos}.sqlite3",
            std::process::id()
        ))
    }
}
