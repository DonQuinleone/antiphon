use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use secrecy::SecretString;

/// Sealed at rest, open in session, ciphertext-only off the
/// machine. Backends orchestrate platform tools; no crypto of
/// our own lives here.
pub trait Vault {
    fn status(&self) -> VaultStatus;
    fn create(&self, opts: &CreateOptions) -> Result<(), VaultError>;
    fn unlock(&self, auth: &Auth) -> Result<Mounted, VaultError>;
    fn lock(&self) -> Result<(), VaultError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultStatus {
    Sealed,
    Open,
    Absent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mounted {
    mount_point: PathBuf,
}

impl Mounted {
    pub fn new(mount_point: impl Into<PathBuf>) -> Mounted {
        Mounted {
            mount_point: mount_point.into(),
        }
    }

    pub fn mount_point(&self) -> &Path {
        &self.mount_point
    }
}

#[derive(Debug, Clone)]
pub struct CreateOptions {
    pub auth: Auth,
}

#[derive(Debug, Clone)]
pub enum Auth {
    Passphrase(SecretString),
    Touchid,
    Yubikey,
}

impl Auth {
    pub fn method(&self) -> &'static str {
        match self {
            Auth::Passphrase(_) => "passphrase",
            Auth::Touchid => "touchid",
            Auth::Yubikey => "yubikey",
        }
    }
}

pub(crate) fn passphrase(
    auth: &Auth,
) -> Result<&SecretString, VaultError> {
    let Auth::Passphrase(secret) = auth else {
        return Err(VaultError::AuthUnsupported(auth.method()));
    };
    Ok(secret)
}

#[derive(Debug)]
pub enum VaultError {
    AlreadyExists(PathBuf),
    Absent(PathBuf),
    AuthUnsupported(&'static str),
    Tool {
        tool: &'static str,
        status: Option<i32>,
        stderr_tail: String,
    },
    Io(io::Error),
    UnsupportedOnThisBuild(&'static str),
}

impl fmt::Display for VaultError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VaultError::AlreadyExists(path) => {
                write!(
                    out,
                    "vault already exists at {}",
                    path.display()
                )
            }
            VaultError::Absent(path) => {
                write!(
                    out,
                    "no vault at {}; create one first",
                    path.display()
                )
            }
            VaultError::AuthUnsupported(method) => {
                write!(
                    out,
                    "unlock method `{method}` is not implemented \
                     on this backend; use a passphrase"
                )
            }
            VaultError::Tool {
                tool,
                status,
                stderr_tail,
            } => {
                write!(out, "`{tool}` ")?;
                match status {
                    Some(code) => write!(out, "exited {code}")?,
                    None => write!(out, "died on a signal")?,
                }
                if stderr_tail.is_empty() {
                    return Ok(());
                }
                write!(out, ": {stderr_tail}")
            }
            VaultError::Io(err) => write!(out, "{err}"),
            VaultError::UnsupportedOnThisBuild(backend) => {
                write!(
                    out,
                    "vault backend `{backend}` is not supported \
                     on this build"
                )
            }
        }
    }
}

impl std::error::Error for VaultError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            VaultError::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for VaultError {
    fn from(err: io::Error) -> VaultError {
        VaultError::Io(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_debug_never_prints_the_passphrase() {
        let auth =
            Auth::Passphrase(SecretString::from("hunter2".to_owned()));
        let printed = format!("{auth:?}");
        assert!(!printed.contains("hunter2"), "{printed}");
    }

    #[test]
    fn tool_error_names_status_and_stderr() {
        let err = VaultError::Tool {
            tool: "hdiutil",
            status: Some(1),
            stderr_tail: "attach failed".to_owned(),
        };
        assert_eq!(
            err.to_string(),
            "`hdiutil` exited 1: attach failed"
        );
    }
}
