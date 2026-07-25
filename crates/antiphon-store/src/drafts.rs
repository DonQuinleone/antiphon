use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::layout::StoreLayout;
use crate::spool::{Spool, SpoolError};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftEnvelope {
    pub account: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedDraft {
    pub id: u64,
    pub account: String,
    pub message_path: PathBuf,
}

/// Drafts awaiting filing on the server, spooled exactly like
/// outgoing mail: the daemon appends each to its account's
/// server drafts folder and delivers a local twin. A draft
/// stays spooled until that append succeeds.
pub struct DraftSpool {
    spool: Spool,
}

impl DraftSpool {
    pub fn open(layout: &StoreLayout) -> DraftSpool {
        DraftSpool {
            spool: Spool::new(layout.draft_outbox_dir()),
        }
    }

    pub fn enqueue(
        &self,
        envelope: &DraftEnvelope,
        raw_message: &[u8],
    ) -> Result<QueuedDraft, SpoolError> {
        let (id, message_path) =
            self.spool.enqueue(envelope, raw_message)?;
        Ok(QueuedDraft {
            id,
            account: envelope.account.clone(),
            message_path,
        })
    }

    pub fn pending(&self) -> Result<Vec<QueuedDraft>, SpoolError> {
        Ok(self
            .spool
            .pending()?
            .into_iter()
            .map(|(id, envelope, message_path)| {
                let DraftEnvelope { account } = envelope;
                QueuedDraft {
                    id,
                    account,
                    message_path,
                }
            })
            .collect())
    }

    pub fn pending_for(
        &self,
        account: &str,
    ) -> Result<Vec<QueuedDraft>, SpoolError> {
        Ok(self
            .pending()?
            .into_iter()
            .filter(|draft| draft.account == account)
            .collect())
    }

    pub fn remove(&self, id: u64) -> Result<(), SpoolError> {
        self.spool.remove(id)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn spool_in(dir: &tempfile::TempDir) -> (StoreLayout, DraftSpool) {
        let layout = StoreLayout::new(dir.path().join("store"));
        let spool = DraftSpool::open(&layout);
        (layout, spool)
    }

    fn envelope(account: &str) -> DraftEnvelope {
        DraftEnvelope {
            account: account.to_string(),
        }
    }

    #[test]
    fn drafts_round_trip_without_a_store_init() {
        let dir = tempfile::tempdir().unwrap();
        let (_, spool) = spool_in(&dir);
        assert!(spool.pending().unwrap().is_empty());

        let queued = spool
            .enqueue(&envelope("personal"), b"Subject: kept")
            .unwrap();
        assert_eq!(queued.id, 1);
        assert_eq!(queued.account, "personal");

        let pending = spool.pending().unwrap();
        assert_eq!(pending.len(), 1);
        let raw = fs::read(&pending[0].message_path).unwrap();
        assert_eq!(raw, b"Subject: kept");

        spool.remove(queued.id).unwrap();
        assert!(spool.pending().unwrap().is_empty());
    }

    #[test]
    fn pending_for_selects_one_account() {
        let dir = tempfile::tempdir().unwrap();
        let (_, spool) = spool_in(&dir);
        spool.enqueue(&envelope("personal"), b"a").unwrap();
        spool.enqueue(&envelope("work"), b"b").unwrap();
        spool.enqueue(&envelope("personal"), b"c").unwrap();

        let personal = spool.pending_for("personal").unwrap();
        assert_eq!(personal.len(), 2);
        assert_eq!((personal[0].id, personal[1].id), (1, 3));
        assert_eq!(spool.pending_for("work").unwrap().len(), 1);
        assert!(spool.pending_for("absent").unwrap().is_empty());
    }

    #[test]
    fn the_spool_lives_beside_the_outbox() {
        let dir = tempfile::tempdir().unwrap();
        let (layout, spool) = spool_in(&dir);
        let queued =
            spool.enqueue(&envelope("personal"), b"x").unwrap();
        assert!(
            queued.message_path.starts_with(layout.draft_outbox_dir())
        );
    }
}
