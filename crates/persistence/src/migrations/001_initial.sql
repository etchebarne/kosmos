CREATE TABLE session (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    active_workspace_id INTEGER REFERENCES workspaces(id) ON DELETE SET NULL,
    next_workspace_id INTEGER NOT NULL
);

CREATE TABLE window (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    state TEXT NOT NULL CHECK (state IN ('windowed', 'maximized', 'fullscreen')),
    origin_x REAL NOT NULL,
    origin_y REAL NOT NULL,
    width REAL NOT NULL,
    height REAL NOT NULL
);

CREATE TABLE workspaces (
    id INTEGER PRIMARY KEY,
    position INTEGER NOT NULL,
    path TEXT NOT NULL,
    name TEXT NOT NULL,
    next_tab_id INTEGER NOT NULL,
    next_pane_id INTEGER NOT NULL,
    next_split_id INTEGER NOT NULL
);

CREATE TABLE pane_nodes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    parent_id INTEGER REFERENCES pane_nodes(id) ON DELETE CASCADE,
    position INTEGER,
    kind TEXT NOT NULL CHECK (kind IN ('leaf', 'split')),
    split_id INTEGER,
    axis TEXT CHECK (axis IN ('row', 'column')),
    ratio REAL,
    pane_id INTEGER,
    active_tab_id INTEGER
);

CREATE INDEX pane_nodes_by_parent ON pane_nodes(workspace_id, parent_id);

CREATE TABLE tabs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pane_node_id INTEGER NOT NULL REFERENCES pane_nodes(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    tab_id INTEGER NOT NULL,
    kind TEXT NOT NULL,
    title TEXT
);

CREATE INDEX tabs_by_pane ON tabs(pane_node_id, position);
