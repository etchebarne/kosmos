use std::io::{self, BufRead, Write};

use crate::ipc::messages::envelope::ServerMessage;

pub(crate) fn read_frame(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut frame = String::new();
    let bytes_read = reader.read_line(&mut frame)?;

    if bytes_read == 0 {
        return Ok(None);
    }

    while frame.ends_with(['\n', '\r']) {
        frame.pop();
    }

    Ok(Some(frame))
}

pub(crate) fn write_message(writer: &mut impl Write, message: &ServerMessage) -> io::Result<()> {
    serde_json::to_writer(&mut *writer, message).map_err(io::Error::other)?;
    writer.write_all(b"\n")?;
    writer.flush()
}
