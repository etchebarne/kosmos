mod ipc;

use std::path::PathBuf;

fn main() -> std::io::Result<()> {
    ipc::run(socket_path(), core::State::new())
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
