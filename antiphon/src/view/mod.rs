mod pipeline;
#[cfg(test)]
mod tests;

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use age::secrecy::SecretString;
use antiphon_store::StoreLayout;

const COMPLETE_MARKER: &str = ".complete";
const ARCHIVE_EXTENSIONS: [&str; 3] = ["age", "gz", "tar"];

/// How an archive is decrypted: with age identities read from
/// files, or with a passphrase collected by the caller.
pub enum ViewKey {
    Identities(Vec<age::x25519::Identity>),
    Passphrase(SecretString),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Opened {
    Unpacked { files: u64 },
    Reused,
}

#[derive(Debug)]
pub enum ViewError {
    Archive { path: PathBuf, message: String },
    Decrypt(String),
    BadEntry { entry: String, reason: String },
    Unpack { path: PathBuf, message: String },
    Store { path: PathBuf, message: String },
    Index(String),
}

impl fmt::Display for ViewError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ViewError::Archive { path, message } => {
                write!(out, "cannot read {}: {message}", path.display())
            }
            ViewError::Decrypt(message) => {
                write!(out, "decryption failed: {message}")
            }
            ViewError::BadEntry { entry, reason } => {
                write!(
                    out,
                    "refusing archive entry \"{entry}\": {reason}"
                )
            }
            ViewError::Unpack { path, message } => {
                write!(
                    out,
                    "cannot unpack to {}: {message}",
                    path.display()
                )
            }
            ViewError::Store { path, message } => {
                write!(
                    out,
                    "cannot prepare the view store {}: {message}",
                    path.display()
                )
            }
            ViewError::Index(message) => {
                write!(out, "notmuch new failed: {message}")
            }
        }
    }
}

impl std::error::Error for ViewError {}

/// The account name an archive file implies: its file name
/// with the archive extensions stripped, so
/// "work-2026-07-28.tar.gz.age" views as "work-2026-07-28".
pub fn archive_stem(archive: &Path) -> String {
    let mut name = Path::new(archive.file_name().unwrap_or_default())
        .to_path_buf();
    while name
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ARCHIVE_EXTENSIONS.contains(&ext))
    {
        name.set_extension("");
    }
    name.to_string_lossy().into_owned()
}

/// Decrypt and unpack `archive` into a throwaway store at
/// `store_root`, indexed and ready for the client. A previous
/// complete unpack (its marker present) is reused untouched; a
/// partial one is discarded and redone.
pub fn open_archive(
    archive: &Path,
    store_root: &Path,
    account: &str,
    key: &ViewKey,
) -> Result<Opened, ViewError> {
    let store = |err: std::io::Error| ViewError::Store {
        path: store_root.to_path_buf(),
        message: err.to_string(),
    };
    let marker = store_root.join(COMPLETE_MARKER);
    if marker.is_file() {
        return Ok(Opened::Reused);
    }
    if store_root.exists() {
        std::fs::remove_dir_all(store_root).map_err(store)?;
    }
    let layout = StoreLayout::new(store_root);
    layout.init().map_err(store)?;
    let files = pipeline::unpack(
        archive,
        &layout.maildir_root(),
        account,
        key,
    )?;
    index(&layout)?;
    std::fs::write(&marker, format!("{}\n", archive.display()))
        .map_err(store)?;
    Ok(Opened::Unpacked { files })
}

fn index(layout: &StoreLayout) -> Result<(), ViewError> {
    let output = Command::new("notmuch")
        .arg("new")
        .env("NOTMUCH_CONFIG", layout.notmuch_config_path())
        .output()
        .map_err(|err| ViewError::Index(err.to_string()))?;
    if output.status.success() {
        return Ok(());
    }
    Err(ViewError::Index(
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}
