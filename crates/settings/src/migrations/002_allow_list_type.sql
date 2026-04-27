CREATE TABLE settings_new (
    key TEXT PRIMARY KEY,
    value_type TEXT NOT NULL CHECK (value_type IN ('bool', 'string', 'int', 'list')),
    value TEXT NOT NULL
);

INSERT INTO settings_new (key, value_type, value)
SELECT key, value_type, value FROM settings;

DROP TABLE settings;

ALTER TABLE settings_new RENAME TO settings;
