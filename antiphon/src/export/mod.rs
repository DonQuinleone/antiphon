mod pipeline;
#[cfg(test)]
mod tests;

use std::fmt;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use age::secrecy::SecretString;

/// How an export is encrypted: to age public keys, or to a
/// passphrase collected by the caller.
pub enum ExportKey {
    Recipients(Vec<age::x25519::Recipient>),
    Passphrase(SecretString),
}

impl ExportKey {
    fn recipients(&self) -> Vec<Box<dyn age::Recipient + Send>> {
        match self {
            ExportKey::Recipients(keys) => keys
                .iter()
                .map(|key| {
                    Box::new(key.clone())
                        as Box<dyn age::Recipient + Send>
                })
                .collect(),
            ExportKey::Passphrase(passphrase) => {
                vec![Box::new(age::scrypt::Recipient::new(
                    passphrase.clone(),
                ))]
            }
        }
    }
}

#[derive(Debug)]
pub struct ExportSummary {
    pub account: String,
    pub files: u64,
    pub bytes: u64,
    pub destination: PathBuf,
}

impl ExportSummary {
    pub fn line(&self) -> String {
        format!(
            "exported {}: {} files, {} bytes to {}",
            self.account,
            self.files,
            self.bytes,
            self.destination.display(),
        )
    }
}

#[derive(Debug)]
pub enum ExportError {
    MissingMaildir { account: String, path: PathBuf },
    BadRecipient { key: String, reason: String },
    Output { path: PathBuf, message: String },
    Archive { path: PathBuf, message: String },
    Encrypt(String),
    Verify { path: PathBuf, message: String },
}

impl fmt::Display for ExportError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::MissingMaildir { account, path } => {
                write!(
                    out,
                    "account {account} has no maildir at {}",
                    path.display()
                )
            }
            ExportError::BadRecipient { key, reason } => {
                write!(out, "bad age recipient \"{key}\": {reason}")
            }
            ExportError::Output { path, message } => {
                write!(
                    out,
                    "cannot write {}: {message}",
                    path.display()
                )
            }
            ExportError::Archive { path, message } => {
                write!(
                    out,
                    "cannot archive {}: {message}",
                    path.display()
                )
            }
            ExportError::Encrypt(message) => {
                write!(out, "encryption failed: {message}")
            }
            ExportError::Verify { path, message } => {
                write!(
                    out,
                    "wrote {} but its age header does not \
                     parse: {message}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for ExportError {}

/// Parse `age1...` public keys, keeping the offending input in
/// the error so the user can see which key is broken.
pub fn parse_recipients(
    keys: &[String],
) -> Result<Vec<age::x25519::Recipient>, ExportError> {
    keys.iter()
        .map(|key| {
            key.trim().parse().map_err(|reason: &str| {
                ExportError::BadRecipient {
                    key: key.clone(),
                    reason: reason.to_string(),
                }
            })
        })
        .collect()
}

/// The default archive name for an account exported today.
pub fn archive_file_name(account: &str) -> String {
    let date = chrono::Utc::now().format("%Y-%m-%d");
    format!("{account}-{date}.tar.gz.age")
}

/// Stream the account's maildir through tar, gzip and age into
/// `destination`, then re-open the result to confirm the age
/// header parses before reporting success.
pub fn export_account(
    maildir: &Path,
    account: &str,
    destination: &Path,
    key: &ExportKey,
) -> Result<ExportSummary, ExportError> {
    if !maildir.is_dir() {
        return Err(ExportError::MissingMaildir {
            account: account.to_string(),
            path: maildir.to_path_buf(),
        });
    }
    let (files, bytes) = pipeline::write_archive(
        maildir,
        account,
        destination,
        &key.recipients(),
    )?;
    verify_header(destination)?;
    Ok(ExportSummary {
        account: account.to_string(),
        files,
        bytes,
        destination: destination.to_path_buf(),
    })
}

fn verify_header(path: &Path) -> Result<(), ExportError> {
    let verify = |message: String| ExportError::Verify {
        path: path.to_path_buf(),
        message,
    };
    let file =
        File::open(path).map_err(|err| verify(err.to_string()))?;
    age::Decryptor::new(BufReader::new(file))
        .map(|_| ())
        .map_err(|err| verify(err.to_string()))
}
