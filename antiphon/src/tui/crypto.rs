use std::path::Path;

use antiphon_pgp::{Cert, Keyring, Signature, mime};
use antiphon_pgp_agent::GpgAgent;

/// What OpenPGP protection the finished compose receives,
/// resolved from the identity default and any per-message
/// toggles before the editor opens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PgpPlan {
    pub sign: bool,
    pub encrypt: bool,
}

impl PgpPlan {
    pub fn label(&self) -> Option<&'static str> {
        match (self.sign, self.encrypt) {
            (false, false) => None,
            (true, false) => Some("[sign]"),
            (false, true) => Some("[encrypt]"),
            (true, true) => Some("[sign+encrypt]"),
        }
    }

    fn is_plain(&self) -> bool {
        !self.sign && !self.encrypt
    }
}

/// Everything the finish step needs to seal a compose: the
/// plan plus the sending identity's signer configuration.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComposeCrypto {
    pub plan: PgpPlan,
    pub key: Option<String>,
    pub address: String,
}

/// Applies the compose plan to an assembled message: signs
/// and/or encrypts per RFC 3156, or passes plain bytes through
/// untouched. Any failure aborts the send; the caller keeps
/// the draft.
pub fn seal(
    raw: &[u8],
    recipients: &[String],
    crypto: &ComposeCrypto,
    keyring: &Keyring,
    gnupg_home: Option<&Path>,
) -> Result<Vec<u8>, String> {
    let plan = crypto.plan;
    if plan.is_plain() {
        return Ok(raw.to_vec());
    }
    let certs = plan
        .encrypt
        .then(|| recipient_certs(keyring, recipients))
        .transpose()?;
    if !plan.sign {
        let certs = certs.expect("encrypt set when sign is not");
        return mime::encrypt_message(raw, &certs)
            .map_err(|error| error.to_string());
    }
    let agent = GpgAgent::connect(gnupg_home)
        .map_err(|error| error.to_string())?;
    let fingerprint = resolve_signer(&agent, crypto)?;
    let signer = |data: &[u8]| {
        agent
            .sign_detached(&fingerprint, data)
            .map_err(|error| error.to_string())
    };
    let sealed = match certs {
        Some(certs) => mime::encrypt_and_sign(raw, &certs, &signer),
        None => mime::sign_message(raw, &signer),
    };
    sealed.map_err(|error| error.to_string())
}

fn recipient_certs(
    keyring: &Keyring,
    recipients: &[String],
) -> Result<Vec<Cert>, String> {
    let mut certs = Vec::with_capacity(recipients.len());
    let mut missing = Vec::new();
    for address in recipients {
        match keyring.cert_for_address(address) {
            Some(cert) => certs.push(cert.clone()),
            None => missing.push(address.as_str()),
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "no key in the pgp keyring for: {}",
            missing.join(", ")
        ));
    }
    Ok(certs)
}

/// The fingerprint to sign with: the identity's configured
/// pgp_key, or the agent's signing cert whose user ID carries
/// the identity address.
fn resolve_signer(
    agent: &GpgAgent,
    crypto: &ComposeCrypto,
) -> Result<String, String> {
    if let Some(key) = &crypto.key {
        return Ok(key.clone());
    }
    let certs =
        agent.signing_certs().map_err(|error| error.to_string())?;
    certs
        .iter()
        .find(|summary| {
            summary
                .primary_user_id
                .as_deref()
                .is_some_and(|uid| uid_matches(uid, &crypto.address))
        })
        .map(|summary| summary.fingerprint.clone())
        .ok_or_else(|| {
            format!(
                "no signing key for {} known to gpg-agent",
                crypto.address
            )
        })
}

fn uid_matches(uid: &str, address: &str) -> bool {
    let uid = uid.to_ascii_lowercase();
    let address = address.to_ascii_lowercase();
    uid == address || uid.contains(&format!("<{address}>"))
}

/// A message opened for reading: the rendered body and its
/// signature verdict.
pub struct Opened {
    pub body: String,
    pub signature: Signature,
}

/// Renders a stored message for the pager. An encrypted
/// message is decrypted through gpg-agent first (connected
/// only then), the inner part rendered as usual and verified;
/// an agent failure becomes the displayed body, never a crash.
pub fn read_message(
    raw: &[u8],
    keyring: &Keyring,
    gnupg_home: Option<&Path>,
) -> Opened {
    let Some(ciphertext) = mime::encrypted_payload(raw) else {
        return Opened {
            body: antiphon_render::body_text(raw).text,
            signature: antiphon_pgp::verify(raw, keyring),
        };
    };
    let decrypted = GpgAgent::connect(gnupg_home)
        .and_then(|agent| agent.decrypt(&ciphertext));
    let entity = match decrypted {
        Ok(entity) => entity,
        Err(error) => {
            return Opened {
                body: format!("cannot decrypt: {error}"),
                signature: Signature::none(),
            };
        }
    };
    let merged = mime::merge_decrypted(raw, &entity);
    Opened {
        body: antiphon_render::body_text(&merged).text,
        signature: antiphon_pgp::verify(&merged, keyring),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU32, Ordering};

    use antiphon_pgp::{Keyring, SignatureStatus};

    use super::{
        ComposeCrypto, PgpPlan, read_message, seal, uid_matches,
    };

    const TEST_USER_ID: &str =
        "Antiphon Test <antiphon-test@example.com>";
    const TEST_ADDRESS: &str = "antiphon-test@example.com";
    const BODY: &str = "A body line for the pager round trip.";

    const PLAIN: &str = concat!(
        "From: Antiphon Test <antiphon-test@example.com>\r\n",
        "To: Antiphon Test <antiphon-test@example.com>\r\n",
        "Subject: sealed\r\n",
        "MIME-Version: 1.0\r\n",
        "Content-Type: text/plain; charset=\"utf-8\"\r\n",
        "\r\n",
        "A body line for the pager round trip.\r\n",
    );

    fn plan(sign: bool, encrypt: bool) -> ComposeCrypto {
        ComposeCrypto {
            plan: PgpPlan { sign, encrypt },
            key: None,
            address: TEST_ADDRESS.to_string(),
        }
    }

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> TempDir {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
            let name = format!(
                "antiphon-crypto-{}-{nonce}",
                std::process::id()
            );
            let path = std::env::temp_dir().join(name);
            std::fs::create_dir_all(&path).unwrap();
            TempDir { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn labels_read_per_plan() {
        let cases = [
            (false, false, None),
            (true, false, Some("[sign]")),
            (false, true, Some("[encrypt]")),
            (true, true, Some("[sign+encrypt]")),
        ];
        for (sign, encrypt, expected) in cases {
            let plan = PgpPlan { sign, encrypt };
            assert_eq!(plan.label(), expected);
        }
    }

    #[test]
    fn uid_matching_needs_the_bracketed_address() {
        let cases = [
            ("Alba <alba@example.com>", "alba@example.com", true),
            ("Alba <alba@example.com>", "ALBA@example.com", true),
            ("alba@example.com", "alba@example.com", true),
            ("Alba <alba@example.com>", "lba@example.com", false),
            ("Alba <other@example.com>", "alba@example.com", false),
        ];
        for (uid, address, expected) in cases {
            assert_eq!(uid_matches(uid, address), expected, "{uid}");
        }
    }

    #[test]
    fn a_plain_plan_passes_the_message_through() {
        let dir = TempDir::new();
        let keyring = Keyring::from_dir(&dir.path);
        let sealed = seal(
            PLAIN.as_bytes(),
            &[TEST_ADDRESS.to_string()],
            &plan(false, false),
            &keyring,
            Some(Path::new("/nonexistent")),
        )
        .unwrap();
        assert_eq!(sealed, PLAIN.as_bytes());
    }

    #[test]
    fn missing_recipient_keys_abort_naming_the_addresses() {
        let dir = TempDir::new();
        let keyring = Keyring::from_dir(&dir.path);
        let recipients = [
            "alba@example.com".to_string(),
            "mara@example.com".to_string(),
        ];
        let error = seal(
            PLAIN.as_bytes(),
            &recipients,
            &plan(false, true),
            &keyring,
            Some(Path::new("/nonexistent")),
        )
        .unwrap_err();
        assert_eq!(
            error,
            "no key in the pgp keyring for: alba@example.com, \
             mara@example.com"
        );
    }

    #[test]
    fn unencrypted_messages_never_touch_the_agent() {
        let dir = TempDir::new();
        let keyring = Keyring::from_dir(&dir.path);
        let opened = read_message(
            PLAIN.as_bytes(),
            &keyring,
            Some(Path::new("/nonexistent")),
        );
        assert!(opened.body.contains(BODY));
        assert_eq!(opened.signature.status, SignatureStatus::None);
    }

    struct EphemeralHome {
        dir: TempDir,
        fingerprint: String,
    }

    impl EphemeralHome {
        fn new() -> Option<EphemeralHome> {
            if !gpg_usable() {
                eprintln!(
                    "SKIP: no usable gpg CLI; live gpg-agent \
                     test not run"
                );
                return None;
            }
            let dir = TempDir::new();
            restrict_permissions(&dir.path);
            let mut home = EphemeralHome {
                dir,
                fingerprint: String::new(),
            };
            home.gpg(&[
                "--quick-gen-key",
                TEST_USER_ID,
                "ed25519",
                "cert,sign",
                "never",
            ]);
            home.fingerprint = home.primary_fingerprint();
            let fingerprint = home.fingerprint.clone();
            home.gpg(&[
                "--quick-add-key",
                &fingerprint,
                "cv25519",
                "encr",
                "never",
            ]);
            Some(home)
        }

        fn path(&self) -> &Path {
            &self.dir.path
        }

        fn gpg(&self, args: &[&str]) -> Vec<u8> {
            let output = Command::new("gpg")
                .arg("--homedir")
                .arg(self.path())
                .args([
                    "--batch",
                    "--pinentry-mode",
                    "loopback",
                    "--passphrase",
                    "",
                ])
                .args(args)
                .output()
                .expect("running gpg");
            assert!(
                output.status.success(),
                "gpg {args:?} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            output.stdout
        }

        fn primary_fingerprint(&self) -> String {
            let listing = self.gpg(&["--list-keys", "--with-colons"]);
            let listing = String::from_utf8_lossy(&listing);
            listing
                .lines()
                .find(|line| line.starts_with("fpr:"))
                .and_then(|line| line.split(':').nth(9))
                .expect("a fingerprint in the gpg listing")
                .to_string()
        }

        fn keyring(&self) -> (TempDir, Keyring) {
            let exported = self.gpg(&["--export"]);
            let dir = TempDir::new();
            std::fs::write(dir.path.join("test.pgp"), exported)
                .unwrap();
            let keyring = Keyring::from_dir(&dir.path);
            (dir, keyring)
        }
    }

    impl Drop for EphemeralHome {
        fn drop(&mut self) {
            let _ = Command::new("gpgconf")
                .arg("--homedir")
                .arg(&self.dir.path)
                .args(["--kill", "all"])
                .status();
        }
    }

    fn gpg_usable() -> bool {
        let gpg = Command::new("gpg").arg("--version").output();
        let gpgconf = Command::new("gpgconf").arg("--version").output();
        matches!(gpg, Ok(out) if out.status.success())
            && matches!(gpgconf, Ok(out) if out.status.success())
    }

    fn restrict_permissions(path: &Path) {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(
            path,
            std::fs::Permissions::from_mode(0o700),
        )
        .expect("restricting GNUPGHOME permissions");
    }

    fn assert_good_signature(
        signature: &antiphon_pgp::Signature,
        context: &str,
    ) {
        let SignatureStatus::Good { signer, .. } = &signature.status
        else {
            panic!("{context}: expected Good, got other");
        };
        assert_eq!(signer, TEST_USER_ID, "{context}");
    }

    #[test]
    fn signed_composes_verify_good_by_address_or_key() {
        let Some(home) = EphemeralHome::new() else {
            return;
        };
        let (_dir, keyring) = home.keyring();
        let by_address = seal(
            PLAIN.as_bytes(),
            &[],
            &plan(true, false),
            &keyring,
            Some(home.path()),
        )
        .unwrap();
        let opened =
            read_message(&by_address, &keyring, Some(home.path()));
        assert!(opened.body.contains(BODY));
        assert_good_signature(&opened.signature, "by address");

        let mut by_key = plan(true, false);
        by_key.key = Some(home.fingerprint.clone());
        by_key.address = "unrelated@example.com".to_string();
        let sealed = seal(
            PLAIN.as_bytes(),
            &[],
            &by_key,
            &keyring,
            Some(home.path()),
        )
        .unwrap();
        let signature = antiphon_pgp::verify(&sealed, &keyring);
        assert_good_signature(&signature, "by key");
    }

    #[test]
    fn an_unknown_signer_aborts_the_send() {
        let Some(home) = EphemeralHome::new() else {
            return;
        };
        let (_dir, keyring) = home.keyring();
        let mut unknown = plan(true, false);
        unknown.address = "nobody@example.com".to_string();
        let error = seal(
            PLAIN.as_bytes(),
            &[],
            &unknown,
            &keyring,
            Some(home.path()),
        )
        .unwrap_err();
        assert_eq!(
            error,
            "no signing key for nobody@example.com known to \
             gpg-agent"
        );
    }

    #[test]
    fn encrypted_composes_decrypt_in_the_pager() {
        let Some(home) = EphemeralHome::new() else {
            return;
        };
        let (_dir, keyring) = home.keyring();
        let sealed = seal(
            PLAIN.as_bytes(),
            &[TEST_ADDRESS.to_string()],
            &plan(true, true),
            &keyring,
            Some(home.path()),
        )
        .unwrap();
        let text = String::from_utf8_lossy(&sealed);
        assert!(text.contains("multipart/encrypted"), "{text}");
        assert!(!text.contains(BODY), "plaintext leaked: {text}");

        let opened = read_message(&sealed, &keyring, Some(home.path()));
        assert!(opened.body.contains(BODY), "{}", opened.body);
        assert_good_signature(&opened.signature, "sign+encrypt");
    }

    #[test]
    fn a_decrypt_failure_shows_the_error_not_a_crash() {
        let Some(home) = EphemeralHome::new() else {
            return;
        };
        let (_dir, keyring) = home.keyring();
        let sealed = seal(
            PLAIN.as_bytes(),
            &[TEST_ADDRESS.to_string()],
            &plan(false, true),
            &keyring,
            Some(home.path()),
        )
        .unwrap();
        let corrupted = corrupt_armour(&sealed);
        let opened =
            read_message(&corrupted, &keyring, Some(home.path()));
        assert!(
            opened.body.starts_with("cannot decrypt:"),
            "{}",
            opened.body
        );
        assert_eq!(opened.signature.status, SignatureStatus::None);
    }

    /// Reverses one base64 line inside the armoured payload so
    /// the ciphertext no longer parses.
    fn corrupt_armour(sealed: &[u8]) -> Vec<u8> {
        let text = String::from_utf8(sealed.to_vec()).unwrap();
        let target = text
            .lines()
            .skip_while(|line| {
                !line.starts_with("-----BEGIN PGP MESSAGE-----")
            })
            .skip(2)
            .find(|line| line.len() > 40)
            .expect("a base64 payload line")
            .to_string();
        let reversed: String = target.chars().rev().collect();
        text.replacen(&target, &reversed, 1).into_bytes()
    }
}
