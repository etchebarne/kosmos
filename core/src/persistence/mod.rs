use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::State;
use crate::settings::{SettingValue, Settings, SettingsError};
use crate::tabs::editor::EditorViewState;
use crate::tabs::file_tree::FileTreeViewState;
use crate::tabs::git::GitDiffViewState;
use crate::tree::{
    Pane, PaneId, PaneNode, SplitAxis, SplitPaneId, Tab, TabId, TabKind, Workspace, WorkspaceId,
};
use crate::window::WindowState;

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
        let connection = store.connection()?;
        migrate(&connection)?;

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

    pub fn save_active_workspace(&self, state: &State) -> Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;

        replace_active_workspace_metadata(&transaction, state.workspaces().active_workspace_id())?;
        transaction.commit()?;

        Ok(())
    }

    pub fn save_settings(&self, state: &State) -> Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;

        replace_settings(&transaction, state.settings())?;
        transaction.commit()?;

        Ok(())
    }

    pub fn save_window_state(&self, state: &State) -> Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;

        replace_window_state(&transaction, state.window_state())?;
        transaction.commit()?;

        Ok(())
    }

    pub fn language_server_selections(&self) -> Result<BTreeMap<String, String>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT server_id, selected_version FROM language_server_configurations ORDER BY server_id",
        )?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let mut selections = BTreeMap::new();

        for row in rows {
            let (server_id, selected_version) = row?;
            selections.insert(server_id, selected_version);
        }

        Ok(selections)
    }

    pub fn select_language_server_version(&self, server_id: &str, version: &str) -> Result<()> {
        let connection = self.connection()?;
        connection.execute(
            "
            INSERT INTO language_server_configurations (server_id, selected_version)
            VALUES (?1, ?2)
            ON CONFLICT(server_id) DO UPDATE SET selected_version = excluded.selected_version
            ",
            params![server_id, version],
        )?;
        Ok(())
    }

    pub fn clear_language_server_selection(&self, server_id: &str) -> Result<()> {
        let connection = self.connection()?;
        connection.execute(
            "DELETE FROM language_server_configurations WHERE server_id = ?1",
            params![server_id],
        )?;
        Ok(())
    }

    pub fn formatter_priorities(&self) -> Result<Vec<String>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT formatter_id FROM formatter_preferences ORDER BY priority, formatter_id",
        )?;
        let rows = statement.query_map([], |row| row.get(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(PersistenceError::from)
    }

    pub fn set_formatter_priorities(&self, formatter_ids: &[String]) -> Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM formatter_preferences", [])?;
        for (priority, formatter_id) in formatter_ids.iter().enumerate() {
            transaction.execute(
                "INSERT INTO formatter_preferences (formatter_id, priority) VALUES (?1, ?2)",
                params![formatter_id, usize_to_i64(priority, "formatter priority")?],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn trusted_language_server_workspaces(&self) -> Result<Vec<PathBuf>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT directory FROM language_server_trusted_workspaces ORDER BY directory",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut workspaces = Vec::new();
        for row in rows {
            workspaces.push(PathBuf::from(row?));
        }
        Ok(workspaces)
    }

    pub fn trust_language_server_workspace(&self, directory: &Path) -> Result<()> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT OR IGNORE INTO language_server_trusted_workspaces (directory) VALUES (?1)",
            params![directory.to_string_lossy()],
        )?;
        Ok(())
    }

    fn connection(&self) -> Result<Connection> {
        let connection = Connection::open(&self.path)?;
        connection.pragma_update(None, "foreign_keys", "ON")?;

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
        CREATE TABLE IF NOT EXISTS metadata (
            key TEXT PRIMARY KEY NOT NULL,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY NOT NULL,
            value_type TEXT NOT NULL CHECK (value_type IN ('boolean', 'string', 'number')),
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS language_server_configurations (
            server_id TEXT PRIMARY KEY NOT NULL,
            selected_version TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS language_server_trusted_workspaces (
            directory TEXT PRIMARY KEY NOT NULL
        );

        CREATE TABLE IF NOT EXISTS formatter_preferences (
            formatter_id TEXT PRIMARY KEY NOT NULL,
            priority INTEGER NOT NULL UNIQUE CHECK (priority >= 0)
        );

        CREATE TABLE IF NOT EXISTS window_state (
            id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
            x INTEGER NOT NULL,
            y INTEGER NOT NULL,
            width INTEGER NOT NULL CHECK (width > 0),
            height INTEGER NOT NULL CHECK (height > 0),
            maximized INTEGER NOT NULL CHECK (maximized IN (0, 1)),
            fullscreen INTEGER NOT NULL CHECK (fullscreen IN (0, 1))
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

        CREATE TABLE IF NOT EXISTS file_tree_expanded_paths (
            workspace_id INTEGER NOT NULL,
            tab_id INTEGER NOT NULL,
            position INTEGER NOT NULL,
            path TEXT NOT NULL,
            PRIMARY KEY (workspace_id, tab_id, path),
            UNIQUE (workspace_id, tab_id, position),
            FOREIGN KEY (workspace_id, tab_id) REFERENCES tabs(workspace_id, id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS git_diff_tabs (
            workspace_id INTEGER NOT NULL,
            tab_id INTEGER NOT NULL,
            path TEXT NOT NULL,
            PRIMARY KEY (workspace_id, tab_id),
            FOREIGN KEY (workspace_id, tab_id) REFERENCES tabs(workspace_id, id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS editor_tabs (
            workspace_id INTEGER NOT NULL,
            tab_id INTEGER NOT NULL,
            path TEXT NOT NULL,
            PRIMARY KEY (workspace_id, tab_id),
            UNIQUE (workspace_id, path),
            FOREIGN KEY (workspace_id, tab_id) REFERENCES tabs(workspace_id, id) ON DELETE CASCADE
        );

        UPDATE tabs
        SET kind = 'blank'
        WHERE kind NOT IN ('blank', 'diff', 'file_tree', 'editor', 'git', 'search', 'terminal');

        UPDATE tabs
        SET kind = 'blank', title = 'Blank'
        WHERE kind = 'diff'
          AND NOT EXISTS (
              SELECT 1
              FROM git_diff_tabs
              WHERE git_diff_tabs.workspace_id = tabs.workspace_id
                AND git_diff_tabs.tab_id = tabs.id
          );

        UPDATE tabs
        SET kind = 'blank', title = 'Blank'
        WHERE kind = 'editor'
          AND NOT EXISTS (
              SELECT 1
              FROM editor_tabs
              WHERE editor_tabs.workspace_id = tabs.workspace_id
                AND editor_tabs.tab_id = tabs.id
          );
        ",
    )?;

    Ok(())
}

fn clear_state(transaction: &Transaction<'_>) -> Result<()> {
    transaction.execute("DELETE FROM metadata", [])?;
    transaction.execute("DELETE FROM settings", [])?;
    transaction.execute("DELETE FROM window_state", [])?;
    transaction.execute("DELETE FROM file_tree_expanded_paths", [])?;
    transaction.execute("DELETE FROM git_diff_tabs", [])?;
    transaction.execute("DELETE FROM editor_tabs", [])?;
    transaction.execute("DELETE FROM tabs", [])?;
    transaction.execute("DELETE FROM panes", [])?;
    transaction.execute("DELETE FROM pane_nodes", [])?;
    transaction.execute("DELETE FROM workspaces", [])?;

    Ok(())
}

fn save_state(transaction: &Transaction<'_>, state: &State) -> Result<()> {
    insert_active_workspace_metadata(transaction, state.workspaces().active_workspace_id())?;
    insert_settings(transaction, state.settings())?;
    insert_window_state(transaction, state.window_state())?;

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

    save_file_tree_view_states(transaction, state)?;
    save_git_diff_view_states(transaction, state)?;
    save_editor_view_states(transaction, state)?;

    Ok(())
}

fn replace_active_workspace_metadata(
    transaction: &Transaction<'_>,
    active_workspace_id: Option<WorkspaceId>,
) -> Result<()> {
    transaction.execute("DELETE FROM metadata WHERE key = 'active_workspace_id'", [])?;
    insert_active_workspace_metadata(transaction, active_workspace_id)
}

fn insert_active_workspace_metadata(
    transaction: &Transaction<'_>,
    active_workspace_id: Option<WorkspaceId>,
) -> Result<()> {
    if let Some(active_workspace_id) = active_workspace_id {
        transaction.execute(
            "INSERT INTO metadata (key, value) VALUES ('active_workspace_id', ?1)",
            params![active_workspace_id.value().to_string()],
        )?;
    }

    Ok(())
}

fn replace_settings(transaction: &Transaction<'_>, settings: &Settings) -> Result<()> {
    transaction.execute("DELETE FROM settings", [])?;
    insert_settings(transaction, settings)
}

fn insert_settings(transaction: &Transaction<'_>, settings: &Settings) -> Result<()> {
    for (key, value) in settings.overrides() {
        let (value_type, value) = encode_setting_value(value);
        transaction.execute(
            "INSERT INTO settings (key, value_type, value) VALUES (?1, ?2, ?3)",
            params![key, value_type, value],
        )?;
    }

    Ok(())
}

fn encode_setting_value(value: &SettingValue) -> (&'static str, String) {
    match value {
        SettingValue::Boolean(value) => ("boolean", if *value { "1" } else { "0" }.to_owned()),
        SettingValue::String(value) => ("string", value.clone()),
        SettingValue::Number(value) => ("number", value.to_string()),
    }
}

fn replace_window_state(
    transaction: &Transaction<'_>,
    window_state: Option<WindowState>,
) -> Result<()> {
    transaction.execute("DELETE FROM window_state", [])?;
    insert_window_state(transaction, window_state)
}

fn insert_window_state(
    transaction: &Transaction<'_>,
    window_state: Option<WindowState>,
) -> Result<()> {
    let Some(window_state) = window_state else {
        return Ok(());
    };

    transaction.execute(
        "
        INSERT INTO window_state (id, x, y, width, height, maximized, fullscreen)
        VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)
        ",
        params![
            window_state.x(),
            window_state.y(),
            window_state.width(),
            window_state.height(),
            window_state.is_maximized(),
            window_state.is_fullscreen(),
        ],
    )?;

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

fn save_file_tree_view_states(transaction: &Transaction<'_>, state: &State) -> Result<()> {
    for view_state in state.file_tree_view_states() {
        for (position, path) in view_state.expanded_paths().iter().enumerate() {
            transaction.execute(
                "
                INSERT INTO file_tree_expanded_paths (workspace_id, tab_id, position, path)
                VALUES (?1, ?2, ?3, ?4)
                ",
                params![
                    to_i64(view_state.workspace_id().value(), "workspace id")?,
                    to_i64(view_state.tab_id().value(), "tab id")?,
                    usize_to_i64(position, "file tree expanded path position")?,
                    path,
                ],
            )?;
        }
    }

    Ok(())
}

fn save_git_diff_view_states(transaction: &Transaction<'_>, state: &State) -> Result<()> {
    for view_state in state.git_diff_view_states() {
        transaction.execute(
            "
            INSERT INTO git_diff_tabs (workspace_id, tab_id, path)
            VALUES (?1, ?2, ?3)
            ",
            params![
                to_i64(view_state.workspace_id().value(), "workspace id")?,
                to_i64(view_state.tab_id().value(), "tab id")?,
                view_state.path(),
            ],
        )?;
    }

    Ok(())
}

fn save_editor_view_states(transaction: &Transaction<'_>, state: &State) -> Result<()> {
    for view_state in state.editor_view_states() {
        transaction.execute(
            "
            INSERT INTO editor_tabs (workspace_id, tab_id, path)
            VALUES (?1, ?2, ?3)
            ",
            params![
                to_i64(view_state.workspace_id().value(), "workspace id")?,
                to_i64(view_state.tab_id().value(), "tab id")?,
                view_state.path(),
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

    let file_tree_view_states = load_file_tree_view_states(connection)?;
    let git_diff_view_states = load_git_diff_view_states(connection)?;
    let editor_view_states = load_editor_view_states(connection)?;
    let settings = load_settings(connection)?;
    let window_state = load_window_state(connection)?;

    State::from_persisted(
        workspaces,
        active_workspace_id,
        file_tree_view_states,
        git_diff_view_states,
        editor_view_states,
        settings,
        window_state,
    )
    .ok_or_else(|| invalid_state("persisted workspaces are not internally consistent"))
}

fn load_window_state(connection: &Connection) -> Result<Option<WindowState>> {
    let row = connection
        .query_row(
            "SELECT x, y, width, height, maximized, fullscreen FROM window_state WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, i32>(0)?,
                    row.get::<_, i32>(1)?,
                    row.get::<_, u32>(2)?,
                    row.get::<_, u32>(3)?,
                    row.get::<_, bool>(4)?,
                    row.get::<_, bool>(5)?,
                ))
            },
        )
        .optional()?;

    row.map(|(x, y, width, height, maximized, fullscreen)| {
        WindowState::new(x, y, width, height, maximized, fullscreen)
            .ok_or_else(|| invalid_state("window state has invalid dimensions"))
    })
    .transpose()
}

fn load_settings(connection: &Connection) -> Result<Settings> {
    let mut statement =
        connection.prepare("SELECT key, value_type, value FROM settings ORDER BY key")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut settings = Settings::default();

    for row in rows {
        let (key, value_type, value) = row?;
        if settings.value(&key).is_none() {
            continue;
        }

        let value = decode_setting_value(&key, &value_type, value)?;
        settings
            .update(&key, value)
            .map_err(persisted_setting_error)?;
    }

    Ok(settings)
}

fn decode_setting_value(key: &str, value_type: &str, value: String) -> Result<SettingValue> {
    match value_type {
        "boolean" => match value.as_str() {
            "0" => Ok(SettingValue::Boolean(false)),
            "1" => Ok(SettingValue::Boolean(true)),
            _ => Err(invalid_state(format!(
                "setting `{key}` has an invalid boolean value"
            ))),
        },
        "string" => Ok(SettingValue::String(value)),
        "number" => value
            .parse::<f64>()
            .map(SettingValue::Number)
            .map_err(|_| invalid_state(format!("setting `{key}` has an invalid number value"))),
        _ => Err(invalid_state(format!(
            "setting `{key}` has an unknown value type"
        ))),
    }
}

fn persisted_setting_error(error: SettingsError) -> PersistenceError {
    invalid_state(format!("persisted {error}"))
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

fn load_file_tree_view_states(connection: &Connection) -> Result<Vec<FileTreeViewState>> {
    let mut statement = connection.prepare(
        "
        SELECT workspace_id, tab_id, path
        FROM file_tree_expanded_paths
        ORDER BY workspace_id, tab_id, position
        ",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut view_states = Vec::new();
    let mut current_key: Option<(WorkspaceId, TabId)> = None;
    let mut current_paths = Vec::new();

    for row in rows {
        let (workspace_id, tab_id, path) = row?;
        let key = (
            WorkspaceId::new(to_u64(workspace_id, "workspace id")?),
            TabId::new(to_u64(tab_id, "tab id")?),
        );

        if current_key.is_some_and(|current_key| current_key != key) {
            push_file_tree_view_state(&mut view_states, current_key, &mut current_paths);
        }

        current_key = Some(key);
        current_paths.push(path);
    }

    push_file_tree_view_state(&mut view_states, current_key, &mut current_paths);

    Ok(view_states)
}

fn load_git_diff_view_states(connection: &Connection) -> Result<Vec<GitDiffViewState>> {
    let mut statement = connection.prepare(
        "
        SELECT workspace_id, tab_id, path
        FROM git_diff_tabs
        ORDER BY workspace_id, tab_id
        ",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut view_states = Vec::new();

    for row in rows {
        let (workspace_id, tab_id, path) = row?;

        view_states.push(GitDiffViewState::new(
            WorkspaceId::new(to_u64(workspace_id, "workspace id")?),
            TabId::new(to_u64(tab_id, "tab id")?),
            path,
        ));
    }

    Ok(view_states)
}

fn load_editor_view_states(connection: &Connection) -> Result<Vec<EditorViewState>> {
    let mut statement = connection.prepare(
        "
        SELECT workspace_id, tab_id, path
        FROM editor_tabs
        ORDER BY workspace_id, tab_id
        ",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut view_states = Vec::new();

    for row in rows {
        let (workspace_id, tab_id, path) = row?;

        view_states.push(EditorViewState::new(
            WorkspaceId::new(to_u64(workspace_id, "workspace id")?),
            TabId::new(to_u64(tab_id, "tab id")?),
            path,
        ));
    }

    Ok(view_states)
}

fn push_file_tree_view_state(
    view_states: &mut Vec<FileTreeViewState>,
    key: Option<(WorkspaceId, TabId)>,
    paths: &mut Vec<String>,
) {
    let Some((workspace_id, tab_id)) = key else {
        return;
    };
    let paths = std::mem::take(paths);
    let view_state = FileTreeViewState::new(workspace_id, tab_id, paths);

    if !view_state.expanded_paths().is_empty() {
        view_states.push(view_state);
    }
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
        TabKind::Diff => "diff",
        TabKind::FileTree => "file_tree",
        TabKind::Editor => "editor",
        TabKind::Git => "git",
        TabKind::Search => "search",
        TabKind::Terminal => "terminal",
    }
}

fn parse_tab_kind(kind: &str) -> Result<TabKind> {
    match kind {
        "blank" => Ok(TabKind::Blank),
        "diff" => Ok(TabKind::Diff),
        "file_tree" => Ok(TabKind::FileTree),
        "editor" => Ok(TabKind::Editor),
        "git" => Ok(TabKind::Git),
        "search" => Ok(TabKind::Search),
        "terminal" => Ok(TabKind::Terminal),
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
    fn every_store_connection_enforces_foreign_keys() {
        let path = test_db_path("foreign-keys");
        let store = StateStore::open(&path).expect("store should open");
        let connection = store.connection().expect("connection should open");
        let enabled = connection
            .query_row("PRAGMA foreign_keys", [], |row| row.get::<_, bool>(0))
            .expect("foreign key setting should be readable");

        assert!(enabled);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn migrates_legacy_diff_tabs_without_view_state_to_blank() {
        let path = test_db_path("legacy-diff-tab");
        let store = StateStore::open(&path).expect("store should open");
        let connection = store.connection().expect("connection should open");
        connection
            .execute(
                "INSERT INTO workspaces (id, position, name, directory, active_pane_id) VALUES (1, 0, 'main', '/workspaces/main', 1)",
                [],
            )
            .expect("workspace should be inserted");
        connection
            .execute(
                "INSERT INTO panes (workspace_id, id, active_tab_id) VALUES (1, 1, 1)",
                [],
            )
            .expect("pane should be inserted");
        connection
            .execute(
                "INSERT INTO tabs (workspace_id, pane_id, position, id, title, kind) VALUES (1, 1, 0, 1, 'Diff', 'diff')",
                [],
            )
            .expect("legacy diff tab should be inserted");
        drop(connection);

        let reopened = StateStore::open(&path).expect("store should reopen");
        let connection = reopened.connection().expect("connection should open");
        let (title, kind) = connection
            .query_row(
                "SELECT title, kind FROM tabs WHERE workspace_id = 1 AND id = 1",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .expect("tab should load");

        assert_eq!((title.as_str(), kind.as_str()), ("Blank", "blank"));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn migrates_editor_tabs_without_view_state_to_blank() {
        let path = test_db_path("orphan-editor-tab");
        let store = StateStore::open(&path).expect("store should open");
        let connection = store.connection().expect("connection should open");
        connection
            .execute(
                "INSERT INTO workspaces (id, position, name, directory, active_pane_id) VALUES (1, 0, 'main', '/workspaces/main', 1)",
                [],
            )
            .expect("workspace should be inserted");
        connection
            .execute(
                "INSERT INTO panes (workspace_id, id, active_tab_id) VALUES (1, 1, 1)",
                [],
            )
            .expect("pane should be inserted");
        connection
            .execute(
                "INSERT INTO tabs (workspace_id, pane_id, position, id, title, kind) VALUES (1, 1, 0, 1, 'main.rs', 'editor')",
                [],
            )
            .expect("orphan editor tab should be inserted");
        drop(connection);

        let reopened = StateStore::open(&path).expect("store should reopen");
        let connection = reopened.connection().expect("connection should open");
        let (title, kind) = connection
            .query_row(
                "SELECT title, kind FROM tabs WHERE workspace_id = 1 AND id = 1",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .expect("tab should load");

        assert_eq!((title.as_str(), kind.as_str()), ("Blank", "blank"));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn saves_and_loads_workspace_tree() {
        let path = test_db_path("round-trip");
        let store = StateStore::open(&path).expect("store should open");
        let mut state = State::new();

        state.open_workspace("/workspaces/main");
        assert!(state.open_tab(None, None, Some("Search".to_owned()), TabKind::Search,));
        assert!(state.split_pane(None, None, SplitAxis::Horizontal, false));
        assert!(state.resize_split(None, SplitPaneId::new(1), 0.65));
        assert!(state.activate_pane(None, PaneId::new(1)));
        assert!(state.activate_tab(None, PaneId::new(1), TabId::new(2)));

        store.save(&state).expect("state should save");

        let mut loaded = store.load().expect("state should load");

        assert_eq!(loaded.workspaces(), state.workspaces());

        assert!(loaded.open_tab(None, None, None, TabKind::Terminal));
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

    #[test]
    fn saves_active_workspace_without_full_state_save() {
        let path = test_db_path("active-workspace");
        let store = StateStore::open(&path).expect("store should open");
        let mut state = State::new();
        let first_workspace_id = state.open_workspace("/workspaces/first");
        state.open_workspace("/workspaces/second");

        store.save(&state).expect("state should save");

        assert!(state.activate_workspace(first_workspace_id));
        store
            .save_active_workspace(&state)
            .expect("active workspace should save");

        let loaded = store.load().expect("state should load");

        assert_eq!(
            loaded.workspaces().active_workspace_id(),
            Some(first_workspace_id)
        );
        assert_eq!(loaded.workspaces().workspaces().len(), 2);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn saves_and_loads_settings() {
        let path = test_db_path("settings");
        let store = StateStore::open(&path).expect("store should open");
        let mut state = State::new();

        state
            .update_setting(
                crate::settings::EDITOR_SOFT_WRAP,
                SettingValue::Boolean(true),
            )
            .expect("setting should update");
        store.save_settings(&state).expect("settings should save");

        let loaded = store.load().expect("settings should load");

        assert_eq!(
            loaded.settings().boolean(crate::settings::EDITOR_SOFT_WRAP),
            Some(true)
        );
        assert_eq!(
            loaded.settings().boolean(crate::settings::EDITOR_MINIMAP),
            Some(false)
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn settings_only_save_preserves_workspace_state() {
        let path = test_db_path("settings-only");
        let store = StateStore::open(&path).expect("store should open");
        let mut state = State::new();
        state.open_workspace("/workspaces/main");
        store.save(&state).expect("state should save");

        state
            .update_setting(crate::settings::EDITOR_MINIMAP, SettingValue::Boolean(true))
            .expect("setting should update");
        store.save_settings(&state).expect("settings should save");

        let loaded = store.load().expect("state should load");

        assert_eq!(loaded.workspaces(), state.workspaces());
        assert_eq!(
            loaded.settings().boolean(crate::settings::EDITOR_MINIMAP),
            Some(true)
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn language_server_selections_survive_full_state_saves() {
        let path = test_db_path("language-server-selection");
        let store = StateStore::open(&path).expect("store should open");
        store
            .select_language_server_version("rust-analyzer", "2026-07-06")
            .expect("selection should save");

        let mut state = State::new();
        state.open_workspace("/workspaces/main");
        store.save(&state).expect("state should save");

        assert_eq!(
            store
                .language_server_selections()
                .expect("selections should load")
                .get("rust-analyzer")
                .map(String::as_str),
            Some("2026-07-06")
        );

        store
            .clear_language_server_selection("rust-analyzer")
            .expect("selection should clear");
        assert!(
            store
                .language_server_selections()
                .expect("selections should load")
                .is_empty()
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn formatter_priorities_are_ordered_and_survive_full_state_saves() {
        let path = test_db_path("formatter-priorities");
        let store = StateStore::open(&path).expect("store should open");
        let priorities = vec!["ruff".to_owned(), "prettier".to_owned(), "shfmt".to_owned()];
        store
            .set_formatter_priorities(&priorities)
            .expect("priorities should save");

        store.save(&State::new()).expect("state should save");

        assert_eq!(
            store
                .formatter_priorities()
                .expect("priorities should load"),
            priorities
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn trusted_language_server_workspaces_survive_full_state_saves() {
        let path = test_db_path("language-server-trust");
        let store = StateStore::open(&path).expect("store should open");
        let workspace = Path::new("/workspaces/trusted");
        store
            .trust_language_server_workspace(workspace)
            .expect("workspace trust should save");

        store.save(&State::new()).expect("state should save");

        assert_eq!(
            store
                .trusted_language_server_workspaces()
                .expect("workspace trust should load"),
            vec![workspace.to_path_buf()]
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn saves_window_state_without_replacing_other_state() {
        let path = test_db_path("window-state");
        let store = StateStore::open(&path).expect("store should open");
        let mut state = State::new();
        state.open_workspace("/workspaces/main");
        store.save(&state).expect("state should save");

        let window_state = WindowState::new(-120, 80, 1440, 900, true, false)
            .expect("window state should be valid");
        state.update_window_state(window_state);
        store
            .save_window_state(&state)
            .expect("window state should save");

        let loaded = store.load().expect("state should load");

        assert_eq!(loaded.window_state(), Some(window_state));
        assert_eq!(loaded.workspaces(), state.workspaces());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn saves_and_loads_file_tree_view_state() {
        let path = test_db_path("file-tree-view-state");
        let workspace_path = test_workspace_path("file-tree-view-state-workspace");
        fs::create_dir_all(workspace_path.join("src/components"))
            .expect("workspace directories should be created");
        let store = StateStore::open(&path).expect("store should open");
        let mut state = State::new();
        let workspace_id = state.open_workspace(&workspace_path);
        assert!(state.set_tab_kind(None, PaneId::new(1), TabId::new(1), TabKind::FileTree));
        assert!(state.set_file_tree_expanded_paths(
            Some(workspace_id),
            TabId::new(1),
            vec!["src".to_owned(), "src/components".to_owned()],
        ));

        store.save(&state).expect("state should save");

        let loaded = store.load().expect("state should load");
        let file_tree = loaded
            .file_tree(Some(workspace_id), Some(TabId::new(1)))
            .expect("file tree should load");

        assert_eq!(file_tree.expanded_paths(), &["src/", "src/components/"]);

        let _ = fs::remove_file(path);
        let _ = fs::remove_dir_all(workspace_path);
    }

    #[test]
    fn saves_and_loads_git_diff_view_state() {
        let path = test_db_path("git-diff-view-state");
        let store = StateStore::open(&path).expect("store should open");
        let mut state = State::new();
        let workspace_id = state.open_workspace("/workspaces/main");

        assert!(state.set_tab_kind(
            Some(workspace_id),
            PaneId::new(1),
            TabId::new(1),
            TabKind::Git,
        ));
        state
            .open_git_diff_tab(Some(workspace_id), TabId::new(1), "src/main.rs")
            .expect("diff tab should open");

        store.save(&state).expect("state should save");

        let loaded = store.load().expect("state should load");
        let view_state = loaded
            .git_diff_view_states()
            .first()
            .expect("diff view state should load");

        assert_eq!(view_state.workspace_id(), workspace_id);
        assert_eq!(view_state.tab_id(), TabId::new(2));
        assert_eq!(view_state.path(), "src/main.rs");

        let _ = fs::remove_file(path);
    }

    #[test]
    fn saves_and_loads_editor_view_state() {
        let path = test_db_path("editor-view-state");
        let workspace_path = test_workspace_path("editor-view-state-workspace");
        fs::create_dir_all(workspace_path.join("src"))
            .expect("workspace directories should be created");
        fs::write(workspace_path.join("src/main.rs"), "fn main() {}")
            .expect("editor file should be created");
        let store = StateStore::open(&path).expect("store should open");
        let mut state = State::new();
        let workspace_id = state.open_workspace(&workspace_path);

        assert!(state.set_tab_kind(
            Some(workspace_id),
            PaneId::new(1),
            TabId::new(1),
            TabKind::FileTree,
        ));
        state
            .open_editor_tab(Some(workspace_id), TabId::new(1), "src/main.rs")
            .expect("editor tab should open");

        store.save(&state).expect("state should save");

        let loaded = store.load().expect("state should load");
        let view_state = loaded
            .editor_view_states()
            .first()
            .expect("editor view state should load");
        let document = loaded
            .editor_document(Some(workspace_id), view_state.tab_id())
            .expect("editor document should load");

        assert_eq!(view_state.workspace_id(), workspace_id);
        assert_eq!(view_state.path(), "src/main.rs");
        assert_eq!(document.content(), "fn main() {}");

        let _ = fs::remove_file(path);
        let _ = fs::remove_dir_all(workspace_path);
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

    fn test_workspace_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();

        std::env::temp_dir().join(format!(
            "kosmos-core-persistence-workspace-{}-{name}-{nanos}",
            std::process::id()
        ))
    }
}
