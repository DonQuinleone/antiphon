use std::path::Path;

use antiphon_pgp::{Cert, Keyring, mime};
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use antiphon_pgp::Keyring;

    use super::super::decrypt::read_message;
    use super::super::testkit::{
        BODY, EphemeralHome, PLAIN, TEST_ADDRESS, TempDir,
        assert_good_signature, plan,
    };
    use super::{PgpPlan, seal, uid_matches};

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

    /// A message with one binary attachment, assembled by the
    /// one real assembly path, sent from the test key's
    /// address.
    fn assembled_with_attachment() -> (Vec<u8>, Vec<u8>) {
        use super::super::attach::Attachment;
        use super::super::compose::{Outgoing, assemble};

        let bytes =
            vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00];
        let outgoing = Outgoing {
            from_name: Some("Antiphon Test".to_string()),
            from: TEST_ADDRESS.to_string(),
            to: vec![TEST_ADDRESS.to_string()],
            subject: "sealed with file".to_string(),
            body: BODY.to_string(),
            ..Outgoing::default()
        };
        let attachment = Attachment {
            path: "mark.png".into(),
            filename: "mark.png".to_string(),
            content_type: "image/png",
            bytes: bytes.clone(),
        };
        (assemble(&outgoing, &[attachment], 1_753_380_000), bytes)
    }

    /// The pgp layers add parts of their own (the detached
    /// signature travels as an attachment too), so the file
    /// is found by name rather than by position.
    fn assert_attachment_intact(raw: &[u8], bytes: &[u8]) {
        use mail_parser::{MessageParser, MimeHeaders};

        let message = MessageParser::default().parse(raw).unwrap();
        let part = message
            .attachments()
            .find(|part| part.attachment_name() == Some("mark.png"))
            .expect("the attached file inside the sealed message");
        assert_eq!(part.contents(), bytes);
    }

    #[test]
    fn signing_wraps_the_full_multipart_per_rfc_3156() {
        let Some(home) = EphemeralHome::new() else {
            return;
        };
        let (_dir, keyring) = home.keyring();
        let (raw, bytes) = assembled_with_attachment();
        let sealed = seal(
            &raw,
            &[],
            &plan(true, false),
            &keyring,
            Some(home.path()),
        )
        .unwrap();
        let text = String::from_utf8_lossy(&sealed);
        assert!(text.contains("multipart/signed"), "{text}");
        assert!(text.contains("multipart/mixed"), "{text}");
        let signature = antiphon_pgp::verify(&sealed, &keyring);
        assert_good_signature(&signature, "signed multipart");
        assert_attachment_intact(&sealed, &bytes);
    }

    #[test]
    fn encryption_hides_the_attachment_until_decrypted() {
        use antiphon_pgp::mime;
        use antiphon_pgp_agent::GpgAgent;

        let Some(home) = EphemeralHome::new() else {
            return;
        };
        let (_dir, keyring) = home.keyring();
        let (raw, bytes) = assembled_with_attachment();
        let sealed = seal(
            &raw,
            &[TEST_ADDRESS.to_string()],
            &plan(true, true),
            &keyring,
            Some(home.path()),
        )
        .unwrap();
        let text = String::from_utf8_lossy(&sealed);
        assert!(text.contains("multipart/encrypted"), "{text}");
        assert!(!text.contains("mark.png"), "name leaked: {text}");

        let payload = mime::encrypted_payload(&sealed).unwrap();
        let agent = GpgAgent::connect(Some(home.path())).unwrap();
        let entity = agent.decrypt(&payload).unwrap();
        let merged = mime::merge_decrypted(&sealed, &entity);
        assert_attachment_intact(&merged, &bytes);
        let signature = antiphon_pgp::verify(&merged, &keyring);
        assert_good_signature(&signature, "decrypted multipart");
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
}
