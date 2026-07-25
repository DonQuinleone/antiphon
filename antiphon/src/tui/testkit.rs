use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use antiphon_pgp::{Keyring, SignatureStatus};

use super::crypto::{ComposeCrypto, PgpPlan};
use super::identity::ComposeIdentity;

pub(super) fn tester_identity() -> ComposeIdentity {
    ComposeIdentity {
        name: Some("Tester".to_string()),
        address: "tester@example.com".to_string(),
        signature: Some("Kind regards\n".to_string()),
        pgp_sign: false,
        pgp_key: None,
    }
}

pub(super) const TEST_USER_ID: &str =
    "Antiphon Test <antiphon-test@example.com>";
pub(super) const TEST_ADDRESS: &str = "antiphon-test@example.com";
pub(super) const BODY: &str = "A body line for the pager round trip.";

pub(super) const PLAIN: &str = concat!(
    "From: Antiphon Test <antiphon-test@example.com>\r\n",
    "To: Antiphon Test <antiphon-test@example.com>\r\n",
    "Subject: sealed\r\n",
    "MIME-Version: 1.0\r\n",
    "Content-Type: text/plain; charset=\"utf-8\"\r\n",
    "\r\n",
    "A body line for the pager round trip.\r\n",
);

pub(super) fn plan(sign: bool, encrypt: bool) -> ComposeCrypto {
    ComposeCrypto {
        plan: PgpPlan { sign, encrypt },
        key: None,
        address: TEST_ADDRESS.to_string(),
    }
}

pub(super) struct TempDir {
    pub(super) path: PathBuf,
}

impl TempDir {
    pub(super) fn new() -> TempDir {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
        let name =
            format!("antiphon-crypto-{}-{nonce}", std::process::id());
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

pub(super) struct EphemeralHome {
    dir: TempDir,
    pub(super) fingerprint: String,
}

impl EphemeralHome {
    pub(super) fn new() -> Option<EphemeralHome> {
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

    pub(super) fn path(&self) -> &Path {
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

    pub(super) fn keyring(&self) -> (TempDir, Keyring) {
        let exported = self.gpg(&["--export"]);
        let dir = TempDir::new();
        std::fs::write(dir.path.join("test.pgp"), exported).unwrap();
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

pub(super) fn assert_good_signature(
    signature: &antiphon_pgp::Signature,
    context: &str,
) {
    let SignatureStatus::Good { signer, .. } = &signature.status else {
        panic!("{context}: expected Good, got other");
    };
    assert_eq!(signer, TEST_USER_ID, "{context}");
}
