//! YubiKey unlock through the FIDO2 hmac-secret extension. The
//! authenticator, given a stored credential and a random salt,
//! returns a stable 32-byte secret that only that physical key
//! can produce, and only on a touch. Enrolment wraps the vault
//! passphrase under that secret; unlock recovers it, so no key
//! material is ever written to disk in the clear and the vault
//! cannot open without the key in hand.
//!
//! More than one key can enrol: each seals the same passphrase
//! under its own secret, so a primary and a backup both unlock
//! independently. Unlock tries each enrolment until the key in
//! the slot opens one.
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
/// The original single-key file; still read so a keyfile written
/// before backup keys keeps unlocking.
const MAGIC_V1: &[u8; 4] = b"AVY1";
/// The multi-key file: a count then that many enrolments.
const MAGIC_V2: &[u8; 4] = b"AVY2";
const SALT_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const CHALLENGE_LEN: usize = 32;

/// One key's enrolment: the credential to assert against, the
/// salt fed to hmac-secret, and the passphrase sealed under the
/// secret that pair yields.
struct Enrolment {
    credential_id: Vec<u8>,
    salt: [u8; SALT_LEN],
    nonce: [u8; NONCE_LEN],
    ciphertext: Vec<u8>,
}

/// Enrol a key: seal the vault passphrase under a fresh
/// hmac-secret credential and add it to any keys already
/// enrolled, so a second call registers a backup rather than
/// replacing the first. Both device steps need a touch; the PIN
/// authorises them. Off the unlock hot path.
pub fn enrol(
    dir: &Path,
    secret: &SecretString,
    pin: &SecretString,
) -> Result<(), VaultError> {
    let entry = make_entry(secret, pin)?;
    let mut entries = load_entries(dir);
    entries.push(entry);
    std::fs::create_dir_all(dir).map_err(VaultError::Io)?;
    std::fs::write(keyfile(dir), encode_all(&entries))
        .map_err(VaultError::Io)
}

/// Unlock: try each enrolment in turn, re-deriving its secret
/// with a touch and opening its sealed passphrase. A key that is
/// not the enrolled one asserts nothing, so that entry errors and
/// the next is tried; the key in the slot opens its own.
pub fn read_passphrase(
    dir: &Path,
    pin: &SecretString,
) -> Result<SecretString, VaultError> {
    let bytes = std::fs::read(keyfile(dir)).map_err(VaultError::Io)?;
    let entries = decode_all(&bytes)?;
    let mut last: Option<VaultError> = None;
    for entry in &entries {
        match open_entry(entry, pin.expose_secret()) {
            Ok(secret) => return Ok(secret),
            Err(error) => last = Some(error),
        }
    }
    Err(last.unwrap_or_else(|| {
        VaultError::Fido2("the enrolment holds no keys".to_owned())
    }))
}

/// Register a fresh credential and seal the passphrase under the
/// secret it derives, for one key.
fn make_entry(
    secret: &SecretString,
    pin: &SecretString,
) -> Result<Enrolment, VaultError> {
    let credential_id = make_credential(pin.expose_secret())?;
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    let key = hmac_secret(&credential_id, &salt, pin.expose_secret())?;
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext =
        seal(&key, &nonce, secret.expose_secret().as_bytes())?;
    Ok(Enrolment {
        credential_id,
        salt,
        nonce: nonce.into(),
        ciphertext,
    })
}

/// Re-derive one enrolment's secret with a touch and open its
/// sealed passphrase.
fn open_entry(
    entry: &Enrolment,
    pin: &str,
) -> Result<SecretString, VaultError> {
    let key = hmac_secret(&entry.credential_id, &entry.salt, pin)?;
    let plaintext = open(&key, &entry.nonce, &entry.ciphertext)?;
    let text = String::from_utf8(plaintext).map_err(|_| {
        VaultError::Fido2(
            "the recovered passphrase is not valid UTF-8".to_owned(),
        )
    })?;
    Ok(SecretString::from(text))
}

/// The enrolments already on disk, empty when the file is absent
/// or unreadable, so enrol adds to what is there.
fn load_entries(dir: &Path) -> Vec<Enrolment> {
    std::fs::read(keyfile(dir))
        .ok()
        .and_then(|bytes| decode_all(&bytes).ok())
        .unwrap_or_default()
}

fn keyfile(dir: &Path) -> PathBuf {
    dir.join(KEYFILE_NAME)
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

fn encode_all(entries: &[Enrolment]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC_V2);
    out.extend_from_slice(&(entries.len() as u16).to_be_bytes());
    for entry in entries {
        out.extend_from_slice(&entry.salt);
        out.extend_from_slice(&entry.nonce);
        push_field(&mut out, &entry.credential_id);
        push_field(&mut out, &entry.ciphertext);
    }
    out
}

/// A length-prefixed byte field: a 4-byte big-endian length then
/// the bytes, so variable-length fields pack unambiguously.
fn push_field(out: &mut Vec<u8>, field: &[u8]) {
    out.extend_from_slice(&(field.len() as u32).to_be_bytes());
    out.extend_from_slice(field);
}

fn decode_all(bytes: &[u8]) -> Result<Vec<Enrolment>, VaultError> {
    let mut cursor = bytes;
    let magic = take(&mut cursor, MAGIC_V2.len())?;
    if magic == MAGIC_V1 {
        return Ok(vec![decode_v1(cursor)?]);
    }
    if magic != MAGIC_V2 {
        return Err(corrupt("not a YubiKey enrolment"));
    }
    let count = u16::from_be_bytes(
        take(&mut cursor, 2)?.try_into().expect("checked length"),
    );
    let mut entries = Vec::with_capacity(count as usize);
    for _ in 0..count {
        entries.push(Enrolment {
            salt: take(&mut cursor, SALT_LEN)?
                .try_into()
                .expect("checked length"),
            nonce: take(&mut cursor, NONCE_LEN)?
                .try_into()
                .expect("checked length"),
            credential_id: take_field(&mut cursor)?,
            ciphertext: take_field(&mut cursor)?,
        });
    }
    Ok(entries)
}

/// The pre-backup single-key body (magic already consumed): salt,
/// nonce, a 2-byte-prefixed credential, then the ciphertext to
/// the end.
fn decode_v1(mut cursor: &[u8]) -> Result<Enrolment, VaultError> {
    let salt = take(&mut cursor, SALT_LEN)?
        .try_into()
        .expect("checked length");
    let nonce = take(&mut cursor, NONCE_LEN)?
        .try_into()
        .expect("checked length");
    let len = u16::from_be_bytes(
        take(&mut cursor, 2)?.try_into().expect("checked length"),
    ) as usize;
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

fn take_field(cursor: &mut &[u8]) -> Result<Vec<u8>, VaultError> {
    let len = u32::from_be_bytes(
        take(cursor, 4)?.try_into().expect("checked length"),
    ) as usize;
    Ok(take(cursor, len)?.to_vec())
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
    let mut cfg = ctap_hid_fido2::Cfg::init();
    // The library reprints "Touch the sensor..." on every
    // keep-alive tick; our own prompt says it once, so quiet the
    // repeats.
    cfg.enable_keep_alive_msg = false;
    ctap_hid_fido2::FidoKeyHidFactory::create(&cfg).map_err(|error| {
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

    fn other() -> Enrolment {
        Enrolment {
            credential_id: vec![9, 9],
            salt: [1u8; SALT_LEN],
            nonce: [2u8; NONCE_LEN],
            ciphertext: vec![3, 4, 5, 6, 7, 8],
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
    fn encode_then_decode_round_trips_every_entry() {
        let entries = vec![sample(), other()];
        let decoded = decode_all(&encode_all(&entries)).unwrap();
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].credential_id, sample().credential_id);
        assert_eq!(decoded[0].nonce, sample().nonce);
        assert_eq!(decoded[1].salt, other().salt);
        assert_eq!(decoded[1].ciphertext, other().ciphertext);
    }

    #[test]
    fn decode_reads_an_old_single_key_file() {
        // The pre-backup format: magic, salt, nonce, a 2-byte
        // credential length, the credential, then the ciphertext
        // to the end.
        let mut v1 = Vec::new();
        v1.extend_from_slice(MAGIC_V1);
        v1.extend_from_slice(&[7u8; SALT_LEN]);
        v1.extend_from_slice(&[9u8; NONCE_LEN]);
        v1.extend_from_slice(&5u16.to_be_bytes());
        v1.extend_from_slice(&[1, 2, 3, 4, 5]);
        v1.extend_from_slice(&[10, 11, 12]);
        let decoded = decode_all(&v1).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].credential_id, vec![1, 2, 3, 4, 5]);
        assert_eq!(decoded[0].ciphertext, vec![10, 11, 12]);
    }

    #[test]
    fn decode_rejects_a_bad_magic() {
        let mut bytes = encode_all(&[sample()]);
        bytes[0] = b'X';
        assert!(matches!(
            decode_all(&bytes),
            Err(VaultError::Fido2(_))
        ));
    }

    #[test]
    fn decode_rejects_a_truncated_file() {
        let bytes = encode_all(&[sample()]);
        assert!(matches!(
            decode_all(&bytes[..10]),
            Err(VaultError::Fido2(_))
        ));
    }

    #[test]
    fn decode_rejects_a_field_running_past_the_end() {
        let mut bytes = encode_all(&[sample()]);
        // Overstate the final ciphertext field's length so it
        // claims more bytes than remain.
        let ct_len_at = bytes.len() - sample().ciphertext.len() - 4;
        bytes[ct_len_at..ct_len_at + 4]
            .copy_from_slice(&9999u32.to_be_bytes());
        assert!(matches!(
            decode_all(&bytes),
            Err(VaultError::Fido2(_))
        ));
    }
}
