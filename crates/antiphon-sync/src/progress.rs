use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use antiphon_store::StoreLayout;
use serde::{Deserialize, Serialize};

const PROGRESS_FILE: &str = "progress.json";

/// Written by the daemon as it works and read by the client
/// from disk, so a busy single-threaded daemon still reports
/// progress without answering IPC.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncProgress {
    pub state: SyncState,
    pub account: String,
    pub folder: String,
    pub fetched: usize,
    pub total: usize,
    pub updated_unix: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    Syncing,
    Idle,
}

impl SyncProgress {
    pub fn syncing(
        account: &str,
        folder: &str,
        fetched: usize,
        total: usize,
    ) -> SyncProgress {
        SyncProgress {
            state: SyncState::Syncing,
            account: account.to_owned(),
            folder: folder.to_owned(),
            fetched,
            total,
            updated_unix: now_unix(),
        }
    }

    pub fn idle() -> SyncProgress {
        SyncProgress {
            state: SyncState::Idle,
            account: String::new(),
            folder: String::new(),
            fetched: 0,
            total: 0,
            updated_unix: now_unix(),
        }
    }
}

pub fn write_progress(layout: &StoreLayout, progress: &SyncProgress) {
    let path = progress_path(layout);
    let Ok(json) = serde_json::to_vec(progress) else {
        return;
    };
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, json).is_err() {
        return;
    }
    let _ = std::fs::rename(&tmp, &path);
}

pub fn read_progress(layout: &StoreLayout) -> Option<SyncProgress> {
    let bytes = std::fs::read(progress_path(layout)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn progress_path(layout: &StoreLayout) -> PathBuf {
    layout.sync_state_dir().join(PROGRESS_FILE)
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_round_trips_through_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let layout = StoreLayout::new(dir.path().join("store"));
        layout.init().unwrap();
        assert_eq!(read_progress(&layout), None);
        let progress =
            SyncProgress::syncing("personal", "inbox", 200, 3400);
        write_progress(&layout, &progress);
        assert_eq!(read_progress(&layout), Some(progress));
        write_progress(&layout, &SyncProgress::idle());
        let read = read_progress(&layout).unwrap();
        assert_eq!(read.state, SyncState::Idle);
    }
}
