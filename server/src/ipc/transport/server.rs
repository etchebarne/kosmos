use std::fs;
use std::io;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use super::connection;

pub(crate) fn run(socket_path: PathBuf, state: core::State) -> io::Result<()> {
    prepare_socket_path(&socket_path)?;

    let listener = UnixListener::bind(&socket_path)?;
    let state = Arc::new(Mutex::new(state));

    println!("kosmos server listening on {}", socket_path.display());

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let state = Arc::clone(&state);

                thread::spawn(move || {
                    if let Err(error) = connection::handle(stream, state) {
                        eprintln!("IPC connection failed: {error}");
                    }
                });
            }
            Err(error) => eprintln!("IPC accept failed: {error}"),
        }
    }

    Ok(())
}

fn prepare_socket_path(socket_path: &Path) -> io::Result<()> {
    if let Some(parent) = socket_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let metadata = match fs::symlink_metadata(socket_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };

    if !metadata.file_type().is_socket() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "IPC socket path {} already exists and is not a socket",
                socket_path.display()
            ),
        ));
    }

    match UnixStream::connect(socket_path) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            format!(
                "kosmos server is already listening on {}",
                socket_path.display()
            ),
        )),
        Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
            fs::remove_file(socket_path)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io::Error::new(
            error.kind(),
            format!(
                "could not inspect existing IPC socket {}: {error}",
                socket_path.display()
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn socket_path(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("kosmos-server-{}-{name}.sock", std::process::id()));

        let _ = fs::remove_file(&path);
        path
    }

    #[test]
    fn prepare_socket_path_removes_stale_socket() {
        let path = socket_path("stale");
        let listener = UnixListener::bind(&path).expect("test socket should bind");
        drop(listener);

        prepare_socket_path(&path).expect("stale socket should be removed");

        assert!(!path.exists());
    }

    #[test]
    fn prepare_socket_path_rejects_live_socket() {
        let path = socket_path("live");
        let listener = UnixListener::bind(&path).expect("test socket should bind");

        let error = prepare_socket_path(&path).expect_err("live socket should be rejected");

        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);

        drop(listener);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn prepare_socket_path_does_not_remove_non_socket_file() {
        let path = socket_path("file");
        fs::write(&path, b"not a socket").expect("test file should be written");

        let error = prepare_socket_path(&path).expect_err("plain file should be rejected");

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(path.exists());

        let _ = fs::remove_file(path);
    }
}
