use std::collections::HashMap;
use std::fs;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use antiphon_store::StoreLayout;
use imap::Session;
use imap::types::{Fetch, Flag, NameAttribute};
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, StreamOwned};

use crate::error::SyncError;
use crate::folders::folder_subdir;
use crate::maildir::MaildirFolder;
use crate::report::{FolderReport, SyncReport};
use crate::state::{AccountState, FolderState};

const FIRST_UID: u32 = 1;
const NEW_MESSAGE_ITEMS: &str = "(UID FLAGS BODY.PEEK[])";
const FLAG_ITEMS: &str = "(UID FLAGS)";
const STATE_FILE_EXTENSION: &str = "state";

type TlsSession = Session<StreamOwned<ClientConnection, TcpStream>>;

#[derive(Clone, Debug)]
pub struct SyncAccount {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
}

struct RemoteFolder {
    name: String,
    delimiter: Option<String>,
}

pub fn sync(
    account: &SyncAccount,
    layout: &StoreLayout,
) -> Result<SyncReport, SyncError> {
    let mut session = connect(account)?;
    let folders = selectable_folders(&mut session)?;
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
    let _ = session.logout();
    run_notmuch_new(&layout.notmuch_config_path())?;
    Ok(report)
}

fn connect(account: &SyncAccount) -> Result<TlsSession, SyncError> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let server_name = ServerName::try_from(account.host.clone())
        .map_err(|_| SyncError::InvalidHost {
            host: account.host.clone(),
        })?;
    let connection =
        ClientConnection::new(Arc::new(config), server_name).map_err(
            |source| SyncError::Tls {
                host: account.host.clone(),
                source,
            },
        )?;
    let tcp = TcpStream::connect((account.host.as_str(), account.port))
        .map_err(|source| SyncError::Connect {
            host: account.host.clone(),
            port: account.port,
            source,
        })?;
    let mut client =
        imap::Client::new(StreamOwned::new(connection, tcp));
    client
        .read_greeting()
        .map_err(SyncError::imap("reading the server greeting"))?;
    client.login(&account.user, &account.password).map_err(
        |(source, _)| SyncError::Login {
            user: account.user.clone(),
            source,
        },
    )
}

fn selectable_folders(
    session: &mut TlsSession,
) -> Result<Vec<RemoteFolder>, SyncError> {
    let names = session
        .list(None, Some("*"))
        .map_err(SyncError::imap("listing folders"))?;
    let folders = names
        .iter()
        .filter(|name| {
            !name.attributes().contains(&NameAttribute::NoSelect)
        })
        .map(|name| RemoteFolder {
            name: name.name().to_owned(),
            delimiter: name.delimiter().map(str::to_owned),
        })
        .collect();
    Ok(folders)
}

fn sync_folder(
    session: &mut TlsSession,
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
    session: &mut TlsSession,
    folder: &RemoteFolder,
    maildir: &MaildirFolder,
    known_uid: u32,
) -> Result<Vec<u32>, SyncError> {
    let range = format!("{}:*", known_uid + 1);
    let fetches = session.uid_fetch(range, NEW_MESSAGE_ITEMS).map_err(
        SyncError::imap(format!(
            "fetching new mail in {}",
            folder.name
        )),
    )?;
    let mut delivered = Vec::new();
    for fetch in fetches.iter() {
        let uid = require_uid(fetch, folder)?;
        if uid <= known_uid {
            continue;
        }
        let body = fetch.body().ok_or_else(|| SyncError::Folder {
            folder: folder.name.clone(),
            detail: format!("uid {uid} came without a body"),
        })?;
        maildir
            .deliver(uid, is_seen(fetch), body)
            .map_err(SyncError::io(maildir.root()))?;
        delivered.push(uid);
    }
    Ok(delivered)
}

fn mirror_flags(
    session: &mut TlsSession,
    folder: &RemoteFolder,
    maildir: &MaildirFolder,
    known_uid: u32,
) -> Result<usize, SyncError> {
    let range = format!("{FIRST_UID}:{known_uid}");
    let fetches = session.uid_fetch(range, FLAG_ITEMS).map_err(
        SyncError::imap(format!("fetching flags in {}", folder.name)),
    )?;
    let mut server_seen = HashMap::new();
    for fetch in fetches.iter() {
        let uid = require_uid(fetch, folder)?;
        server_seen.insert(uid, is_seen(fetch));
    }
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

fn require_uid(
    fetch: &Fetch,
    folder: &RemoteFolder,
) -> Result<u32, SyncError> {
    fetch.uid.ok_or_else(|| SyncError::Folder {
        folder: folder.name.clone(),
        detail: String::from("server omitted the UID from a UID FETCH"),
    })
}

fn is_seen(fetch: &Fetch) -> bool {
    fetch.flags().iter().any(|flag| matches!(flag, Flag::Seen))
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
