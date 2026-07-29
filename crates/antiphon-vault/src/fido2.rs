//! YubiKey unlock through the FIDO2 hmac-secret extension. The
//! authenticator, given a stored credential and a salt, returns a
//! stable 32-byte secret that only that physical key can produce,
//! and only on a touch. Enrolment wraps the vault passphrase
//! under that secret; unlock recovers it, so no key material is
//! ever written to disk in the clear and the vault cannot open
//! without the key in hand.
//!
//! More than one key can enrol for backup. Every key shares one
//! salt but derives its own secret (its credential differs), so
//! unlock sends every credential in a single assertion: the key
//! in the slot matches its own and opens its entry with one
//! touch, whichever key it is.
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
const MAGIC: &[u8; 4] = b"AVY3";
const SALT_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const CHALLENGE_LEN: usize = 32;

/// One key's entry: the credential to assert against and the
/// passphrase sealed under the secret it derives from the shared
/// salt.
struct Entry {
    credential_id: Vec<u8>,
    nonce: [u8; NONCE_LEN],
    ciphertext: Vec<u8>,
}

/// The enrolment file: one salt shared by every key, and an entry
/// per enrolled key.
struct Keyfile {
    salt: [u8; SALT_LEN],
    entries: Vec<Entry>,
}

/// Enrol a key: seal the vault passphrase under the secret it
/// derives from the enrolment's salt and add it to any keys
/// already enrolled, so a second call registers a backup rather
/// than replacing the first. The first key sets the shared salt;
/// later keys reuse it. Both device steps need a touch; the PIN
/// authorises them. Off the unlock hot path.
pub fn enrol(
    dir: &Path,
    secret: &SecretString,
    pin: &SecretString,
) -> Result<(), VaultError> {
    let existing = load_keyfile(dir);
    let salt = existing.as_ref().map_or_else(random_salt, |k| k.salt);
    let credential_id = make_credential(pin.expose_secret())?;
    let key = hmac_secret(&credential_id, &salt, pin.expose_secret())?;
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext =
        seal(&key, &nonce, secret.expose_secret().as_bytes())?;
    let entry = Entry {
        credential_id,
        nonce: nonce.into(),
        ciphertext,
    };
    let mut keyfile = existing.unwrap_or_else(|| Keyfile {
        salt,
        entries: vec![],
    });
    keyfile.entries.push(entry);
    std::fs::create_dir_all(dir).map_err(VaultError::Io)?;
    std::fs::write(keyfile_path(dir), encode(&keyfile))
        .map_err(VaultError::Io)
}

/// Unlock: one assertion over every enrolled credential, so the
/// key in the slot matches its own with a single touch; its entry
/// then opens the sealed passphrase.
pub fn read_passphrase(
    dir: &Path,
    pin: &SecretString,
) -> Result<SecretString, VaultError> {
    let bytes =
        std::fs::read(keyfile_path(dir)).map_err(VaultError::Io)?;
    let keyfile = decode(&bytes)?;
    let (matched, secret) = assert_any(&keyfile, pin.expose_secret())?;
    let entry = keyfile
        .entries
        .iter()
        .find(|entry| entry.credential_id == matched)
        .ok_or_else(|| {
            VaultError::Fido2(
                "the key asserted an unknown credential".to_owned(),
            )
        })?;
    let plaintext = open(&secret, &entry.nonce, &entry.ciphertext)?;
    let text = String::from_utf8(plaintext).map_err(|_| {
        VaultError::Fido2(
            "the recovered passphrase is not valid UTF-8".to_owned(),
        )
    })?;
    Ok(SecretString::from(text))
}

fn load_keyfile(dir: &Path) -> Option<Keyfile> {
    std::fs::read(keyfile_path(dir))
        .ok()
        .and_then(|bytes| decode(&bytes).ok())
}

fn keyfile_path(dir: &Path) -> PathBuf {
    dir.join(KEYFILE_NAME)
}

fn random_salt() -> [u8; SALT_LEN] {
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    salt
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

fn encode(keyfile: &Keyfile) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&keyfile.salt);
    out.extend_from_slice(
        &(keyfile.entries.len() as u16).to_be_bytes(),
    );
    for entry in &keyfile.entries {
        push_field(&mut out, &entry.credential_id);
        out.extend_from_slice(&entry.nonce);
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

fn decode(bytes: &[u8]) -> Result<Keyfile, VaultError> {
    let mut cursor = bytes;
    if take(&mut cursor, MAGIC.len())? != MAGIC {
        return Err(corrupt("not a current enrolment; re-enrol"));
    }
    let salt = take(&mut cursor, SALT_LEN)?
        .try_into()
        .expect("checked length");
    let count = u16::from_be_bytes(
        take(&mut cursor, 2)?.try_into().expect("checked length"),
    );
    let mut entries = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let credential_id = take_field(&mut cursor)?;
        let nonce = take(&mut cursor, NONCE_LEN)?
            .try_into()
            .expect("checked length");
        let ciphertext = take_field(&mut cursor)?;
        entries.push(Entry {
            credential_id,
            nonce,
            ciphertext,
        });
    }
    Ok(Keyfile { salt, entries })
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

/// Assert against one credential to read hmac-secret(salt) at
/// enrolment, when the key being enrolled is the one present.
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
    hmac_output(assertions.into_iter().next())
}

/// Assert over every enrolled credential at once: the present key
/// matches its own and returns hmac-secret(salt) with one touch.
/// The matched credential id says which entry to open.
fn assert_any(
    keyfile: &Keyfile,
    pin: &str,
) -> Result<(Vec<u8>, [u8; 32]), VaultError> {
    use ctap_hid_fido2::fidokey::GetAssertionArgsBuilder;
    use ctap_hid_fido2::fidokey::get_assertion::get_assertion_params::Extension;

    let device = open_device()?;
    let challenge = random_challenge();
    let mut builder = GetAssertionArgsBuilder::new(RPID, &challenge)
        .pin(pin)
        .extensions(&[Extension::HmacSecret(Some(keyfile.salt))]);
    for entry in &keyfile.entries {
        builder = builder.add_credential_id(&entry.credential_id);
    }
    let assertions =
        device
            .get_assertion_with_args(&builder.build())
            .map_err(|error| VaultError::Fido2(error.to_string()))?;
    let assertion = assertions.into_iter().next().ok_or_else(|| {
        VaultError::Fido2("no enrolled key is present".to_owned())
    })?;
    let credential_id = assertion.credential_id.clone();
    let secret = hmac_output(Some(assertion))?;
    Ok((credential_id, secret))
}

/// The 32-byte hmac-secret output carried on an assertion, or an
/// error when none is present.
fn hmac_output(
    assertion: Option<
        ctap_hid_fido2::fidokey::get_assertion::get_assertion_params::Assertion,
    >,
) -> Result<[u8; 32], VaultError> {
    use ctap_hid_fido2::fidokey::get_assertion::get_assertion_params::Extension;

    let assertion = assertion.ok_or_else(|| {
        VaultError::Fido2("the key returned no assertion".to_owned())
    })?;
    assertion
        .extensions
        .iter()
        .find_map(|extension| match extension {
            Extension::HmacSecret(Some(output)) => Some(*output),
            _ => None,
        })
        .ok_or_else(|| {
            VaultError::Fido2(
                "the key returned no hmac-secret; enrol the \
                 credential with the extension"
                    .to_owned(),
            )
        })
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

    fn sample() -> Keyfile {
        Keyfile {
            salt: [7u8; SALT_LEN],
            entries: vec![
                Entry {
                    credential_id: vec![1, 2, 3, 4, 5],
                    nonce: [9u8; NONCE_LEN],
                    ciphertext: vec![10, 11, 12],
                },
                Entry {
                    credential_id: vec![9, 9],
                    nonce: [2u8; NONCE_LEN],
                    ciphertext: vec![3, 4, 5, 6, 7, 8],
                },
            ],
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
    fn encode_then_decode_round_trips_salt_and_every_entry() {
        let decoded = decode(&encode(&sample())).unwrap();
        assert_eq!(decoded.salt, [7u8; SALT_LEN]);
        assert_eq!(decoded.entries.len(), 2);
        assert_eq!(
            decoded.entries[0].credential_id,
            vec![1, 2, 3, 4, 5]
        );
        assert_eq!(decoded.entries[0].nonce, [9u8; NONCE_LEN]);
        assert_eq!(
            decoded.entries[1].ciphertext,
            vec![3, 4, 5, 6, 7, 8]
        );
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
            decode(&bytes[..20]),
            Err(VaultError::Fido2(_))
        ));
    }

    #[test]
    fn decode_rejects_a_field_running_past_the_end() {
        let mut bytes = encode(&sample());
        // Overstate the first credential field's length so it
        // claims more bytes than remain.
        let cred_len_at = MAGIC.len() + SALT_LEN + 2;
        bytes[cred_len_at..cred_len_at + 4]
            .copy_from_slice(&9999u32.to_be_bytes());
        assert!(matches!(decode(&bytes), Err(VaultError::Fido2(_))));
    }
}
