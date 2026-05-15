use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use rusqlite::{Connection, Result};

#[derive(Clone, Copy)]
pub struct Database {
    pub file_name: &'static str,
    pub label: &'static str,
    pub migrations: &'static [&'static str],
    pub foreign_keys: bool,
}

pub fn with_connection<F, R>(
    slot: &'static OnceLock<Option<Mutex<Connection>>>,
    database: Database,
    f: F,
) -> Option<R>
where
    F: FnOnce(&mut Connection) -> Result<R>,
{
    let mu = slot.get_or_init(|| open(database)).as_ref()?;
    let mut conn = mu.lock().unwrap();
    match f(&mut conn) {
        Ok(value) => Some(value),
        Err(err) => {
            eprintln!("kosmos: {} db error: {err}", database.label);
            None
        }
    }
}

fn open(database: Database) -> Option<Mutex<Connection>> {
    let path = data_path(database.file_name)?;
    if let Some(parent) = path.parent()
        && let Err(err) = fs::create_dir_all(parent)
    {
        eprintln!("kosmos: failed to create {}: {err}", parent.display());
        return None;
    }
    let mut conn = match Connection::open(&path) {
        Ok(c) => c,
        Err(err) => {
            eprintln!(
                "kosmos: failed to open {} db at {}: {err}",
                database.label,
                path.display()
            );
            return None;
        }
    };
    if let Err(err) = init(&mut conn, database) {
        eprintln!("kosmos: {} db init failed: {err}", database.label);
        return None;
    }
    Some(Mutex::new(conn))
}

fn init(conn: &mut Connection, database: Database) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    if database.foreign_keys {
        conn.pragma_update(None, "foreign_keys", "ON")?;
    }
    let current: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (i, sql) in database.migrations.iter().enumerate() {
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

fn data_path(file_name: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".kosmos").join(file_name))
}
