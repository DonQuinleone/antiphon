use std::fmt;
use std::io;

use crate::frame::MAX_FRAME_BYTES;

#[derive(Debug)]
pub enum IpcError {
    Io(io::Error),
    Json(serde_json::Error),
    FrameTooLarge { length: usize },
    Protocol(String),
}

impl IpcError {
    pub fn is_timeout(&self) -> bool {
        matches!(
            self,
            IpcError::Io(error) if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            )
        )
    }
}

impl fmt::Display for IpcError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IpcError::Io(error) => {
                write!(out, "ipc i/o failed: {error}")
            }
            IpcError::Json(error) => {
                write!(out, "ipc frame is not valid JSON: {error}")
            }
            IpcError::FrameTooLarge { length } => write!(
                out,
                "ipc frame of {length} bytes exceeds the \
                 {MAX_FRAME_BYTES} byte limit"
            ),
            IpcError::Protocol(message) => {
                write!(out, "ipc protocol violation: {message}")
            }
        }
    }
}

impl std::error::Error for IpcError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            IpcError::Io(error) => Some(error),
            IpcError::Json(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for IpcError {
    fn from(error: io::Error) -> IpcError {
        IpcError::Io(error)
    }
}

impl From<serde_json::Error> for IpcError {
    fn from(error: serde_json::Error) -> IpcError {
        IpcError::Json(error)
    }
}
