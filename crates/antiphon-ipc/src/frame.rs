use std::io::{Read, Write};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::IpcError;

pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

pub fn write_frame<T, W>(out: &mut W, value: &T) -> Result<(), IpcError>
where
    T: Serialize,
    W: Write,
{
    let body = serde_json::to_vec(value)?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(IpcError::FrameTooLarge { length: body.len() });
    }
    out.write_all(&(body.len() as u32).to_be_bytes())?;
    out.write_all(&body)?;
    out.flush()?;
    Ok(())
}

pub fn read_frame<T, R>(input: &mut R) -> Result<T, IpcError>
where
    T: DeserializeOwned,
    R: Read,
{
    let mut prefix = [0u8; size_of::<u32>()];
    input.read_exact(&mut prefix)?;
    let length = u32::from_be_bytes(prefix) as usize;
    if length > MAX_FRAME_BYTES {
        return Err(IpcError::FrameTooLarge { length });
    }
    let mut body = vec![0u8; length];
    input.read_exact(&mut body)?;
    Ok(serde_json::from_slice(&body)?)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    struct Trickle<R> {
        inner: R,
    }

    impl<R: Read> Read for Trickle<R> {
        fn read(
            &mut self,
            buffer: &mut [u8],
        ) -> std::io::Result<usize> {
            let step = buffer.len().min(1);
            self.inner.read(&mut buffer[..step])
        }
    }

    #[test]
    fn a_value_survives_the_wire() {
        let mut wire = Vec::new();
        write_frame(&mut wire, &vec!["one", "two"]).unwrap();
        let back: Vec<String> =
            read_frame(&mut Cursor::new(wire)).unwrap();
        assert_eq!(back, ["one", "two"]);
    }

    #[test]
    fn torn_reads_still_deliver_the_frame() {
        let mut wire = Vec::new();
        write_frame(&mut wire, &"drip fed").unwrap();
        let mut trickle = Trickle {
            inner: Cursor::new(wire),
        };
        let back: String = read_frame(&mut trickle).unwrap();
        assert_eq!(back, "drip fed");
    }

    #[test]
    fn an_oversize_frame_is_rejected_on_read() {
        let oversize = (MAX_FRAME_BYTES as u32) + 1;
        let wire = oversize.to_be_bytes().to_vec();
        let error = read_frame::<String, _>(&mut Cursor::new(wire))
            .unwrap_err();
        let expected = oversize as usize;
        assert!(matches!(
            error,
            IpcError::FrameTooLarge { length } if length == expected
        ));
    }

    #[test]
    fn an_oversize_frame_is_refused_on_write() {
        let huge = "x".repeat(MAX_FRAME_BYTES);
        let mut wire = Vec::new();
        let error = write_frame(&mut wire, &huge).unwrap_err();
        assert!(matches!(error, IpcError::FrameTooLarge { .. }));
        assert!(wire.is_empty());
    }

    #[test]
    fn garbage_after_the_json_body_is_rejected() {
        let body = b"true rubbish";
        let mut wire = (body.len() as u32).to_be_bytes().to_vec();
        wire.extend_from_slice(body);
        let error =
            read_frame::<bool, _>(&mut Cursor::new(wire)).unwrap_err();
        assert!(matches!(error, IpcError::Json(_)));
    }

    #[test]
    fn a_frame_never_bleeds_into_the_next() {
        let mut wire = Vec::new();
        write_frame(&mut wire, &"first").unwrap();
        write_frame(&mut wire, &"second").unwrap();
        let mut cursor = Cursor::new(wire);
        let first: String = read_frame(&mut cursor).unwrap();
        let second: String = read_frame(&mut cursor).unwrap();
        assert_eq!(
            (first.as_str(), second.as_str()),
            ("first", "second")
        );
        let error = read_frame::<String, _>(&mut cursor).unwrap_err();
        assert!(matches!(error, IpcError::Io(_)));
    }
}
