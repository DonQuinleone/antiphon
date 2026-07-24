use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use crate::IpcError;
use crate::frame::{read_frame, write_frame};
use crate::protocol::{Event, Request, Response};

pub struct IpcClient {
    stream: UnixStream,
}

impl IpcClient {
    pub fn connect(path: &Path) -> io::Result<IpcClient> {
        let stream = UnixStream::connect(path)?;
        Ok(IpcClient { stream })
    }

    /// A busy daemon (mid initial sync) cannot answer; a
    /// bounded wait lets callers report that instead of
    /// hanging.
    pub fn set_read_timeout(
        &self,
        timeout: Duration,
    ) -> io::Result<()> {
        self.stream.set_read_timeout(Some(timeout))
    }

    pub fn request(
        &mut self,
        request: &Request,
    ) -> Result<Response, IpcError> {
        write_frame(&mut self.stream, request)?;
        read_frame(&mut self.stream)
    }

    pub fn subscribe(mut self) -> Result<EventStream, IpcError> {
        match self.request(&Request::Subscribe)? {
            Response::Ack => Ok(EventStream {
                stream: self.stream,
            }),
            Response::Error(message) => {
                Err(IpcError::Protocol(message))
            }
            other => Err(IpcError::Protocol(format!(
                "expected Ack to Subscribe, got {other:?}"
            ))),
        }
    }
}

pub struct EventStream {
    stream: UnixStream,
}

impl Iterator for EventStream {
    type Item = Result<Event, IpcError>;

    fn next(&mut self) -> Option<Self::Item> {
        match read_frame(&mut self.stream) {
            Err(IpcError::Io(error))
                if error.kind() == io::ErrorKind::UnexpectedEof =>
            {
                None
            }
            result => Some(result),
        }
    }
}
