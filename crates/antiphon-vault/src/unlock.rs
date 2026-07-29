//! Ordered resolution of the vault passphrase across the
//! configured unlock methods. Each method is a [`SecretSource`];
//! the resolver tries them in order and returns the first secret,
//! so a cancelled Touch ID prompt falls through to the passphrase
//! command rather than failing the unlock. Every source failing
//! is itself a failure: the vault stays sealed, never bypassed.

use std::path::{Path, PathBuf};

use secrecy::SecretString;

use crate::fido2;
use crate::passphrase::passphrase_command;
use crate::touchid;
use crate::vault::VaultError;

/// One configured way to obtain the vault passphrase.
pub trait SecretSource {
    fn method(&self) -> &'static str;
    fn passphrase(&self) -> Result<SecretString, VaultError>;
}

/// Try each source in order; the first to yield a secret wins,
/// and its method name comes back so the caller can report which
/// one opened the vault. With no sources, or all failing, the
/// vault stays sealed.
pub fn resolve_passphrase(
    sources: &[Box<dyn SecretSource>],
) -> Result<(SecretString, &'static str), VaultError> {
    let mut last: Option<VaultError> = None;
    for source in sources {
        match source.passphrase() {
            Ok(secret) => return Ok((secret, source.method())),
            Err(err) => last = Some(err),
        }
    }
    Err(last.unwrap_or(VaultError::NoUnlockMethod))
}

/// The vault passphrase read from a biometry-gated Keychain item,
/// prompting for Touch ID. On non-macOS builds the read reports
/// the method unsupported, so the resolver falls through.
pub struct TouchidSource {
    store_root: PathBuf,
}

impl TouchidSource {
    pub fn new(store_root: impl Into<PathBuf>) -> TouchidSource {
        TouchidSource {
            store_root: store_root.into(),
        }
    }
}

impl SecretSource for TouchidSource {
    fn method(&self) -> &'static str {
        "touchid"
    }

    fn passphrase(&self) -> Result<SecretString, VaultError> {
        touchid::read_passphrase(&self.store_root)
    }
}

/// The vault passphrase recovered through a YubiKey's FIDO2
/// hmac-secret, prompting for a touch. The PIN comes from the
/// same command that yields a passphrase, so one prompt path
/// serves both; on a host without the key the read reports the
/// method unsupported and the resolver falls through.
pub struct YubikeySource {
    enrol_dir: PathBuf,
    pin_command: String,
}

impl YubikeySource {
    pub fn new(
        enrol_dir: impl Into<PathBuf>,
        pin_command: impl Into<String>,
    ) -> YubikeySource {
        YubikeySource {
            enrol_dir: enrol_dir.into(),
            pin_command: pin_command.into(),
        }
    }
}

impl SecretSource for YubikeySource {
    fn method(&self) -> &'static str {
        "yubikey"
    }

    fn passphrase(&self) -> Result<SecretString, VaultError> {
        let pin = passphrase_command(&self.pin_command)?;
        fido2::read_passphrase(&self.enrol_dir, &pin)
    }
}

/// The vault passphrase produced by running `passphrase_cmd`.
pub struct PassphraseCmdSource {
    command: String,
}

impl PassphraseCmdSource {
    pub fn new(command: impl Into<String>) -> PassphraseCmdSource {
        PassphraseCmdSource {
            command: command.into(),
        }
    }
}

impl SecretSource for PassphraseCmdSource {
    fn method(&self) -> &'static str {
        "passphrase"
    }

    fn passphrase(&self) -> Result<SecretString, VaultError> {
        passphrase_command(&self.command)
    }
}

/// Store the passphrase into the Touch ID Keychain item for a
/// store, the one-time enrolment that gives Touch ID something to
/// unlock. Off the hot path; driven by `antiphon vault
/// touchid-enrol`.
pub fn enrol_touchid(
    store_root: &Path,
    secret: &SecretString,
) -> Result<(), VaultError> {
    touchid::store_passphrase(store_root, secret)
}

/// Seal the vault passphrase behind a YubiKey's hmac-secret, the
/// one-time enrolment that gives the key something to unlock.
/// Off the hot path; driven by `antiphon vault yubikey-enrol`.
pub fn enrol_yubikey(
    enrol_dir: &Path,
    secret: &SecretString,
    pin: &SecretString,
) -> Result<(), VaultError> {
    fido2::enrol(enrol_dir, secret, pin)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use secrecy::ExposeSecret;

    use super::*;

    struct Scripted {
        method: &'static str,
        secret: Option<&'static str>,
        calls: Rc<Cell<usize>>,
    }

    impl Scripted {
        fn ok(
            method: &'static str,
            secret: &'static str,
        ) -> (Scripted, Rc<Cell<usize>>) {
            Scripted::new(method, Some(secret))
        }

        fn err(method: &'static str) -> (Scripted, Rc<Cell<usize>>) {
            Scripted::new(method, None)
        }

        fn new(
            method: &'static str,
            secret: Option<&'static str>,
        ) -> (Scripted, Rc<Cell<usize>>) {
            let calls = Rc::new(Cell::new(0));
            let source = Scripted {
                method,
                secret,
                calls: Rc::clone(&calls),
            };
            (source, calls)
        }
    }

    impl SecretSource for Scripted {
        fn method(&self) -> &'static str {
            self.method
        }

        fn passphrase(&self) -> Result<SecretString, VaultError> {
            self.calls.set(self.calls.get() + 1);
            match self.secret {
                Some(secret) => {
                    Ok(SecretString::from(secret.to_owned()))
                }
                None => Err(VaultError::AuthUnsupported(self.method)),
            }
        }
    }

    fn boxed(sources: Vec<Scripted>) -> Vec<Box<dyn SecretSource>> {
        sources
            .into_iter()
            .map(|item| Box::new(item) as Box<dyn SecretSource>)
            .collect()
    }

    #[test]
    fn first_source_that_succeeds_wins() {
        let (touchid, _) = Scripted::ok("touchid", "from-touchid");
        let (secret, method) =
            resolve_passphrase(&boxed(vec![touchid])).unwrap();
        assert_eq!(secret.expose_secret(), "from-touchid");
        assert_eq!(method, "touchid");
    }

    #[test]
    fn touchid_cancel_falls_through_to_passphrase() {
        let (touchid, _) = Scripted::err("touchid");
        let (passphrase, _) =
            Scripted::ok("passphrase", "from-command");
        let (secret, method) =
            resolve_passphrase(&boxed(vec![touchid, passphrase]))
                .unwrap();
        assert_eq!(secret.expose_secret(), "from-command");
        assert_eq!(method, "passphrase", "the winner is reported");
    }

    #[test]
    fn a_working_touchid_never_runs_the_passphrase_command() {
        let (touchid, touchid_calls) =
            Scripted::ok("touchid", "from-touchid");
        let (passphrase, passphrase_calls) =
            Scripted::ok("passphrase", "from-command");
        resolve_passphrase(&boxed(vec![touchid, passphrase])).unwrap();
        assert_eq!(touchid_calls.get(), 1);
        assert_eq!(passphrase_calls.get(), 0);
    }

    #[test]
    fn all_sources_failing_keeps_the_vault_sealed() {
        let (touchid, _) = Scripted::err("touchid");
        let (passphrase, _) = Scripted::err("passphrase");
        let err = resolve_passphrase(&boxed(vec![touchid, passphrase]))
            .unwrap_err();
        assert!(matches!(err, VaultError::AuthUnsupported(_)));
    }

    #[test]
    fn no_sources_reports_no_unlock_method() {
        let list: Vec<Box<dyn SecretSource>> = Vec::new();
        let err = resolve_passphrase(&list).unwrap_err();
        assert!(matches!(err, VaultError::NoUnlockMethod));
    }
}
