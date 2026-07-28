use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;

use antiphon_store::StoreLayout;

use crate::auth::Auth;
use crate::error::SyncError;
use crate::folders::{excluded, folder_subdir};
use crate::maildir::MaildirFolder;
use crate::progress::{SyncProgress, write_progress};
use crate::reconcile::{now_unix, remove_vanished, sweep_due};
use crate::report::{FolderReport, SyncReport};
use crate::session::{ImapSession, SelectedFolder};
use crate::state::{AccountState, FolderState};
use crate::tagging::retag_folders;

const FIRST_UID: u32 = 1;
const STATE_FILE_EXTENSION: &str = "state";
/// Bounded batches keep memory flat on a huge first sync and
/// let the indexer make mail visible while later batches are
/// still downloading.
const FETCH_BATCH: usize = 200;

#[derive(Clone, Debug)]
pub struct SyncAccount {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: Auth,
    /// Maildir-relative folder names (`folders_unsynced` in
    /// the account file) the engine skips entirely; matched
    /// case-insensitively, and INBOX is never excludable.
    pub excluded_folders: Vec<String>,
}

pub(crate) struct RemoteFolder {
    pub(crate) name: String,
    pub(crate) delimiter: Option<String>,
}

struct Delivery {
    uid: u32,
    path: PathBuf,
}

pub fn sync(
    account: &SyncAccount,
    layout: &StoreLayout,
) -> Result<SyncReport, SyncError> {
    let mut session = ImapSession::connect(account)?;
    let folders = session
        .list_selectable()
        .map_err(SyncError::imap("listing folders"))?;
    let state_path = state_path(layout, account);
    ensure_dir(state_path.parent().unwrap_or(layout.root()))?;
    let mut state = AccountState::load(&state_path)?;
    let mut report = SyncReport::default();
    let indexer = Indexer::start(layout, &account.name);
    // One failing folder must not abort the rest: record it
    // and carry on, so a folder the server mishandles costs
    // only itself. Saving state still aborts, because a store
    // that cannot persist cursors corrupts every later pass.
    let outcome = (|| {
        for folder in folders {
            if excluded(&folder, &account.excluded_folders) {
                continue;
            }
            let folder_report = sync_folder(
                &mut session,
                account,
                layout,
                &folder,
                &mut state,
                &indexer,
            );
            let folder_report = match folder_report {
                Ok(folder_report) => folder_report,
                Err(error) => {
                    report
                        .errors
                        .push(format!("{}: {error}", folder.name));
                    continue;
                }
            };
            state.save(&state_path)?;
            report.folders.push(folder_report);
        }
        Ok(())
    })();
    session.logout();
    indexer.finish()?;
    outcome?;
    run_notmuch_new(&layout.notmuch_config_path())?;
    retag_folders(&layout.notmuch_config_path(), &account.name)?;
    Ok(report)
}

/// Indexing overlaps the network: delivered batches become
/// searchable while the next batch downloads. One thread, so
/// notmuch keeps its single writer.
struct Indexer {
    sender: Option<mpsc::Sender<()>>,
    handle: std::thread::JoinHandle<Result<(), SyncError>>,
}

impl Indexer {
    fn start(layout: &StoreLayout, account: &str) -> Indexer {
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

    fn nudge(&self) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(());
        }
    }

    fn finish(mut self) -> Result<(), SyncError> {
        self.sender.take();
        self.handle.join().unwrap_or_else(|_| {
            Err(SyncError::Notmuch {
                detail: "the indexer thread panicked".into(),
            })
        })
    }
}

fn sync_folder(
    session: &mut ImapSession,
    account: &SyncAccount,
    layout: &StoreLayout,
    folder: &RemoteFolder,
    state: &mut AccountState,
    indexer: &Indexer,
) -> Result<FolderReport, SyncError> {
    let maildir = open_maildir(account, layout, folder)?;
    let mailbox = session.examine(&folder.name).map_err(
        SyncError::imap(format!("examining {}", folder.name)),
    )?;
    let uid_validity =
        mailbox.uid_validity.ok_or_else(|| SyncError::Folder {
            folder: folder.name.clone(),
            detail: String::from("server reported no UIDVALIDITY"),
        })?;
    let now = now_unix();
    let baseline =
        folder_baseline(&maildir, state, folder, uid_validity, now)?;
    let sweeping = sweep_due(baseline.last_sweep_unix, now);
    let removed_messages = if sweeping {
        sweep_folder(session, folder, &maildir, &mailbox, indexer)?
    } else {
        0
    };
    let known_uid = baseline.last_uid;
    let has_mail = mailbox.exists > 0;
    // UIDNEXT lets an up-to-date folder skip the fetch, which
    // would otherwise return the newest message again because
    // an IMAP `n:*` range never selects the empty set.
    let expects_new =
        mailbox.uid_next.is_none_or(|next| next > known_uid + 1);
    let deliveries = if has_mail && expects_new {
        fetch_new(
            session, account, layout, folder, &maildir, known_uid,
            indexer,
        )?
    } else {
        Vec::new()
    };
    let updated_messages = if has_mail && known_uid >= FIRST_UID {
        mirror_flags(session, folder, &maildir, known_uid)?
    } else {
        0
    };
    let last_uid = deliveries
        .iter()
        .map(|delivery| delivery.uid)
        .max()
        .unwrap_or(known_uid);
    let last_sweep_unix = if sweeping {
        now
    } else {
        baseline.last_sweep_unix
    };
    state.set_folder(
        &folder.name,
        FolderState {
            uid_validity,
            last_uid,
            last_sweep_unix,
        },
    );
    Ok(FolderReport {
        folder: folder.name.clone(),
        new_messages: deliveries.len(),
        updated_messages,
        removed_messages,
        delivered: deliveries
            .into_iter()
            .map(|delivery| delivery.path)
            .collect(),
    })
}

fn open_maildir(
    account: &SyncAccount,
    layout: &StoreLayout,
    folder: &RemoteFolder,
) -> Result<MaildirFolder, SyncError> {
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
    Ok(maildir)
}

/// Returns the stored cursor for this folder, wiping local
/// mail and restarting from zero when the server's UIDVALIDITY
/// no longer matches the recorded one. Wiped and brand-new
/// folders stamp the sweep clock to `now`: everything local is
/// about to be refetched, so nothing can have vanished.
fn folder_baseline(
    maildir: &MaildirFolder,
    state: &AccountState,
    folder: &RemoteFolder,
    uid_validity: u32,
    now: u64,
) -> Result<FolderState, SyncError> {
    let fresh = FolderState {
        uid_validity,
        last_uid: 0,
        last_sweep_unix: now,
    };
    match state.folder(&folder.name) {
        Some(stored) if stored.uid_validity == uid_validity => {
            Ok(stored)
        }
        Some(_) => {
            maildir
                .remove_delivered()
                .map_err(SyncError::io(maildir.root()))?;
            Ok(fresh)
        }
        None => Ok(fresh),
    }
}

/// Removes messages deleted or moved server-side, by listing
/// the folder's full UID set and dropping local files the
/// listing no longer covers. A folder the server reports empty
/// skips the listing: `UID FETCH 1:*` has nothing to select.
fn sweep_folder(
    session: &mut ImapSession,
    folder: &RemoteFolder,
    maildir: &MaildirFolder,
    mailbox: &SelectedFolder,
    indexer: &Indexer,
) -> Result<usize, SyncError> {
    let server: HashSet<u32> = if mailbox.exists == 0 {
        HashSet::new()
    } else {
        session
            .list_new_uids(FIRST_UID)
            .map_err(SyncError::imap(format!(
                "sweeping {}",
                folder.name
            )))?
            .into_iter()
            .collect()
    };
    let removed = remove_vanished(maildir, &server)?;
    if removed > 0 {
        indexer.nudge();
    }
    Ok(removed)
}

fn fetch_new(
    session: &mut ImapSession,
    account: &SyncAccount,
    layout: &StoreLayout,
    folder: &RemoteFolder,
    maildir: &MaildirFolder,
    known_uid: u32,
    indexer: &Indexer,
) -> Result<Vec<Delivery>, SyncError> {
    let uids: Vec<u32> = session
        .list_new_uids(known_uid + 1)
        .map_err(SyncError::imap(format!(
            "listing new mail in {}",
            folder.name
        )))?
        .into_iter()
        .filter(|uid| *uid > known_uid)
        .collect();
    let total = uids.len();
    let mut delivered = Vec::new();
    for batch in uids.chunks(FETCH_BATCH) {
        write_progress(
            layout,
            &SyncProgress::syncing(
                &account.name,
                &folder.name,
                delivered.len(),
                total,
            ),
        );
        deliver_batch(
            session,
            folder,
            maildir,
            known_uid,
            batch,
            &mut delivered,
        )?;
        indexer.nudge();
    }
    write_progress(
        layout,
        &SyncProgress::syncing(
            &account.name,
            &folder.name,
            delivered.len(),
            total,
        ),
    );
    Ok(delivered)
}

fn deliver_batch(
    session: &mut ImapSession,
    folder: &RemoteFolder,
    maildir: &MaildirFolder,
    known_uid: u32,
    batch: &[u32],
    delivered: &mut Vec<Delivery>,
) -> Result<(), SyncError> {
    let fetched =
        session.fetch_uids(batch).map_err(SyncError::imap(format!(
            "fetching new mail in {}",
            folder.name
        )))?;
    for message in fetched {
        if message.uid <= known_uid {
            continue;
        }
        let Some(body) = message.body else {
            return Err(SyncError::Folder {
                folder: folder.name.clone(),
                detail: format!(
                    "uid {} came without a body",
                    message.uid
                ),
            });
        };
        let path = maildir
            .deliver(message.uid, message.seen, &body)
            .map_err(SyncError::io(maildir.root()))?;
        delivered.push(Delivery {
            uid: message.uid,
            path,
        });
    }
    Ok(())
}

fn mirror_flags(
    session: &mut ImapSession,
    folder: &RemoteFolder,
    maildir: &MaildirFolder,
    known_uid: u32,
) -> Result<usize, SyncError> {
    let server_seen = session
        .fetch_seen_flags(FIRST_UID, known_uid)
        .map_err(SyncError::imap(format!(
            "fetching flags in {}",
            folder.name
        )))?;
    let local =
        maildir.scan().map_err(SyncError::io(maildir.root()))?;
    let mut updated = 0;
    for message in local {
        let Some(&seen) = server_seen.get(&message.uid) else {
            continue;
        };
        if message.seen == seen {
            continue;
        }
        maildir
            .mirror_seen(&message, seen)
            .map_err(SyncError::io(maildir.root()))?;
        updated += 1;
    }
    Ok(updated)
}

/// The extension is appended, never substituted: an account
/// named `work.gmail` must not share `work.state` with an
/// account named `work`.
pub(crate) fn state_path(
    layout: &StoreLayout,
    account: &SyncAccount,
) -> PathBuf {
    layout
        .sync_state_dir()
        .join(format!("{}.{STATE_FILE_EXTENSION}", account.name))
}

fn ensure_dir(dir: &Path) -> Result<(), SyncError> {
    fs::create_dir_all(dir).map_err(SyncError::io(dir))
}

pub(crate) fn run_notmuch_new(config: &Path) -> Result<(), SyncError> {
    let output = Command::new("notmuch")
        .arg("new")
        .env("NOTMUCH_CONFIG", config)
        .output()
        .map_err(|source| SyncError::NotmuchSpawn { source })?;
    if output.status.success() {
        return Ok(());
    }
    Err(SyncError::Notmuch {
        detail: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

#[cfg(test)]
mod state_path_tests {
    use super::*;

    fn account(name: &str) -> SyncAccount {
        SyncAccount {
            name: name.to_owned(),
            host: "imap.example.com".to_owned(),
            port: 993,
            user: "quin@example.com".to_owned(),
            auth: Auth::Password("never-used".to_owned()),
            excluded_folders: Vec::new(),
        }
    }

    #[test]
    fn dotted_account_names_keep_distinct_state_files() {
        let layout = StoreLayout::new(PathBuf::from("/store"));
        let plain = state_path(&layout, &account("work"));
        let dotted = state_path(&layout, &account("work.gmail"));
        assert!(plain.ends_with("work.state"));
        assert!(dotted.ends_with("work.gmail.state"));
        assert_ne!(plain, dotted);
    }
}
