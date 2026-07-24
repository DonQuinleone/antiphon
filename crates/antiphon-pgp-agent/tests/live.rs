use std::io::Write;
use std::path::Path;
use std::process::Command;

use antiphon_pgp_agent::GpgAgent;
use sequoia_openpgp::parse::Parse;
use sequoia_openpgp::parse::stream::{
    DetachedVerifierBuilder, MessageLayer, MessageStructure,
    VerificationHelper,
};
use sequoia_openpgp::policy::StandardPolicy;
use sequoia_openpgp::serialize::stream::{
    Encryptor, LiteralWriter, Message,
};
use sequoia_openpgp::{Cert, KeyHandle};

const TEST_USER_ID: &str = "Antiphon Test <antiphon-test@example.com>";
const PLAINTEXT: &[u8] = b"O sing unto the Lord a new song.\n";

struct EphemeralHome {
    dir: tempfile::TempDir,
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
        let dir = tempfile::tempdir()
            .expect("creating an ephemeral GNUPGHOME");
        restrict_permissions(dir.path());
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
        self.dir.path()
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
        let fingerprint = listing
            .lines()
            .find(|line| line.starts_with("fpr:"))
            .and_then(|line| line.split(':').nth(9))
            .expect("a fingerprint in the gpg listing");
        fingerprint.to_string()
    }

    fn cert(&self) -> Cert {
        let exported = self.gpg(&["--export"]);
        Cert::from_bytes(&exported)
            .expect("parsing the exported certificate")
    }
}

impl Drop for EphemeralHome {
    fn drop(&mut self) {
        let _ = Command::new("gpgconf")
            .arg("--homedir")
            .arg(self.dir.path())
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

struct AcceptCert {
    cert: Cert,
}

impl VerificationHelper for AcceptCert {
    fn get_certs(
        &mut self,
        _ids: &[KeyHandle],
    ) -> sequoia_openpgp::Result<Vec<Cert>> {
        Ok(vec![self.cert.clone()])
    }

    fn check(
        &mut self,
        structure: MessageStructure,
    ) -> sequoia_openpgp::Result<()> {
        let good = structure.into_iter().any(|layer| {
            matches!(
                layer,
                MessageLayer::SignatureGroup { ref results }
                    if results.iter().any(Result::is_ok)
            )
        });
        if good {
            return Ok(());
        }
        Err(sequoia_openpgp::Error::InvalidOperation(
            "no good signature".into(),
        )
        .into())
    }
}

#[test]
fn signing_certs_lists_the_ephemeral_key() {
    let Some(home) = EphemeralHome::new() else {
        return;
    };
    let agent = GpgAgent::connect(Some(home.path()))
        .expect("connecting to the ephemeral agent");

    let summaries =
        agent.signing_certs().expect("listing signing certs");

    assert_eq!(summaries.len(), 1);
    assert!(
        summaries[0]
            .fingerprint
            .eq_ignore_ascii_case(&home.fingerprint)
    );
    assert_eq!(
        summaries[0].primary_user_id.as_deref(),
        Some(TEST_USER_ID)
    );
}

#[test]
fn detached_signature_verifies_against_the_cert() {
    let Some(home) = EphemeralHome::new() else {
        return;
    };
    let agent = GpgAgent::connect(Some(home.path()))
        .expect("connecting to the ephemeral agent");

    let signature = agent
        .sign_detached(&home.fingerprint, PLAINTEXT)
        .expect("signing via the agent");

    let armoured = String::from_utf8_lossy(&signature);
    assert!(
        armoured.starts_with("-----BEGIN PGP SIGNATURE-----"),
        "not an armoured signature: {armoured}"
    );

    let policy = StandardPolicy::new();
    let helper = AcceptCert { cert: home.cert() };
    DetachedVerifierBuilder::from_bytes(&signature)
        .expect("reading the detached signature")
        .with_policy(&policy, None, helper)
        .expect("preparing verification")
        .verify_bytes(PLAINTEXT)
        .expect("verifying the detached signature");
}

#[test]
fn decrypt_round_trips_a_message_to_the_cert() {
    let Some(home) = EphemeralHome::new() else {
        return;
    };
    let agent = GpgAgent::connect(Some(home.path()))
        .expect("connecting to the ephemeral agent");

    let cert = home.cert();
    let policy = StandardPolicy::new();
    let valid = cert
        .with_policy(&policy, None)
        .expect("validating the test certificate");
    let recipients = valid
        .keys()
        .alive()
        .revoked(false)
        .supported()
        .for_transport_encryption();

    let mut ciphertext = Vec::new();
    let message = Message::new(&mut ciphertext);
    let message = Encryptor::for_recipients(message, recipients)
        .build()
        .expect("building the encryptor");
    let mut writer = LiteralWriter::new(message)
        .build()
        .expect("building the literal writer");
    writer.write_all(PLAINTEXT).expect("writing plaintext");
    writer.finalize().expect("finalising the message");

    let decrypted = agent
        .decrypt(&ciphertext)
        .expect("decrypting via the agent");
    assert_eq!(decrypted, PLAINTEXT);
}
