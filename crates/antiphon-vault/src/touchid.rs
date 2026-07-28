//! macOS Touch ID unlock. The vault passphrase is kept as a
//! biometry-gated Keychain generic password: storing it is a
//! one-time enrolment, and reading it back triggers the Touch ID
//! prompt. The retrieved secret then mounts the vault through the
//! ordinary passphrase path, so no biometric logic reaches the
//! backends.
//!
//! The item is bound to the currently enrolled fingerprints
//! (`BIOMETRY_CURRENT_SET`): adding or removing a finger
//! invalidates it, so a newly enrolled print on a stolen machine
//! cannot open the vault. Re-enrol after changing fingerprints.

use std::path::Path;

use secrecy::SecretString;

use crate::vault::VaultError;

/// Every Antiphon vault item shares this Keychain service; the
/// store root disambiguates one vault from another on the same
/// login keychain.
#[cfg(target_os = "macos")]
const KEYCHAIN_SERVICE: &str = "antiphon vault";

/// The Keychain account is the store root path, the same value
/// the daemon derives from its layout, so enrolment and unlock
/// always name the same item.
#[cfg(target_os = "macos")]
fn account(store_root: &Path) -> String {
    store_root.display().to_string()
}

/// Enrol: write the vault passphrase into the biometric Keychain
/// item, replacing any prior one so its access control reflects
/// the current fingerprint set. Kept off the unlock hot path.
#[cfg(target_os = "macos")]
pub fn store_passphrase(
    store_root: &Path,
    secret: &SecretString,
) -> Result<(), VaultError> {
    macos::store(&account(store_root), secret)
}

/// Unlock: read the passphrase back, prompting for Touch ID. Any
/// failure (cancel, no biometry, item absent) is an error the
/// resolver falls through, never a bypass.
#[cfg(target_os = "macos")]
pub fn read_passphrase(
    store_root: &Path,
) -> Result<SecretString, VaultError> {
    macos::read(&account(store_root))
}

#[cfg(not(target_os = "macos"))]
pub fn store_passphrase(
    _store_root: &Path,
    _secret: &SecretString,
) -> Result<(), VaultError> {
    Err(VaultError::AuthUnsupported("touchid"))
}

#[cfg(not(target_os = "macos"))]
pub fn read_passphrase(
    _store_root: &Path,
) -> Result<SecretString, VaultError> {
    Err(VaultError::AuthUnsupported("touchid"))
}

#[cfg(target_os = "macos")]
mod macos {
    use secrecy::{ExposeSecret, SecretString};
    use security_framework::base::Error as SecError;
    use security_framework::passwords::{
        AccessControlOptions, PasswordOptions, delete_generic_password,
        generic_password, set_generic_password_options,
    };

    use super::KEYCHAIN_SERVICE;
    use crate::vault::VaultError;

    const ITEM_LABEL: &str = "Antiphon vault passphrase";

    pub fn store(
        account: &str,
        secret: &SecretString,
    ) -> Result<(), VaultError> {
        // A prior item may carry a stale ACL from an older
        // fingerprint set, so replace rather than update.
        let _ = delete_generic_password(KEYCHAIN_SERVICE, account);
        let mut options = PasswordOptions::new_generic_password(
            KEYCHAIN_SERVICE,
            account,
        );
        options.set_access_control_options(
            AccessControlOptions::BIOMETRY_CURRENT_SET,
        );
        options.set_label(ITEM_LABEL);
        set_generic_password_options(
            secret.expose_secret().as_bytes(),
            options,
        )
        .map_err(map_error)
    }

    pub fn read(account: &str) -> Result<SecretString, VaultError> {
        let options = PasswordOptions::new_generic_password(
            KEYCHAIN_SERVICE,
            account,
        );
        let bytes = generic_password(options).map_err(map_error)?;
        let text = String::from_utf8(bytes).map_err(|_| {
            VaultError::Touchid(
                "the stored passphrase is not valid UTF-8".to_owned(),
            )
        })?;
        Ok(SecretString::from(text))
    }

    fn map_error(err: SecError) -> VaultError {
        VaultError::Touchid(format!(
            "keychain returned OSStatus {}",
            err.code()
        ))
    }
}
