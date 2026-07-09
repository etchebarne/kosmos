use std::io::{self, BufRead, Read, Write};

use crate::ipc::messages::envelope::ServerMessage;

const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;

pub(crate) fn read_frame(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut frame = String::new();
    let bytes_read = reader
        .take((MAX_FRAME_BYTES + 3) as u64)
        .read_line(&mut frame)?;

    if bytes_read == 0 {
        return Ok(None);
    }

    while frame.ends_with(['\n', '\r']) {
        frame.pop();
    }

    if frame.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("IPC frame exceeds the {MAX_FRAME_BYTES}-byte limit"),
        ));
    }

    Ok(Some(frame))
}

pub(crate) fn write_message(writer: &mut impl Write, message: &ServerMessage) -> io::Result<()> {
    serde_json::to_writer(&mut *writer, message).map_err(io::Error::other)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn rejects_oversized_frames() {
        let mut frame = vec![b'x'; MAX_FRAME_BYTES + 1];
        frame.push(b'\n');
        let mut reader = Cursor::new(frame);

        let error = read_frame(&mut reader).expect_err("oversized frames should be rejected");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn accepts_a_maximum_sized_terminated_frame() {
        let mut frame = vec![b'x'; MAX_FRAME_BYTES];
        frame.extend_from_slice(b"\r\n");
        let mut reader = Cursor::new(frame);

        let frame = read_frame(&mut reader)
            .expect("maximum frame should be readable")
            .expect("frame should exist");

        assert_eq!(frame.len(), MAX_FRAME_BYTES);
    }
}
