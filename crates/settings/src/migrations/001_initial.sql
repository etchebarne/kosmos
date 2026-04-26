CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value_type TEXT NOT NULL CHECK (value_type IN ('bool', 'string', 'int')),
    value TEXT NOT NULL
);
