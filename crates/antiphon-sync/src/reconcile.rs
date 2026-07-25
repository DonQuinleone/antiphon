use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::SyncError;
use crate::maildir::MaildirFolder;

/// A full-folder UID listing costs a round trip per folder, so
/// each folder is swept at most once per this many seconds; new
/// mail still arrives on every pass.
const SWEEP_INTERVAL_SECS: u64 = 15 * 60;

pub(crate) fn sweep_due(last_sweep_unix: u64, now_unix: u64) -> bool {
    now_unix.saturating_sub(last_sweep_unix) >= SWEEP_INTERVAL_SECS
}

/// Deletes every locally delivered message whose UID the server
/// no longer lists, returning how many vanished. Files without
/// a UID marker were never delivered by the engine and are left
/// alone.
pub(crate) fn remove_vanished(
    maildir: &MaildirFolder,
    server: &HashSet<u32>,
) -> Result<usize, SyncError> {
    let local =
        maildir.scan().map_err(SyncError::io(maildir.root()))?;
    let mut removed = 0;
    for message in local {
        if server.contains(&message.uid) {
            continue;
        }
        maildir
            .remove(&message)
            .map_err(SyncError::io(maildir.root()))?;
        removed += 1;
    }
    Ok(removed)
}

pub(crate) fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn a_sweep_is_due_only_after_the_interval() {
        let now = 1_700_000_000;
        assert!(sweep_due(0, now));
        assert!(sweep_due(now - SWEEP_INTERVAL_SECS, now));
        assert!(!sweep_due(now - SWEEP_INTERVAL_SECS + 1, now));
        assert!(!sweep_due(now, now));
    }

    #[test]
    fn a_clock_step_backwards_never_underflows() {
        assert!(!sweep_due(1_700_000_000, 1_600_000_000));
    }

    #[test]
    fn vanished_messages_are_removed_and_kept_ones_spared() {
        let dir = tempfile::tempdir().unwrap();
        let maildir = MaildirFolder::new(dir.path().to_path_buf());
        maildir.ensure().unwrap();
        maildir.deliver(1, false, b"kept unseen").unwrap();
        maildir.deliver(2, true, b"kept seen").unwrap();
        maildir.deliver(3, true, b"vanished").unwrap();
        let server: HashSet<u32> = [1, 2].into_iter().collect();
        let removed = remove_vanished(&maildir, &server).unwrap();
        assert_eq!(removed, 1);
        let mut uids: Vec<u32> = maildir
            .scan()
            .unwrap()
            .into_iter()
            .map(|message| message.uid)
            .collect();
        uids.sort_unstable();
        assert_eq!(uids, [1, 2]);
    }

    #[test]
    fn an_empty_server_set_clears_delivered_mail_only() {
        let dir = tempfile::tempdir().unwrap();
        let maildir = MaildirFolder::new(dir.path().to_path_buf());
        maildir.ensure().unwrap();
        maildir.deliver(7, true, b"vanished").unwrap();
        let foreign = dir.path().join("cur/keep:2,S");
        fs::write(&foreign, b"not ours").unwrap();
        let removed =
            remove_vanished(&maildir, &HashSet::new()).unwrap();
        assert_eq!(removed, 1);
        assert!(foreign.exists());
        assert!(maildir.scan().unwrap().is_empty());
    }

    #[test]
    fn nothing_vanishes_when_the_server_lists_everything() {
        let dir = tempfile::tempdir().unwrap();
        let maildir = MaildirFolder::new(dir.path().to_path_buf());
        maildir.ensure().unwrap();
        maildir.deliver(1, false, b"one").unwrap();
        maildir.deliver(2, false, b"two").unwrap();
        let server: HashSet<u32> = [1, 2, 3].into_iter().collect();
        let removed = remove_vanished(&maildir, &server).unwrap();
        assert_eq!(removed, 0);
        assert_eq!(maildir.scan().unwrap().len(), 2);
    }
}
