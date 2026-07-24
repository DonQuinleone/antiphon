use std::fmt;
use std::io;
use std::path::PathBuf;

#[derive(Debug)]
pub enum SyncError {
    Connect {
        host: String,
        port: u16,
        source: io::Error,
    },
    InvalidHost {
        host: String,
    },
    Tls {
        host: String,
        source: rustls::Error,
    },
    Login {
        user: String,
        source: imap::Error,
    },
    Imap {
        context: String,
        source: imap::Error,
    },
    Io {
        path: PathBuf,
        source: io::Error,
    },
    State {
        path: PathBuf,
        line: usize,
        detail: String,
    },
    Folder {
        folder: String,
        detail: String,
    },
    Index {
        source: antiphon_store::SearchError,
    },
    Notmuch {
        detail: String,
    },
    NotmuchSpawn {
        source: io::Error,
    },
}

impl fmt::Display for SyncError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect { host, port, source } => {
                write!(out, "connecting to {host}:{port}: {source}")
            }
            Self::InvalidHost { host } => {
                write!(out, "`{host}` is not a valid host name")
            }
            Self::Tls { host, source } => {
                write!(out, "tls handshake with {host}: {source}")
            }
            Self::Login { user, source } => {
                write!(out, "logging in as {user}: {source}")
            }
            Self::Imap { context, source } => {
                write!(out, "{context}: {source}")
            }
            Self::Io { path, source } => {
                write!(out, "{}: {source}", path.display())
            }
            Self::State { path, line, detail } => write!(
                out,
                "sync state {} line {line}: {detail}",
                path.display()
            ),
            Self::Folder { folder, detail } => {
                write!(out, "folder {folder}: {detail}")
            }
            Self::Index { source } => {
                write!(out, "searching the store index: {source}")
            }
            Self::Notmuch { detail } => {
                write!(out, "notmuch new failed: {detail}")
            }
            Self::NotmuchSpawn { source } => {
                write!(out, "running notmuch new: {source}")
            }
        }
    }
}

impl std::error::Error for SyncError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Connect { source, .. } => Some(source),
            Self::Tls { source, .. } => Some(source),
            Self::Login { source, .. } => Some(source),
            Self::Imap { source, .. } => Some(source),
            Self::Io { source, .. } => Some(source),
            Self::Index { source } => Some(source),
            Self::NotmuchSpawn { source } => Some(source),
            Self::InvalidHost { .. }
            | Self::State { .. }
            | Self::Folder { .. }
            | Self::Notmuch { .. } => None,
        }
    }
}

impl SyncError {
    pub(crate) fn io(
        path: impl Into<PathBuf>,
    ) -> impl FnOnce(io::Error) -> Self {
        let path = path.into();
        move |source| Self::Io { path, source }
    }

    pub(crate) fn imap(
        context: impl Into<String>,
    ) -> impl FnOnce(imap::Error) -> Self {
        let context = context.into();
        move |source| Self::Imap { context, source }
    }
}
