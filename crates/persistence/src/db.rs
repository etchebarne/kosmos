use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use rusqlite::{Connection, Result};

const MIGRATIONS: &[&str] = &[include_str!("migrations/001_initial.sql")];

static CONN: OnceLock<Option<Mutex<Connection>>> = OnceLock::new();

pub fn with<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut Connection) -> Result<R>,
{
    let mu = CONN.get_or_init(open).as_ref()?;
    let mut conn = mu.lock().unwrap();
    match f(&mut conn) {
        Ok(value) => Some(value),
        Err(err) => {
            eprintln!("kosmos: db error: {err}");
            None
        }
    }
}

fn open() -> Option<Mutex<Connection>> {
    let path = data_path()?;
    if let Some(parent) = path.parent()
        && let Err(err) = fs::create_dir_all(parent)
    {
        eprintln!("kosmos: failed to create {}: {err}", parent.display());
        return None;
    }
    let mut conn = match Connection::open(&path) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("kosmos: failed to open db at {}: {err}", path.display());
            return None;
        }
    };
    if let Err(err) = init(&mut conn) {
        eprintln!("kosmos: db init failed: {err}");
        return None;
    }
    Some(Mutex::new(conn))
}

fn init(conn: &mut Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    let current: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate() {
        let version = (i + 1) as u32;
        if version <= current {
            continue;
        }
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
}

fn data_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".kosmos").join("data.db"))
}
