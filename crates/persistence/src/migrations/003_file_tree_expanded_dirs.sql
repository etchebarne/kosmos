CREATE TABLE file_tree_expanded_dirs (
    workspace_id INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    PRIMARY KEY (workspace_id, path)
);
