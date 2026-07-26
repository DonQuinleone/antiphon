use crate::engine::{RemoteFolder, SyncAccount};
use crate::error::SyncError;
use crate::folders::folder_subdir;
use crate::session::ImapSession;
use imap_client::imap_types::flag::Flag;

const SENT_SUBDIR: &str = "sent";

/// Files a sent copy in the server's own sent folder, so the
/// record survives this machine. Best-effort by design: the
/// message has already gone and a local twin already exists,
/// so a failure here is a log line, never a lost send.
pub fn append_sent(
    account: &SyncAccount,
    raw: &[u8],
) -> Result<String, SyncError> {
    let mut session = ImapSession::connect(account)?;
    let outcome = append_in(&mut session, raw);
    session.logout();
    outcome
}

fn append_in(
    session: &mut ImapSession,
    raw: &[u8],
) -> Result<String, SyncError> {
    let folders = session
        .list_selectable()
        .map_err(SyncError::imap("listing folders"))?;
    let Some(folder) = sent_folder(folders) else {
        return Err(SyncError::Folder {
            folder: SENT_SUBDIR.to_string(),
            detail: "no sent folder on the server".to_string(),
        });
    };
    session
        .append(&folder.name, vec![Flag::Seen], raw)
        .map_err(SyncError::imap(format!(
            "appending to {}",
            folder.name
        )))?;
    Ok(folder.name)
}

/// The server sent folder is whichever selectable folder maps
/// to the local `sent` subdirectory, the same rule the drafts
/// push uses for its folder.
fn sent_folder(folders: Vec<RemoteFolder>) -> Option<RemoteFolder> {
    folders.into_iter().find(|folder| {
        folder_subdir(&folder.name, folder.delimiter.as_deref())
            .map(|subdir| subdir == std::path::Path::new(SENT_SUBDIR))
            .unwrap_or(false)
    })
}
