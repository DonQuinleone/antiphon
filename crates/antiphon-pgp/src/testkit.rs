use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use sequoia_openpgp::Cert;
use sequoia_openpgp::armor::Kind;
use sequoia_openpgp::cert::CertBuilder;
use sequoia_openpgp::crypto::{SessionKey, SymmetricAlgorithm};
use sequoia_openpgp::packet::{PKESK, SKESK};
use sequoia_openpgp::parse::Parse;
use sequoia_openpgp::parse::stream::{
    DecryptionHelper, DecryptorBuilder, MessageStructure,
    VerificationHelper,
};
use sequoia_openpgp::policy::StandardPolicy;
use sequoia_openpgp::serialize::SerializeInto;
use sequoia_openpgp::serialize::stream::{Armorer, Message, Signer};
use sequoia_openpgp::{KeyHandle, crypto::KeyPair};

use crate::keyring::Keyring;

pub(crate) struct TempDir {
    pub(crate) path: PathBuf,
}

impl TempDir {
    pub(crate) fn new() -> TempDir {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
        let name =
            format!("antiphon-pgp-{}-{nonce}", std::process::id());
        let path = std::env::temp_dir().join(name);
        std::fs::create_dir_all(&path).unwrap();
        TempDir { path }
    }

    pub(crate) fn write(&self, name: &str, bytes: &[u8]) {
        std::fs::write(self.path.join(name), bytes).unwrap();
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

pub(crate) fn cert(user_id: &str) -> Cert {
    CertBuilder::general_purpose(Some(user_id))
        .generate()
        .unwrap()
        .0
}

pub(crate) fn signing_only_cert(user_id: &str) -> Cert {
    CertBuilder::new()
        .add_userid(user_id)
        .add_signing_subkey()
        .generate()
        .unwrap()
        .0
}

pub(crate) fn keyring_with(cert: &Cert) -> (TempDir, Keyring) {
    let dir = TempDir::new();
    dir.write("trusted.asc", &cert.armored().to_vec().unwrap());
    let keyring = Keyring::from_dir(&dir.path);
    (dir, keyring)
}

pub(crate) fn empty_keyring() -> (TempDir, Keyring) {
    let dir = TempDir::new();
    let keyring = Keyring::from_dir(&dir.path);
    (dir, keyring)
}

pub(crate) fn signing_keypair(cert: &Cert) -> KeyPair {
    let policy = StandardPolicy::new();
    cert.keys()
        .unencrypted_secret()
        .with_policy(&policy, None)
        .alive()
        .revoked(false)
        .for_signing()
        .next()
        .unwrap()
        .key()
        .clone()
        .into_keypair()
        .unwrap()
}

pub(crate) fn detached_signature(cert: &Cert, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let message = Message::new(&mut out);
    let message =
        Armorer::new(message).kind(Kind::Signature).build().unwrap();
    let mut signer = Signer::new(message, signing_keypair(cert))
        .unwrap()
        .detached()
        .build()
        .unwrap();
    signer.write_all(data).unwrap();
    signer.finalize().unwrap();
    out
}

pub(crate) fn decrypt_with(cert: &Cert, ciphertext: &[u8]) -> Vec<u8> {
    let policy = StandardPolicy::new();
    let helper = SecretKeys { cert: cert.clone() };
    let mut reader = DecryptorBuilder::from_bytes(ciphertext)
        .unwrap()
        .with_policy(&policy, None, helper)
        .unwrap();
    let mut plaintext = Vec::new();
    reader.read_to_end(&mut plaintext).unwrap();
    plaintext
}

struct SecretKeys {
    cert: Cert,
}

impl VerificationHelper for SecretKeys {
    fn get_certs(
        &mut self,
        _ids: &[KeyHandle],
    ) -> sequoia_openpgp::Result<Vec<Cert>> {
        Ok(Vec::new())
    }

    fn check(
        &mut self,
        _structure: MessageStructure,
    ) -> sequoia_openpgp::Result<()> {
        Ok(())
    }
}

impl DecryptionHelper for SecretKeys {
    fn decrypt(
        &mut self,
        pkesks: &[PKESK],
        _skesks: &[SKESK],
        sym_algo: Option<SymmetricAlgorithm>,
        decrypt: &mut dyn FnMut(
            Option<SymmetricAlgorithm>,
            &SessionKey,
        ) -> bool,
    ) -> sequoia_openpgp::Result<Option<Cert>> {
        let policy = StandardPolicy::new();
        let keys: Vec<KeyPair> = self
            .cert
            .keys()
            .unencrypted_secret()
            .with_policy(&policy, None)
            .supported()
            .for_transport_encryption()
            .filter_map(|ka| ka.key().clone().into_keypair().ok())
            .collect();
        for pkesk in pkesks {
            for mut keypair in keys.clone() {
                let Some((algo, session_key)) =
                    pkesk.decrypt(&mut keypair, sym_algo)
                else {
                    continue;
                };
                if decrypt(algo, &session_key) {
                    return Ok(None);
                }
            }
        }
        Err(anyhow::anyhow!("no matching decryption key"))
    }
}
