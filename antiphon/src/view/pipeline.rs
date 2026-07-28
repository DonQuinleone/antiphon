use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::{Component, Path, PathBuf};

use flate2::read::GzDecoder;

use super::{ViewError, ViewKey};

/// Unpack the encrypted archive under `maildir_root/account`
/// and return the number of files written. The data streams
/// file -> age -> gunzip -> untar, mirroring the export
/// pipeline in reverse, so memory use stays flat however large
/// the mailbox is.
pub(super) fn unpack(
    archive: &Path,
    maildir_root: &Path,
    account: &str,
    key: &ViewKey,
) -> Result<u64, ViewError> {
    let archive_err = |err: io::Error| ViewError::Archive {
        path: archive.to_path_buf(),
        message: err.to_string(),
    };
    let file = File::open(archive).map_err(archive_err)?;
    let decryptor = age::Decryptor::new(BufReader::new(file))
        .map_err(|err| ViewError::Decrypt(err.to_string()))?;
    let reader = decrypt(decryptor, key)?;
    let mut entries = tar::Archive::new(GzDecoder::new(reader));
    let account_root = maildir_root.join(account);
    let mut files = 0;
    for entry in entries.entries().map_err(archive_err)? {
        let mut entry = entry.map_err(archive_err)?;
        files += extract(&mut entry, &account_root)?;
    }
    Ok(files)
}

fn decrypt<R: Read + 'static>(
    decryptor: age::Decryptor<R>,
    key: &ViewKey,
) -> Result<Box<dyn Read>, ViewError> {
    let failed =
        |err: age::DecryptError| ViewError::Decrypt(err.to_string());
    let reader: Box<dyn Read> = match key {
        ViewKey::Identities(keys) => Box::new(
            decryptor
                .decrypt(
                    keys.iter().map(|key| key as &dyn age::Identity),
                )
                .map_err(failed)?,
        ),
        ViewKey::Passphrase(passphrase) => {
            let identity =
                age::scrypt::Identity::new(passphrase.clone());
            Box::new(
                decryptor
                    .decrypt(std::iter::once(
                        &identity as &dyn age::Identity,
                    ))
                    .map_err(failed)?,
            )
        }
    };
    Ok(reader)
}

fn extract(
    entry: &mut tar::Entry<impl Read>,
    account_root: &Path,
) -> Result<u64, ViewError> {
    let name = entry
        .path()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let refuse = |reason: &str| ViewError::BadEntry {
        entry: name.clone(),
        reason: reason.to_string(),
    };
    let path = entry.path().map_err(|_| refuse("not valid utf-8"))?;
    let destination =
        account_root.join(safe_remainder(&path, &refuse)?);
    let unpack_err = |err: io::Error| ViewError::Unpack {
        path: destination.clone(),
        message: err.to_string(),
    };
    match entry.header().entry_type() {
        tar::EntryType::Directory => {
            std::fs::create_dir_all(&destination)
                .map_err(unpack_err)?;
            Ok(0)
        }
        tar::EntryType::Regular => {
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent).map_err(unpack_err)?;
            }
            entry.unpack(&destination).map_err(unpack_err)?;
            Ok(1)
        }
        _ => Err(refuse("only files and directories are unpacked")),
    }
}

/// The entry path with its leading component (the archived
/// account directory) dropped, refusing anything that could
/// step outside the store: absolute paths, `..`, prefixes.
fn safe_remainder(
    path: &Path,
    refuse: &impl Fn(&str) -> ViewError,
) -> Result<PathBuf, ViewError> {
    let mut components = path.components();
    let mut remainder = PathBuf::new();
    let leading = components
        .next()
        .ok_or_else(|| refuse("empty entry path"))?;
    let all = std::iter::once(leading).chain(components.clone());
    for component in all {
        let Component::Normal(_) = component else {
            return Err(refuse("the path escapes the store"));
        };
    }
    for component in components {
        remainder.push(component);
    }
    Ok(remainder)
}
