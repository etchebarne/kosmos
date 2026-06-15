mod ipc;

use std::io;
use std::path::PathBuf;

fn main() -> io::Result<()> {
    let store = core::persistence::StateStore::open(database_path()?).map_err(io::Error::other)?;
    let state = store.load().map_err(io::Error::other)?;

    ipc::run(socket_path(), state, store)
}

fn socket_path() -> PathBuf {
    if let Some(socket_path) = std::env::var_os("KOSMOS_SOCKET") {
        return socket_path.into();
    }

    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("kosmos")
        .join("server.sock")
}

fn database_path() -> io::Result<PathBuf> {
    if let Some(database_path) = std::env::var_os("KOSMOS_DATABASE") {
        return Ok(database_path.into());
    }

    Ok(config_dir()?.join("state.sqlite3"))
}

fn config_dir() -> io::Result<PathBuf> {
    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(config_home).join("kosmos"));
    }

    let home = std::env::var_os("HOME").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "HOME must be set when XDG_CONFIG_HOME is not set",
        )
    })?;

    Ok(PathBuf::from(home).join(".config").join("kosmos"))
}
