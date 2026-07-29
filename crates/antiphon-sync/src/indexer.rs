use std::sync::mpsc::{self, Sender};

use antiphon_store::StoreLayout;

use crate::error::SyncError;
use crate::notmuch::run_notmuch_new;
use crate::tagging::retag_folders;

/// Indexing overlaps the network: delivered batches become
/// searchable while the next batch downloads. One thread, so
/// notmuch keeps its single writer, and every folder worker of
/// an account shares it through a cloned nudge channel.
pub(crate) struct Indexer {
    sender: Option<Sender<()>>,
    handle: std::thread::JoinHandle<Result<(), SyncError>>,
}

impl Indexer {
    pub(crate) fn start(
        layout: &StoreLayout,
        account: &str,
    ) -> Indexer {
        let config = layout.notmuch_config_path();
        let account = account.to_owned();
        let (sender, receiver) = mpsc::channel::<()>();
        let handle = std::thread::spawn(move || {
            while receiver.recv().is_ok() {
                while receiver.try_recv().is_ok() {}
                run_notmuch_new(&config)?;
                retag_folders(&config, &account)?;
            }
            Ok(())
        });
        Indexer {
            sender: Some(sender),
            handle,
        }
    }

    /// One nudge channel per folder worker: the sender is not
    /// `Sync`, so workers own a clone rather than share a
    /// borrow of the indexer.
    pub(crate) fn nudge_channel(&self) -> IndexNudge {
        IndexNudge {
            sender: self.sender.clone(),
        }
    }

    pub(crate) fn finish(mut self) -> Result<(), SyncError> {
        self.sender.take();
        self.handle.join().unwrap_or_else(|_| {
            Err(SyncError::Notmuch {
                detail: "the indexer thread panicked".into(),
            })
        })
    }
}

/// A folder worker's handle to the shared indexer. Dropping it
/// is harmless; the indexer stops when every clone and the
/// owning `Indexer` have released their senders.
pub(crate) struct IndexNudge {
    sender: Option<Sender<()>>,
}

impl IndexNudge {
    pub(crate) fn nudge(&self) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(());
        }
    }
}
