use std::sync::{Mutex, OnceLock};

use rusqlite::{Connection, Result};
use storage::{Database, with_connection};

const MIGRATIONS: &[&str] = &[
    include_str!("migrations/001_initial.sql"),
    include_str!("migrations/002_tab_path.sql"),
];

static CONN: OnceLock<Option<Mutex<Connection>>> = OnceLock::new();

const DATABASE: Database = Database {
    file_name: "data.db",
    label: "session",
    migrations: MIGRATIONS,
    foreign_keys: true,
};

pub fn with<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut Connection) -> Result<R>,
{
    with_connection(&CONN, DATABASE, f)
}
