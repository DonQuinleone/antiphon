use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::layout::StoreLayout;

const MESSAGE_EXT: &str = "eml";
const ENVELOPE_EXT: &str = "json";

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

#[derive(Debug)]
pub enum OutboxError {
    Io { path: PathBuf, source: io::Error },
    Envelope { path: PathBuf, detail: String },
}

impl fmt::Display for OutboxError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(out, "{}: {source}", path.display())
            }
            Self::Envelope { path, detail } => {
                write!(out, "{}: {detail}", path.display())
            }
        }
    }
}

impl std::error::Error for OutboxError {}

pub struct Outbox {
    dir: PathBuf,
}

impl Outbox {
    pub fn open(layout: &StoreLayout) -> Outbox {
        Outbox {
            dir: layout.outbox_dir(),
        }
    }

    /// The message is durable once enqueue returns: both files
    /// are fsynced, envelope last, and a message without its
    /// envelope is ignored by pending() as a torn enqueue.
    pub fn enqueue(
        &self,
        envelope: &Envelope,
        raw_message: &[u8],
    ) -> Result<QueuedMessage, OutboxError> {
        let id = self.next_id()?;
        let message_path = self.path_for(id, MESSAGE_EXT);
        write_synced(&message_path, raw_message)?;
        let json =
            serde_json::to_vec(envelope).expect("envelope serialises");
        write_synced(&self.path_for(id, ENVELOPE_EXT), &json)?;
        Ok(QueuedMessage {
            id,
            envelope: envelope.clone(),
            message_path,
        })
    }

    pub fn pending(&self) -> Result<Vec<QueuedMessage>, OutboxError> {
        let entries = fs::read_dir(&self.dir).map_err(|source| {
            OutboxError::Io {
                path: self.dir.clone(),
                source,
            }
        })?;
        let mut ids: Vec<u64> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| queued_id(&entry.path()))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids.into_iter()
            .filter_map(|id| self.load(id).transpose())
            .collect()
    }

    pub fn remove(&self, id: u64) -> Result<(), OutboxError> {
        for ext in [ENVELOPE_EXT, MESSAGE_EXT] {
            let path = self.path_for(id, ext);
            let Err(source) = fs::remove_file(&path) else {
                continue;
            };
            if source.kind() == io::ErrorKind::NotFound {
                continue;
            }
            return Err(OutboxError::Io { path, source });
        }
        Ok(())
    }

    fn load(
        &self,
        id: u64,
    ) -> Result<Option<QueuedMessage>, OutboxError> {
        let envelope_path = self.path_for(id, ENVELOPE_EXT);
        let message_path = self.path_for(id, MESSAGE_EXT);
        if !envelope_path.exists() || !message_path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&envelope_path).map_err(|source| {
            OutboxError::Io {
                path: envelope_path.clone(),
                source,
            }
        })?;
        let envelope =
            serde_json::from_slice(&bytes).map_err(|error| {
                OutboxError::Envelope {
                    path: envelope_path,
                    detail: error.to_string(),
                }
            })?;
        Ok(Some(QueuedMessage {
            id,
            envelope,
            message_path,
        }))
    }

    fn next_id(&self) -> Result<u64, OutboxError> {
        let entries = fs::read_dir(&self.dir).map_err(|source| {
            OutboxError::Io {
                path: self.dir.clone(),
                source,
            }
        })?;
        let highest = entries
            .filter_map(Result::ok)
            .filter_map(|entry| queued_id(&entry.path()))
            .max()
            .unwrap_or(0);
        Ok(highest + 1)
    }

    fn path_for(&self, id: u64, ext: &str) -> PathBuf {
        self.dir.join(format!("{id:016}.{ext}"))
    }
}

fn queued_id(path: &Path) -> Option<u64> {
    path.file_stem()?.to_str()?.parse().ok()
}

fn write_synced(path: &Path, bytes: &[u8]) -> Result<(), OutboxError> {
    let wrap = |source| OutboxError::Io {
        path: path.to_path_buf(),
        source,
    };
    let mut file = fs::File::create(path).map_err(wrap)?;
    io::Write::write_all(&mut file, bytes).map_err(wrap)?;
    file.sync_data().map_err(wrap)
}

#[cfg(test)]
mod tests {
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
