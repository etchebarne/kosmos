use std::fs;
use std::io;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use super::connection;

pub(crate) fn run(socket_path: PathBuf, state: core::State) -> io::Result<()> {
    if let Some(parent) = socket_path.parent() {
        fs::create_dir_all(parent)?;
    }

    if socket_path.exists() {
        fs::remove_file(&socket_path)?;
    }

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
