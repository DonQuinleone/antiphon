use std::fmt;
use std::io;
use std::path::PathBuf;

use imap_client::client::tokio::ClientError;

#[derive(Debug)]
pub enum SyncError {
    Runtime {
        source: io::Error,
    },
    Connect {
        host: String,
        port: u16,
        source: Box<ClientError>,
    },
    Login {
        user: String,
        source: Box<ClientError>,
    },
    Imap {
        context: String,
        source: Box<ClientError>,
    },
    Smtp {
        host: String,
        source: lettre::transport::smtp::Error,
    },
    SmtpMessage {
        detail: String,
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
            Self::Runtime { source } => {
                write!(out, "starting the sync runtime: {source}")
            }
            Self::Connect { host, port, source } => {
                write!(out, "connecting to {host}:{port}: {source}")
            }
            Self::Login { user, source } => {
                write!(out, "logging in as {user}: {source}")
            }
            Self::Imap { context, source } => {
                write!(out, "{context}: {source}")
            }
            Self::Smtp { host, source } => {
                write!(out, "sending mail via {host}: {source}")
            }
            Self::SmtpMessage { detail } => {
                write!(out, "outgoing message: {detail}")
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
            Self::Runtime { source } => Some(source),
            Self::Connect { source, .. } => Some(source.as_ref()),
            Self::Login { source, .. } => Some(source.as_ref()),
            Self::Imap { source, .. } => Some(source.as_ref()),
            Self::Smtp { source, .. } => Some(source),
            Self::Io { source, .. } => Some(source),
            Self::Index { source } => Some(source),
            Self::NotmuchSpawn { source } => Some(source),
            Self::SmtpMessage { .. }
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
    ) -> impl FnOnce(ClientError) -> Self {
        let context = context.into();
        move |source| Self::Imap {
            context,
            source: Box::new(source),
        }
    }

    pub(crate) fn smtp(
        host: impl Into<String>,
    ) -> impl FnOnce(lettre::transport::smtp::Error) -> Self {
        let host = host.into();
        move |source| Self::Smtp { host, source }
    }
}
