use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::layout::StoreLayout;
use crate::spool::{Spool, SpoolError};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub account: String,
    pub from: String,
    pub recipients: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedMessage {
    pub id: u64,
    pub envelope: Envelope,
    pub message_path: PathBuf,
}

pub struct Outbox {
    spool: Spool,
}

impl Outbox {
    pub fn open(layout: &StoreLayout) -> Outbox {
        Outbox {
            spool: Spool::new(layout.outbox_dir()),
        }
    }

    /// The message is durable once enqueue returns: both files
    /// are fsynced, envelope last, and a message without its
    /// envelope is ignored by pending() as a torn enqueue.
    pub fn enqueue(
        &self,
        envelope: &Envelope,
        raw_message: &[u8],
    ) -> Result<QueuedMessage, SpoolError> {
        let (id, message_path) =
            self.spool.enqueue(envelope, raw_message)?;
        Ok(QueuedMessage {
            id,
            envelope: envelope.clone(),
            message_path,
        })
    }

    pub fn pending(&self) -> Result<Vec<QueuedMessage>, SpoolError> {
        Ok(self
            .spool
            .pending()?
            .into_iter()
            .map(|(id, envelope, message_path)| QueuedMessage {
                id,
                envelope,
                message_path,
            })
            .collect())
    }

    pub fn remove(&self, id: u64) -> Result<(), SpoolError> {
        self.spool.remove(id)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::spool::ENVELOPE_EXT;

    use super::*;

    fn outbox_in(dir: &tempfile::TempDir) -> Outbox {
        let layout = StoreLayout::new(dir.path().join("store"));
        layout.init().unwrap();
        Outbox::open(&layout)
    }

    fn envelope() -> Envelope {
        Envelope {
            account: "personal".to_string(),
            from: "quin@example.com".to_string(),
            recipients: vec!["mara@example.com".to_string()],
        }
    }

    #[test]
    fn enqueue_pending_remove_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let outbox = outbox_in(&dir);
        let first =
            outbox.enqueue(&envelope(), b"Subject: one").unwrap();
        let second =
            outbox.enqueue(&envelope(), b"Subject: two").unwrap();
        assert_eq!((first.id, second.id), (1, 2));

        let pending = outbox.pending().unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].id, 1);
        assert_eq!(pending[0].envelope, envelope());
        let raw = fs::read(&pending[1].message_path).unwrap();
        assert_eq!(raw, b"Subject: two");

        outbox.remove(1).unwrap();
        outbox.remove(1).unwrap();
        let left = outbox.pending().unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, 2);
    }

    #[test]
    fn ids_continue_past_removals() {
        let dir = tempfile::tempdir().unwrap();
        let outbox = outbox_in(&dir);
        outbox.enqueue(&envelope(), b"a").unwrap();
        let second = outbox.enqueue(&envelope(), b"b").unwrap();
        outbox.remove(1).unwrap();
        let third = outbox.enqueue(&envelope(), b"c").unwrap();
        assert_eq!((second.id, third.id), (2, 3));
    }

    #[test]
    fn torn_enqueue_is_invisible_to_pending() {
        let dir = tempfile::tempdir().unwrap();
        let outbox = outbox_in(&dir);
        let queued = outbox.enqueue(&envelope(), b"x").unwrap();
        fs::remove_file(
            queued.message_path.with_extension(ENVELOPE_EXT),
        )
        .unwrap();
        assert!(outbox.pending().unwrap().is_empty());
    }
}
