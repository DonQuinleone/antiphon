use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;

use antiphon_store::StoreLayout;

use crate::auth::Auth;
use crate::error::SyncError;
use crate::folders::folder_subdir;
use crate::maildir::MaildirFolder;
use crate::progress::{SyncProgress, write_progress};
use crate::report::{FolderReport, SyncReport};
use crate::session::ImapSession;
use crate::state::{AccountState, FolderState};

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
    let indexer = Indexer::start(layout);
    let outcome = (|| {
        for folder in folders {
            let folder_report = sync_folder(
                &mut session,
                account,
                layout,
                &folder,
                &mut state,
                &indexer,
            )?;
            state.save(&state_path)?;
            report.folders.push(folder_report);
        }
        Ok(())
    })();
    session.logout();
    indexer.finish()?;
    outcome?;
    run_notmuch_new(&layout.notmuch_config_path())?;
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
    fn start(layout: &StoreLayout) -> Indexer {
        let config = layout.notmuch_config_path();
        let (sender, receiver) = mpsc::channel::<()>();
        let handle = std::thread::spawn(move || {
            while receiver.recv().is_ok() {
                while receiver.try_recv().is_ok() {}
                run_notmuch_new(&config)?;
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
    let known_uid = known_uid(&maildir, state, folder, uid_validity)?;
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
    state.set_folder(
        &folder.name,
        FolderState {
            uid_validity,
            last_uid,
        },
    );
    Ok(FolderReport {
        folder: folder.name.clone(),
        new_messages: deliveries.len(),
        updated_messages,
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

/// Returns the highest UID already in the store for this
/// folder, wiping and restarting from zero when the server's
/// UIDVALIDITY no longer matches the recorded one.
fn known_uid(
    maildir: &MaildirFolder,
    state: &AccountState,
    folder: &RemoteFolder,
    uid_validity: u32,
) -> Result<u32, SyncError> {
    match state.folder(&folder.name) {
        Some(stored) if stored.uid_validity == uid_validity => {
            Ok(stored.last_uid)
        }
        Some(_) => {
            maildir
                .remove_delivered()
                .map_err(SyncError::io(maildir.root()))?;
            Ok(0)
        }
        None => Ok(0),
    }
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

fn state_path(layout: &StoreLayout, account: &SyncAccount) -> PathBuf {
    layout
        .sync_state_dir()
        .join(&account.name)
        .with_extension(STATE_FILE_EXTENSION)
}

fn ensure_dir(dir: &Path) -> Result<(), SyncError> {
    fs::create_dir_all(dir).map_err(SyncError::io(dir))
}

fn run_notmuch_new(config: &Path) -> Result<(), SyncError> {
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
