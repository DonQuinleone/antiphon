use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use antiphon_store::StoreLayout;

use crate::error::SyncError;
use crate::folders::folder_subdir;
use crate::maildir::MaildirFolder;
use crate::report::{FolderReport, SyncReport};
use crate::session::ImapSession;
use crate::state::{AccountState, FolderState};

const FIRST_UID: u32 = 1;
const STATE_FILE_EXTENSION: &str = "state";

#[derive(Clone, Debug)]
pub struct SyncAccount {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
}

pub(crate) struct RemoteFolder {
    pub(crate) name: String,
    pub(crate) delimiter: Option<String>,
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
    for folder in folders {
        let folder_report = sync_folder(
            &mut session,
            account,
            layout,
            &folder,
            &mut state,
        )?;
        state.save(&state_path)?;
        report.folders.push(folder_report);
    }
    session.logout();
    run_notmuch_new(&layout.notmuch_config_path())?;
    Ok(report)
}

fn sync_folder(
    session: &mut ImapSession,
    account: &SyncAccount,
    layout: &StoreLayout,
    folder: &RemoteFolder,
    state: &mut AccountState,
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
    let new_messages = if has_mail && expects_new {
        fetch_new(session, folder, &maildir, known_uid)?
    } else {
        Vec::new()
    };
    let updated_messages = if has_mail && known_uid >= FIRST_UID {
        mirror_flags(session, folder, &maildir, known_uid)?
    } else {
        0
    };
    let last_uid =
        new_messages.iter().copied().max().unwrap_or(known_uid);
    state.set_folder(
        &folder.name,
        FolderState {
            uid_validity,
            last_uid,
        },
    );
    Ok(FolderReport {
        folder: folder.name.clone(),
        new_messages: new_messages.len(),
        updated_messages,
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
    folder: &RemoteFolder,
    maildir: &MaildirFolder,
    known_uid: u32,
) -> Result<Vec<u32>, SyncError> {
    let fetched =
        session.fetch_new(known_uid + 1).map_err(SyncError::imap(
            format!("fetching new mail in {}", folder.name),
        ))?;
    let mut delivered = Vec::new();
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
        maildir
            .deliver(message.uid, message.seen, &body)
            .map_err(SyncError::io(maildir.root()))?;
        delivered.push(message.uid);
    }
    Ok(delivered)
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
