use std::path::Path;
use std::process::Command;

use crate::error::SyncError;

/// notmuch's new.tags stamps every indexed message with
/// `inbox`, whichever folder it came from; this strips it from
/// everything that is not actually in the account's inbox, and
/// each run also heals any store polluted by earlier passes.
pub(crate) fn retag_folders(
    config: &Path,
    account: &str,
) -> Result<(), SyncError> {
    let query = format!(
        "tag:inbox and path:\"{account}/**\" \
         and not path:\"{account}/cur\" \
         and not path:\"{account}/new\" \
         and not path:\"{account}/inbox/**\""
    );
    let output = Command::new("notmuch")
        .args(["tag", "-inbox", "--", &query])
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
