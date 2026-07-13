use std::env;
use std::io;

fn main() -> io::Result<()> {
    let path = env::args_os().nth(1).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: export-ipc-schema <output-path>",
        )
    })?;
    kosmos_server::ipc::schema::export(path)
}
