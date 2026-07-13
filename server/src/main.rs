use std::io;
use std::path::PathBuf;

fn main() -> io::Result<()> {
    terminate_with_parent()?;
    let store = core::persistence::StateStore::open(database_path()?).map_err(io::Error::other)?;
    let language_server_manager = language_server_paths().and_then(|paths| {
        core::language_servers::LanguageServerManager::open(paths, store.clone())
            .map_err(io::Error::other)
    });
    let mut state = store.load().map_err(io::Error::other)?;
    match language_server_manager {
        Ok(manager) => state.attach_language_server_manager(manager),
        Err(error) => eprintln!("language server manager unavailable: {error}"),
    }
    match formatter_paths().and_then(|paths| {
        core::formatters::FormatterManager::open(paths, store.clone()).map_err(io::Error::other)
    }) {
        Ok(manager) => state.attach_formatter_manager(manager),
        Err(error) => eprintln!("formatter manager unavailable: {error}"),
    }

    kosmos_server::ipc::run(socket_path(), core::Application::new(state, store))
}

fn terminate_with_parent() -> io::Result<()> {
    let Some(parent_pid) = std::env::var_os("KOSMOS_PARENT_PID") else {
        return Ok(());
    };
    let parent_pid = parent_pid
        .to_string_lossy()
        .parse::<libc::pid_t>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid KOSMOS_PARENT_PID"))?;

    // The parent can exit before the death signal is installed, so verify it again afterwards.
    if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) } != 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::getppid() } != parent_pid {
        return Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "Kosmos desktop exited before the server started",
        ));
    }

    Ok(())
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

fn data_dir() -> io::Result<PathBuf> {
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(data_home).join("kosmos"));
    }

    Ok(home_dir()?.join(".local").join("share").join("kosmos"))
}

fn cache_dir() -> io::Result<PathBuf> {
    if let Some(cache_home) = std::env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(cache_home).join("kosmos"));
    }

    Ok(home_dir()?.join(".cache").join("kosmos"))
}

fn language_server_paths() -> io::Result<core::language_servers::LanguageServerPaths> {
    Ok(core::language_servers::LanguageServerPaths::new(
        data_dir()?.join("language-servers"),
        cache_dir()?.join("language-server-downloads"),
    ))
}

fn formatter_paths() -> io::Result<core::formatters::FormatterPaths> {
    Ok(core::formatters::FormatterPaths::new(
        data_dir()?.join("formatters"),
        cache_dir()?.join("formatter-downloads"),
    ))
}

fn home_dir() -> io::Result<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "HOME must be set when an XDG directory is not set",
        )
    })
}
