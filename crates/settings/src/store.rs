use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use gpui::SharedString;
use rusqlite::{Connection, Result, params};
use storage::{Database, with_connection};

use crate::value::SettingValue;

const MIGRATIONS: &[&str] = &[
    include_str!("migrations/001_initial.sql"),
    include_str!("migrations/002_allow_list_type.sql"),
];

static CONN: OnceLock<Option<Mutex<Connection>>> = OnceLock::new();

const DATABASE: Database = Database {
    file_name: "settings.db",
    label: "settings",
    migrations: MIGRATIONS,
    foreign_keys: false,
};

pub fn load_all() -> HashMap<SharedString, SettingValue> {
    with(load_inner).unwrap_or_default()
}

pub fn save(key: &str, value: &SettingValue) {
    with(|conn| save_inner(conn, key, value));
}

fn load_inner(conn: &mut Connection) -> Result<HashMap<SharedString, SettingValue>> {
    let mut stmt = conn.prepare("SELECT key, value_type, value FROM settings")?;
    let rows = stmt.query_map([], |row| {
        let key: String = row.get(0)?;
        let kind: String = row.get(1)?;
        let raw: String = row.get(2)?;
        Ok((key, kind, raw))
    })?;
    let mut map = HashMap::new();
    for row in rows {
        let (key, kind, raw) = row?;
        let Some(value) = decode(&kind, &raw) else {
            continue;
        };
        map.insert(SharedString::from(key), value);
    }
    Ok(map)
}

fn save_inner(conn: &mut Connection, key: &str, value: &SettingValue) -> Result<()> {
    let (kind, raw) = encode(value);
    conn.execute(
        "INSERT INTO settings (key, value_type, value) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET
           value_type = excluded.value_type,
           value = excluded.value",
        params![key, kind, raw],
    )?;
    Ok(())
}

fn encode(value: &SettingValue) -> (&'static str, String) {
    match value {
        SettingValue::Bool(b) => ("bool", if *b { "1" } else { "0" }.to_string()),
        SettingValue::String(s) => ("string", s.to_string()),
        SettingValue::Int(i) => ("int", i.to_string()),
        SettingValue::List(items) => {
            let arr: Vec<serde_json::Value> = items.iter().map(value_to_json).collect();
            ("list", serde_json::Value::Array(arr).to_string())
        }
    }
}

fn decode(kind: &str, raw: &str) -> Option<SettingValue> {
    match kind {
        "bool" => Some(SettingValue::Bool(raw == "1" || raw == "true")),
        "string" => Some(SettingValue::String(SharedString::from(raw.to_string()))),
        "int" => raw.parse::<i64>().ok().map(SettingValue::Int),
        "list" => {
            let v: serde_json::Value = serde_json::from_str(raw).ok()?;
            let arr = v.as_array()?;
            let items: Option<Vec<_>> = arr.iter().map(value_from_json).collect();
            Some(SettingValue::List(items?))
        }
        _ => None,
    }
}

fn value_to_json(v: &SettingValue) -> serde_json::Value {
    match v {
        SettingValue::Bool(b) => serde_json::json!({ "type": "bool", "value": *b }),
        SettingValue::String(s) => serde_json::json!({ "type": "string", "value": s.as_ref() }),
        SettingValue::Int(i) => serde_json::json!({ "type": "int", "value": *i }),
        SettingValue::List(items) => {
            let arr: Vec<serde_json::Value> = items.iter().map(value_to_json).collect();
            serde_json::json!({ "type": "list", "value": arr })
        }
    }
}

fn value_from_json(v: &serde_json::Value) -> Option<SettingValue> {
    let kind = v.get("type")?.as_str()?;
    let inner = v.get("value")?;
    match kind {
        "bool" => Some(SettingValue::Bool(inner.as_bool()?)),
        "string" => Some(SettingValue::String(SharedString::from(
            inner.as_str()?.to_string(),
        ))),
        "int" => Some(SettingValue::Int(inner.as_i64()?)),
        "list" => {
            let arr = inner.as_array()?;
            let items: Option<Vec<_>> = arr.iter().map(value_from_json).collect();
            Some(SettingValue::List(items?))
        }
        _ => None,
    }
}

fn with<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut Connection) -> Result<R>,
{
    with_connection(&CONN, DATABASE, f)
}
