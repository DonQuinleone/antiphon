//! YubiKey unlock through the FIDO2 hmac-secret extension. The
//! authenticator, given a stored credential and a random salt,
//! returns a stable 32-byte secret that only that physical key
//! can produce, and only on a touch. Enrolment wraps the vault
//! passphrase under that secret; unlock recovers it, so no key
//! material is ever written to disk in the clear and the vault
//! cannot open without the key in hand.
//!
//! The recovered passphrase then mounts the vault through the
//! ordinary passphrase path, exactly as Touch ID does, so no
//! FIDO2 logic reaches the backends. Every failure (no device,
//! a cancelled touch, a wrong PIN, a missing enrolment) is an
//! error the resolver falls through, never a bypass.

use std::path::{Path, PathBuf};

use chacha20poly1305::aead::rand_core::RngCore;
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use secrecy::{ExposeSecret, SecretString};

use crate::vault::VaultError;

/// The relying-party id the credential is bound to; a constant,
/// since the credential never leaves this host.
const RPID: &str = "antiphon.vault";
const KEYFILE_NAME: &str = "yubikey.enrol";
const MAGIC: &[u8; 4] = b"AVY1";
const SALT_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const CHALLENGE_LEN: usize = 32;

/// The persisted enrolment: the credential to assert against,
/// the salt fed to hmac-secret, and the passphrase sealed under
/// the secret that pair yields.
struct Enrolment {
    credential_id: Vec<u8>,
    salt: [u8; SALT_LEN],
    nonce: [u8; NONCE_LEN],
    ciphertext: Vec<u8>,
}

/// Enrol: create an hmac-secret credential, seal the vault
/// passphrase under the secret it derives for a fresh salt, and
/// write the enrolment. Both device steps need a touch; the PIN
/// authorises them. Off the unlock hot path.
pub fn enrol(
    store_root: &Path,
    secret: &SecretString,
    pin: &SecretString,
) -> Result<(), VaultError> {
    let credential_id = make_credential(pin.expose_secret())?;
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    let key = hmac_secret(&credential_id, &salt, pin.expose_secret())?;
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext =
        seal(&key, &nonce, secret.expose_secret().as_bytes())?;
    let enrolment = Enrolment {
        credential_id,
        salt,
        nonce: nonce.into(),
        ciphertext,
    };
    std::fs::write(keyfile(store_root), encode(&enrolment))
        .map_err(VaultError::Io)
}

/// Unlock: read the enrolment, re-derive the secret with a touch,
/// and open the sealed passphrase. A wrong key derives a
/// different secret, so the open fails rather than yielding a
/// bad passphrase.
pub fn read_passphrase(
    store_root: &Path,
    pin: &SecretString,
) -> Result<SecretString, VaultError> {
    let bytes =
        std::fs::read(keyfile(store_root)).map_err(VaultError::Io)?;
    let enrolment = decode(&bytes)?;
    let key = hmac_secret(
        &enrolment.credential_id,
        &enrolment.salt,
        pin.expose_secret(),
    )?;
    let plaintext =
        open(&key, &enrolment.nonce, &enrolment.ciphertext)?;
    let text = String::from_utf8(plaintext).map_err(|_| {
        VaultError::Fido2(
            "the recovered passphrase is not valid UTF-8".to_owned(),
        )
    })?;
    Ok(SecretString::from(text))
}

fn keyfile(store_root: &Path) -> PathBuf {
    store_root.join(KEYFILE_NAME)
}

fn seal(
    key: &[u8; 32],
    nonce: &Nonce,
    plaintext: &[u8],
) -> Result<Vec<u8>, VaultError> {
    ChaCha20Poly1305::new(Key::from_slice(key))
        .encrypt(nonce, plaintext)
        .map_err(|_| {
            VaultError::Fido2(
                "sealing the passphrase failed".to_owned(),
            )
        })
}

fn open(
    key: &[u8; 32],
    nonce: &[u8; NONCE_LEN],
    ciphertext: &[u8],
) -> Result<Vec<u8>, VaultError> {
    ChaCha20Poly1305::new(Key::from_slice(key))
        .decrypt(Nonce::from_slice(nonce), ciphertext)
        .map_err(|_| {
            VaultError::Fido2(
                "the YubiKey secret did not match this enrolment"
                    .to_owned(),
            )
        })
}

fn encode(enrolment: &Enrolment) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&enrolment.salt);
    out.extend_from_slice(&enrolment.nonce);
    let len = enrolment.credential_id.len() as u16;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&enrolment.credential_id);
    out.extend_from_slice(&enrolment.ciphertext);
    out
}

fn decode(bytes: &[u8]) -> Result<Enrolment, VaultError> {
    let mut cursor = bytes;
    if take(&mut cursor, MAGIC.len())? != MAGIC {
        return Err(corrupt("not a YubiKey enrolment"));
    }
    let salt = take(&mut cursor, SALT_LEN)?
        .try_into()
        .expect("checked length");
    let nonce = take(&mut cursor, NONCE_LEN)?
        .try_into()
        .expect("checked length");
    let len_bytes = take(&mut cursor, 2)?;
    let len = u16::from_be_bytes([len_bytes[0], len_bytes[1]]) as usize;
    let credential_id = take(&mut cursor, len)?.to_vec();
    if cursor.is_empty() {
        return Err(corrupt("missing ciphertext"));
    }
    Ok(Enrolment {
        credential_id,
        salt,
        nonce,
        ciphertext: cursor.to_vec(),
    })
}

fn take<'a>(
    cursor: &mut &'a [u8],
    n: usize,
) -> Result<&'a [u8], VaultError> {
    if cursor.len() < n {
        return Err(corrupt("truncated"));
    }
    let (head, tail) = cursor.split_at(n);
    *cursor = tail;
    Ok(head)
}

fn corrupt(what: &str) -> VaultError {
    VaultError::Fido2(format!("enrolment file: {what}"))
}

/// Register a credential carrying the hmac-secret extension and
/// return its id. Needs a touch; the PIN authorises it.
fn make_credential(pin: &str) -> Result<Vec<u8>, VaultError> {
    use ctap_hid_fido2::fidokey::MakeCredentialArgsBuilder;
    use ctap_hid_fido2::fidokey::make_credential::make_credential_params::Extension;

    let device = open_device()?;
    let challenge = random_challenge();
    let args = MakeCredentialArgsBuilder::new(RPID, &challenge)
        .pin(pin)
        .extensions(&[Extension::HmacSecret(Some(true))])
        .build();
    let attestation = device
        .make_credential_with_args(&args)
        .map_err(|error| VaultError::Fido2(error.to_string()))?;
    Ok(attestation.credential_descriptor.id)
}

/// Assert against the credential to read hmac-secret(salt): the
/// 32-byte secret only this key produces, and only on a touch.
fn hmac_secret(
    credential_id: &[u8],
    salt: &[u8; SALT_LEN],
    pin: &str,
) -> Result<[u8; 32], VaultError> {
    use ctap_hid_fido2::fidokey::GetAssertionArgsBuilder;
    use ctap_hid_fido2::fidokey::get_assertion::get_assertion_params::Extension;

    let device = open_device()?;
    let challenge = random_challenge();
    let args = GetAssertionArgsBuilder::new(RPID, &challenge)
        .pin(pin)
        .credential_id(credential_id)
        .extensions(&[Extension::HmacSecret(Some(*salt))])
        .build();
    let assertions = device
        .get_assertion_with_args(&args)
        .map_err(|error| VaultError::Fido2(error.to_string()))?;
    for assertion in assertions {
        for extension in assertion.extensions {
            if let Extension::HmacSecret(Some(output)) = extension {
                return Ok(output);
            }
        }
    }
    Err(VaultError::Fido2(
        "the key returned no hmac-secret; enrol the credential \
         with the extension"
            .to_owned(),
    ))
}

fn open_device() -> Result<ctap_hid_fido2::FidoKeyHid, VaultError> {
    ctap_hid_fido2::FidoKeyHidFactory::create(
        &ctap_hid_fido2::Cfg::init(),
    )
    .map_err(|error| {
        VaultError::Fido2(format!("no FIDO2 device: {error}"))
    })
}

fn random_challenge() -> Vec<u8> {
    let mut challenge = vec![0u8; CHALLENGE_LEN];
    OsRng.fill_bytes(&mut challenge);
    challenge
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Enrolment {
        Enrolment {
            credential_id: vec![1, 2, 3, 4, 5],
            salt: [7u8; SALT_LEN],
            nonce: [9u8; NONCE_LEN],
            ciphertext: vec![10, 11, 12],
        }
    }

    #[test]
    fn seal_then_open_round_trips_under_the_same_key() {
        let key = [42u8; 32];
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
        let sealed = seal(&key, &nonce, b"correct horse").unwrap();
        let nonce_bytes: [u8; NONCE_LEN] = nonce.into();
        let opened = open(&key, &nonce_bytes, &sealed).unwrap();
        assert_eq!(opened, b"correct horse");
    }

    #[test]
    fn a_different_key_never_opens_the_seal() {
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
        let sealed = seal(&[1u8; 32], &nonce, b"secret").unwrap();
        let nonce_bytes: [u8; NONCE_LEN] = nonce.into();
        let error =
            open(&[2u8; 32], &nonce_bytes, &sealed).unwrap_err();
        assert!(matches!(error, VaultError::Fido2(_)));
    }

    #[test]
    fn encode_then_decode_preserves_every_field() {
        let original = sample();
        let decoded = decode(&encode(&original)).unwrap();
        assert_eq!(decoded.credential_id, original.credential_id);
        assert_eq!(decoded.salt, original.salt);
        assert_eq!(decoded.nonce, original.nonce);
        assert_eq!(decoded.ciphertext, original.ciphertext);
    }

    #[test]
    fn decode_rejects_a_bad_magic() {
        let mut bytes = encode(&sample());
        bytes[0] = b'X';
        assert!(matches!(decode(&bytes), Err(VaultError::Fido2(_))));
    }

    #[test]
    fn decode_rejects_a_truncated_file() {
        let bytes = encode(&sample());
        assert!(matches!(
            decode(&bytes[..10]),
            Err(VaultError::Fido2(_))
        ));
    }

    #[test]
    fn decode_rejects_a_missing_ciphertext() {
        let mut enrolment = sample();
        enrolment.ciphertext.clear();
        assert!(matches!(
            decode(&encode(&enrolment)),
            Err(VaultError::Fido2(_))
        ));
    }
}
