use std::fs;
use std::path::Path;

use antiphon_store::{
    DraftSpool, QueuedDraft, SpoolError, StoreLayout,
};
use imap_client::imap_types::flag::Flag;

use crate::engine::{
    RemoteFolder, SyncAccount, run_notmuch_new, state_path,
};
use crate::error::SyncError;
use crate::folders::folder_subdir;
use crate::maildir::MaildirFolder;
use crate::session::ImapSession;
use crate::state::{AccountState, FolderState};
use crate::tagging::retag_folders;

const DRAFTS_SUBDIR: &str = "drafts";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DraftPush {
    pub filed: usize,
    pub left: usize,
    pub folder: Option<String>,
}

/// Files this account's spooled drafts on its server drafts
/// folder: APPEND with \Draft and \Seen, drop the spool entry,
/// then deliver a local twin under the server's UID so the
/// next sync treats it as an ordinary message. With no server
/// drafts folder everything stays spooled for a later pass; a
/// draft is never lost.
pub fn push_drafts(
    account: &SyncAccount,
    layout: &StoreLayout,
) -> Result<DraftPush, SyncError> {
    let spool = DraftSpool::open(layout);
    let pending =
        spool.pending_for(&account.name).map_err(spool_error)?;
    if pending.is_empty() {
        return Ok(DraftPush::default());
    }
    let mut session = ImapSession::connect(account)?;
    let outcome =
        file_pending(&mut session, account, layout, &spool, pending);
    session.logout();
    outcome
}

fn file_pending(
    session: &mut ImapSession,
    account: &SyncAccount,
    layout: &StoreLayout,
    spool: &DraftSpool,
    pending: Vec<QueuedDraft>,
) -> Result<DraftPush, SyncError> {
    let total = pending.len();
    let folders = session
        .list_selectable()
        .map_err(SyncError::imap("listing folders"))?;
    let Some(folder) = drafts_folder(folders) else {
        return Ok(DraftPush {
            filed: 0,
            left: total,
            folder: None,
        });
    };
    let mut filed = 0;
    let mut twins = 0;
    let mut failure = None;
    for draft in pending {
        match file_one(session, account, layout, spool, &folder, &draft)
        {
            Ok(twin) => {
                filed += 1;
                twins += usize::from(twin);
            }
            Err(error) => {
                failure = Some(error);
                break;
            }
        }
    }
    if twins > 0 {
        refresh_index(layout, &account.name)?;
    }
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(DraftPush {
        filed,
        left: total - filed,
        folder: Some(folder.name),
    })
}

/// The server drafts folder is whichever selectable folder
/// maps onto the local drafts subdirectory.
fn drafts_folder(folders: Vec<RemoteFolder>) -> Option<RemoteFolder> {
    folders.into_iter().find(|folder| {
        folder_subdir(&folder.name, folder.delimiter.as_deref())
            .is_ok_and(|subdir| subdir == Path::new(DRAFTS_SUBDIR))
    })
}

/// The spool entry is dropped as soon as the APPEND succeeds:
/// from then on the server holds the draft, and a failed twin
/// delivery only means the next sync fetches it instead.
fn file_one(
    session: &mut ImapSession,
    account: &SyncAccount,
    layout: &StoreLayout,
    spool: &DraftSpool,
    folder: &RemoteFolder,
    draft: &QueuedDraft,
) -> Result<bool, SyncError> {
    let raw = fs::read(&draft.message_path)
        .map_err(SyncError::io(&draft.message_path))?;
    let appended = session
        .append(&folder.name, vec![Flag::Draft, Flag::Seen], &raw)
        .map_err(SyncError::imap(format!(
            "appending to {}",
            folder.name
        )))?;
    spool.remove(draft.id).map_err(spool_error)?;
    let Some((uid, uid_validity)) = appended else {
        return Ok(false);
    };
    deliver_twin(account, layout, folder, uid, uid_validity, &raw)?;
    Ok(true)
}

fn deliver_twin(
    account: &SyncAccount,
    layout: &StoreLayout,
    folder: &RemoteFolder,
    uid: u32,
    uid_validity: u32,
    raw: &[u8],
) -> Result<(), SyncError> {
    let subdir =
        folder_subdir(&folder.name, folder.delimiter.as_deref())
            .map_err(|detail| SyncError::Folder {
                folder: folder.name.clone(),
                detail,
            })?;
    let maildir = MaildirFolder::new(
        layout.account_maildir(&account.name).join(subdir),
    );
    maildir.ensure().map_err(SyncError::io(maildir.root()))?;
    maildir
        .deliver(uid, true, raw)
        .map_err(SyncError::io(maildir.root()))?;
    advance_cursor(account, layout, &folder.name, uid, uid_validity)
}

/// The twin already sits in the maildir under its server UID;
/// advancing the folder cursor stops the next sync fetching it
/// again. A gap means another client appended in between:
/// leave the cursor so the sync collects theirs (ours arrives
/// twice, which notmuch collapses by Message-ID).
fn advance_cursor(
    account: &SyncAccount,
    layout: &StoreLayout,
    folder_name: &str,
    uid: u32,
    uid_validity: u32,
) -> Result<(), SyncError> {
    let path = state_path(layout, account);
    let mut state = AccountState::load(&path)?;
    let Some(stored) = state.folder(folder_name) else {
        return Ok(());
    };
    let contiguous = stored.uid_validity == uid_validity
        && uid == stored.last_uid + 1;
    if !contiguous {
        return Ok(());
    }
    state.set_folder(
        folder_name,
        FolderState {
            uid_validity,
            last_uid: uid,
            last_sweep_unix: stored.last_sweep_unix,
        },
    );
    state.save(&path)
}

fn refresh_index(
    layout: &StoreLayout,
    account: &str,
) -> Result<(), SyncError> {
    run_notmuch_new(&layout.notmuch_config_path())?;
    retag_folders(&layout.notmuch_config_path(), account)
}

fn spool_error(source: SpoolError) -> SyncError {
    SyncError::Spool { source }
}

#[cfg(test)]
mod tests {
    use antiphon_store::DraftEnvelope;

    use crate::auth::Auth;

    use super::*;

    fn folder(name: &str, delimiter: &str) -> RemoteFolder {
        RemoteFolder {
            name: name.to_owned(),
            delimiter: Some(delimiter.to_owned()),
        }
    }

    fn account(host: &str, port: u16) -> SyncAccount {
        SyncAccount {
            name: "personal".to_owned(),
            host: host.to_owned(),
            port,
            user: "quin@example.com".to_owned(),
            auth: Auth::Password("secret".to_owned()),
        }
    }

    fn layout_in(dir: &tempfile::TempDir) -> StoreLayout {
        StoreLayout::new(dir.path().join("store"))
    }

    #[test]
    fn the_drafts_folder_maps_onto_the_drafts_subdir() {
        let found = drafts_folder(vec![
            folder("INBOX", "/"),
            folder("Sent", "/"),
            folder("Drafts", "/"),
        ]);
        assert_eq!(found.unwrap().name, "Drafts");
        let upper = drafts_folder(vec![folder("DRAFTS", ".")]);
        assert_eq!(upper.unwrap().name, "DRAFTS");
        assert!(drafts_folder(vec![folder("INBOX", "/")]).is_none());
        assert!(
            drafts_folder(vec![folder("INBOX.Drafts", ".")]).is_none()
        );
    }

    #[test]
    fn an_empty_spool_returns_before_any_connection() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout_in(&dir);
        let push =
            push_drafts(&account("unreachable.invalid", 993), &layout)
                .unwrap();
        assert_eq!(push, DraftPush::default());
    }

    #[test]
    fn a_failed_connection_leaves_the_draft_spooled() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout_in(&dir);
        let spool = DraftSpool::open(&layout);
        spool
            .enqueue(
                &DraftEnvelope {
                    account: "personal".to_owned(),
                },
                b"Subject: kept",
            )
            .unwrap();
        let error =
            push_drafts(&account("127.0.0.1", 1), &layout).unwrap_err();
        assert!(matches!(error, SyncError::Connect { .. }));
        assert_eq!(spool.pending().unwrap().len(), 1);
    }

    #[test]
    fn the_cursor_advances_only_over_a_contiguous_append() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout_in(&dir);
        let account = account("imap.example.com", 993);
        std::fs::create_dir_all(layout.sync_state_dir()).unwrap();
        let path = state_path(&layout, &account);
        let mut state = AccountState::default();
        state.set_folder(
            "Drafts",
            FolderState {
                uid_validity: 3,
                last_uid: 6,
                last_sweep_unix: 0,
            },
        );
        state.save(&path).unwrap();

        advance_cursor(&account, &layout, "Drafts", 7, 3).unwrap();
        let advanced = AccountState::load(&path).unwrap();
        assert_eq!(advanced.folder("Drafts").unwrap().last_uid, 7);

        advance_cursor(&account, &layout, "Drafts", 9, 3).unwrap();
        advance_cursor(&account, &layout, "Drafts", 8, 4).unwrap();
        advance_cursor(&account, &layout, "Unknown", 1, 1).unwrap();
        let held = AccountState::load(&path).unwrap();
        assert_eq!(held.folder("Drafts").unwrap().last_uid, 7);
        assert!(held.folder("Unknown").is_none());
    }
}
