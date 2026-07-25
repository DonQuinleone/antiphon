use std::time::Duration;

use imap_client::imap_types::core::Tag;
use tokio::time::timeout;

use crate::engine::SyncAccount;
use crate::error::SyncError;
use crate::session::ImapSession;

const INBOX: &str = "INBOX";
/// Bounds the DONE handshake so a dead connection can never
/// hang a shutdown or a re-issue.
const DONE_TIMEOUT: Duration = Duration::from_secs(5);

pub enum IdleWait {
    /// The server pushed an untagged update; the IDLE ended and
    /// the next [`IdleSession::wait`] issues a fresh one.
    Update,
    /// The window elapsed with the IDLE still standing.
    Quiet,
}

/// One IMAP connection parked in IDLE on INBOX. The watcher
/// polls [`wait`](IdleSession::wait) in short windows so it can
/// observe shutdown between them, and calls
/// [`refresh`](IdleSession::refresh) to stay inside the RFC
/// 2177 29-minute re-issue deadline.
pub struct IdleSession {
    session: ImapSession,
    tag: Option<Tag<'static>>,
}

impl IdleSession {
    pub fn connect(account: &SyncAccount) -> Result<Self, SyncError> {
        let mut session = ImapSession::connect(account)?;
        session
            .examine(INBOX)
            .map_err(SyncError::imap("examining INBOX"))?;
        Ok(Self { session, tag: None })
    }

    pub fn supports_idle(&self) -> bool {
        self.session.client.state.ext_idle_supported()
    }

    /// Issues IDLE if none stands, then watches the connection
    /// for one window. Dropping the watch mid-window is safe:
    /// the protocol state lives on the session, so the next
    /// call resumes where this one stopped.
    pub fn wait(
        &mut self,
        window: Duration,
    ) -> Result<IdleWait, SyncError> {
        let tag = match self.tag.take() {
            Some(tag) => tag,
            None => self.session.client.enqueue_idle(),
        };
        let watched = self.session.runtime.block_on(timeout(
            window,
            self.session.client.idle(tag.clone()),
        ));
        match watched {
            Err(_elapsed) => {
                self.tag = Some(tag);
                Ok(IdleWait::Quiet)
            }
            Ok(Ok(())) => Ok(IdleWait::Update),
            Ok(Err(error)) => Err(SyncError::Idle {
                detail: error.to_string(),
            }),
        }
    }

    /// Ends the standing IDLE with DONE so the next wait issues
    /// a fresh one before the server's re-issue deadline.
    pub fn refresh(&mut self) -> Result<(), SyncError> {
        let Some(tag) = self.tag.take() else {
            return Ok(());
        };
        let done = self.session.runtime.block_on(timeout(
            DONE_TIMEOUT,
            self.session.client.idle_done(tag),
        ));
        match done {
            Err(_elapsed) => Err(SyncError::Idle {
                detail: String::from(
                    "the server never acknowledged DONE",
                ),
            }),
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(SyncError::Idle {
                detail: error.to_string(),
            }),
        }
    }

    pub fn close(mut self) {
        let _ = self.refresh();
        self.session.logout();
    }
}
