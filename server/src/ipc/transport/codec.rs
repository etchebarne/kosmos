use std::io::{self, BufRead, Read, Write};

use crate::ipc::messages::envelope::ServerMessage;

const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
const MAX_RESPONSE_FRAME_BYTES: usize = 64 * 1024 * 1024;

struct FrameBuffer {
    bytes: Vec<u8>,
    limit: usize,
}

impl Write for FrameBuffer {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if buffer.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("IPC response exceeds the {}-byte limit", self.limit),
            ));
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

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
    let frame = serialize_message(message, MAX_RESPONSE_FRAME_BYTES)?;
    writer.write_all(&frame)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

fn serialize_message(message: &ServerMessage, limit: usize) -> io::Result<Vec<u8>> {
    let mut frame = FrameBuffer {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut frame, message).map_err(io::Error::other)?;
    Ok(frame.bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[derive(Default)]
    struct WriteCounter {
        calls: usize,
        bytes: usize,
    }

    impl Write for WriteCounter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.calls += 1;
            self.bytes += buffer.len();
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

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

    #[test]
    fn serializes_response_before_writing_to_the_transport() {
        let mut writer = WriteCounter::default();
        let message = ServerMessage::ok(1, vec!["completion"; 10_000]);

        write_message(&mut writer, &message).expect("response should serialize");

        assert_eq!(writer.calls, 2);
        assert!(writer.bytes > 100_000);
    }

    #[test]
    fn rejects_responses_above_the_outbound_limit() {
        let message = ServerMessage::ok(1, "completion".repeat(1_000));

        assert!(serialize_message(&message, 128).is_err());
    }
}
