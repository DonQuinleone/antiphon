use std::cell::RefCell;
use std::rc::Rc;

use mail_parser::{Message, MessageParser, MimeHeaders};
use sequoia_openpgp::packet::Signature as SigPacket;
use sequoia_openpgp::parse::Parse;
use sequoia_openpgp::parse::stream::{
    DetachedVerifierBuilder, GoodChecksum, MessageLayer,
    MessageStructure, VerificationError, VerificationHelper,
    VerificationResult, VerifierBuilder,
};
use sequoia_openpgp::policy::StandardPolicy;
use sequoia_openpgp::{Cert, Fingerprint, KeyHandle, KeyID};

use crate::keyring::Keyring;
use crate::status::{Signature, SignatureStatus};

const CLEARTEXT_BEGIN: &[u8] = b"-----BEGIN PGP SIGNED MESSAGE-----";
const CLEARTEXT_END: &[u8] = b"-----END PGP SIGNATURE-----";
const PGP_MIME_TYPE: &str = "application";
const PGP_MIME_SUBTYPE: &str = "pgp-signature";
const SIGNED_TYPE: &str = "multipart";
const SIGNED_SUBTYPE: &str = "signed";

enum Signed {
    Detached { data: Vec<u8>, signature: Vec<u8> },
    Cleartext(Vec<u8>),
}

/// Verifies whatever signature the message carries against the
/// trusted keyring, describing the outcome for display. A message
/// with no signature yields `SignatureStatus::None`; the verifier
/// never claims Good for a key it cannot check.
pub fn verify(raw_message: &[u8], keyring: &Keyring) -> Signature {
    match find_signed(raw_message) {
        Some(Signed::Detached { data, signature }) => {
            verify_detached(&data, &signature, keyring)
        }
        Some(Signed::Cleartext(blob)) => {
            verify_cleartext(&blob, keyring)
        }
        None => Signature::none(),
    }
}

fn find_signed(raw: &[u8]) -> Option<Signed> {
    if let Some(message) = MessageParser::default().parse(raw)
        && let Some(signed) = detached(raw, &message)
    {
        return Some(signed);
    }
    find_cleartext(raw).map(Signed::Cleartext)
}

fn detached(raw: &[u8], message: &Message) -> Option<Signed> {
    let part = message.parts.iter().find(|part| is_signed(part))?;
    let children = part.sub_parts()?;
    let signature_id = children
        .iter()
        .copied()
        .find(|id| is_pgp_signature(&message.parts[*id as usize]))?;
    let signed_id =
        children.iter().copied().find(|id| *id != signature_id)?;
    let signed = &message.parts[signed_id as usize];
    let start = signed.offset_header as usize;
    let end = signed.offset_end as usize;
    let data = raw.get(start..end)?.to_vec();
    let signature =
        message.parts[signature_id as usize].contents().to_vec();
    Some(Signed::Detached { data, signature })
}

fn is_signed(part: &mail_parser::MessagePart) -> bool {
    subtype_is(part, SIGNED_TYPE, SIGNED_SUBTYPE)
}

fn is_pgp_signature(part: &mail_parser::MessagePart) -> bool {
    subtype_is(part, PGP_MIME_TYPE, PGP_MIME_SUBTYPE)
}

fn subtype_is(
    part: &mail_parser::MessagePart,
    ctype: &str,
    subtype: &str,
) -> bool {
    let Some(content_type) = part.content_type() else {
        return false;
    };
    let matches_subtype = content_type
        .subtype()
        .is_some_and(|found| found.eq_ignore_ascii_case(subtype));
    content_type.ctype().eq_ignore_ascii_case(ctype) && matches_subtype
}

fn find_cleartext(raw: &[u8]) -> Option<Vec<u8>> {
    let start = find_subslice(raw, CLEARTEXT_BEGIN)?;
    let tail = &raw[start..];
    let end_rel = find_subslice(tail, CLEARTEXT_END)?;
    let end = start + end_rel + CLEARTEXT_END.len();
    Some(raw[start..end].to_vec())
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn verify_detached(
    data: &[u8],
    signature: &[u8],
    keyring: &Keyring,
) -> Signature {
    let out = fresh_slot();
    let helper = collector(keyring, &out);
    let policy = StandardPolicy::new();
    let _ = DetachedVerifierBuilder::from_bytes(signature)
        .and_then(|builder| builder.with_policy(&policy, None, helper))
        .and_then(|mut verifier| verifier.verify_bytes(data));
    read_slot(&out)
}

fn verify_cleartext(blob: &[u8], keyring: &Keyring) -> Signature {
    let out = fresh_slot();
    let helper = collector(keyring, &out);
    let policy = StandardPolicy::new();
    let built = VerifierBuilder::from_bytes(blob)
        .and_then(|builder| builder.with_policy(&policy, None, helper));
    if let Ok(mut verifier) = built {
        let mut sink = Vec::new();
        let _ = std::io::copy(&mut verifier, &mut sink);
    }
    read_slot(&out)
}

type Slot = Rc<RefCell<SignatureStatus>>;

fn fresh_slot() -> Slot {
    Rc::new(RefCell::new(SignatureStatus::None))
}

fn read_slot(slot: &Slot) -> Signature {
    Signature::from_status(slot.borrow().clone())
}

fn collector<'a>(keyring: &'a Keyring, slot: &Slot) -> Collector<'a> {
    Collector {
        certs: keyring.certs(),
        out: Rc::clone(slot),
    }
}

struct Collector<'a> {
    certs: &'a [Cert],
    out: Slot,
}

impl VerificationHelper for Collector<'_> {
    fn get_certs(
        &mut self,
        _ids: &[KeyHandle],
    ) -> sequoia_openpgp::Result<Vec<Cert>> {
        Ok(self.certs.to_vec())
    }

    fn check(
        &mut self,
        structure: MessageStructure,
    ) -> sequoia_openpgp::Result<()> {
        *self.out.borrow_mut() = summarise(structure, self.certs);
        Ok(())
    }
}

fn summarise(
    structure: MessageStructure,
    certs: &[Cert],
) -> SignatureStatus {
    let mut best = SignatureStatus::None;
    for layer in structure {
        let MessageLayer::SignatureGroup { results } = layer else {
            continue;
        };
        for result in results {
            let candidate = classify(result, certs);
            if rank(&candidate) > rank(&best) {
                best = candidate;
            }
        }
    }
    best
}

fn rank(status: &SignatureStatus) -> u8 {
    match status {
        SignatureStatus::None => 0,
        SignatureStatus::Unknown { .. } => 1,
        SignatureStatus::Bad { .. } => 2,
        SignatureStatus::Good { .. } => 3,
    }
}

fn classify(
    result: VerificationResult,
    certs: &[Cert],
) -> SignatureStatus {
    match result {
        Ok(good) => good_status(&good, certs),
        Err(VerificationError::MissingKey { sig }) => {
            SignatureStatus::Unknown {
                key_id: issuer_key_id(sig),
            }
        }
        Err(error) => SignatureStatus::Bad {
            key_id: error_key_id(&error),
        },
    }
}

fn good_status(good: &GoodChecksum, certs: &[Cert]) -> SignatureStatus {
    let key_id = good.ka.key().keyid().to_hex();
    let fingerprint = good.ka.key().fingerprint();
    let signer = cert_for_key(certs, &fingerprint)
        .and_then(primary_uid)
        .unwrap_or_else(|| format!("key 0x{key_id}"));
    SignatureStatus::Good { signer, key_id }
}

fn cert_for_key<'a>(
    certs: &'a [Cert],
    fingerprint: &Fingerprint,
) -> Option<&'a Cert> {
    certs.iter().find(|cert| {
        cert.keys().any(|ka| ka.key().fingerprint() == *fingerprint)
    })
}

fn primary_uid(cert: &Cert) -> Option<String> {
    cert.userids()
        .next()
        .map(|amalgamation| amalgamation.userid().to_string())
}

fn error_key_id(error: &VerificationError) -> String {
    let sig = match error {
        VerificationError::MissingKey { sig } => sig,
        VerificationError::UnboundKey { sig, .. } => sig,
        VerificationError::BadKey { sig, .. } => sig,
        VerificationError::BadSignature { sig, .. } => sig,
        VerificationError::MalformedSignature { sig, .. } => sig,
        _ => return "unknown".to_string(),
    };
    issuer_key_id(sig)
}

fn issuer_key_id(sig: &SigPacket) -> String {
    sig.get_issuers()
        .first()
        .map(|handle| KeyID::from(handle).to_hex())
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    use sequoia_openpgp::Cert;
    use sequoia_openpgp::armor::Kind;
    use sequoia_openpgp::cert::CertBuilder;
    use sequoia_openpgp::crypto::KeyPair;
    use sequoia_openpgp::policy::StandardPolicy;
    use sequoia_openpgp::serialize::SerializeInto;
    use sequoia_openpgp::serialize::stream::{
        Armorer, Message, Signer,
    };

    use super::{MessageParser, detached, verify};
    use crate::keyring::Keyring;
    use crate::status::SignatureStatus;

    const ALICE: &str = "Alice <alice@example.com>";
    const SIGNATURE_SLOT: &str = "REPLACE_ME_SIGNATURE";
    const CLEARTEXT_BODY: &[u8] = b"Inline cleartext body.\n";

    const MIME_TEMPLATE: &str = concat!(
        "From: Alice <alice@example.com>\r\n",
        "To: Bob <bob@example.com>\r\n",
        "Subject: signed\r\n",
        "MIME-Version: 1.0\r\n",
        "Content-Type: multipart/signed; ",
        "boundary=\"sig-boundary\"; ",
        "protocol=\"application/pgp-signature\"; ",
        "micalg=\"pgp-sha512\"\r\n",
        "\r\n",
        "--sig-boundary\r\n",
        "Content-Type: text/plain; charset=utf-8\r\n",
        "\r\n",
        "Hello, PGP/MIME signed world.\r\n",
        "--sig-boundary\r\n",
        "Content-Type: application/pgp-signature; ",
        "name=\"signature.asc\"\r\n",
        "\r\n",
        "REPLACE_ME_SIGNATURE\r\n",
        "--sig-boundary--\r\n",
    );

    const UNSIGNED: &[u8] = concat!(
        "From: Alice <alice@example.com>\r\n",
        "Subject: plain\r\n",
        "Content-Type: text/plain; charset=utf-8\r\n",
        "\r\n",
        "Nothing signed here.\r\n",
    )
    .as_bytes();

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> TempDir {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
            let name =
                format!("antiphon-pgp-{}-{nonce}", std::process::id());
            let path = std::env::temp_dir().join(name);
            std::fs::create_dir_all(&path).unwrap();
            TempDir { path }
        }

        fn write(&self, name: &str, bytes: &[u8]) {
            std::fs::write(self.path.join(name), bytes).unwrap();
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn alice() -> Cert {
        CertBuilder::general_purpose(Some(ALICE))
            .generate()
            .unwrap()
            .0
    }

    fn keyring_with(cert: &Cert) -> (TempDir, Keyring) {
        let dir = TempDir::new();
        dir.write("alice.asc", &cert.armored().to_vec().unwrap());
        let keyring = Keyring::from_dir(&dir.path);
        (dir, keyring)
    }

    fn empty_keyring() -> (TempDir, Keyring) {
        let dir = TempDir::new();
        let keyring = Keyring::from_dir(&dir.path);
        (dir, keyring)
    }

    fn signing_keypair(cert: &Cert) -> KeyPair {
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

    fn detached_signature(cert: &Cert, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let message = Message::new(&mut out);
        let message = Armorer::new(message)
            .kind(Kind::Signature)
            .build()
            .unwrap();
        let mut signer = Signer::new(message, signing_keypair(cert))
            .unwrap()
            .detached()
            .build()
            .unwrap();
        signer.write_all(data).unwrap();
        signer.finalize().unwrap();
        out
    }

    fn cleartext_signature(cert: &Cert, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let message = Message::new(&mut out);
        let mut signer = Signer::new(message, signing_keypair(cert))
            .unwrap()
            .cleartext()
            .build()
            .unwrap();
        signer.write_all(data).unwrap();
        signer.finalize().unwrap();
        out
    }

    fn signed_part_bytes(raw: &[u8]) -> Vec<u8> {
        let message = MessageParser::default().parse(raw).unwrap();
        match detached(raw, &message).unwrap() {
            super::Signed::Detached { data, .. } => data,
            super::Signed::Cleartext(_) => unreachable!(),
        }
    }

    fn signed_mime(cert: &Cert) -> Vec<u8> {
        let template = MIME_TEMPLATE.as_bytes();
        let data = signed_part_bytes(template);
        let signature = detached_signature(cert, &data);
        let armored = String::from_utf8(signature).unwrap();
        MIME_TEMPLATE
            .replace(SIGNATURE_SLOT, armored.trim_end())
            .into_bytes()
    }

    fn signed_cleartext(cert: &Cert) -> Vec<u8> {
        let blob = cleartext_signature(cert, CLEARTEXT_BODY);
        let mut message = concat!(
            "From: Alice <alice@example.com>\r\n",
            "Subject: clear\r\n",
            "Content-Type: text/plain; charset=utf-8\r\n",
            "\r\n",
        )
        .as_bytes()
        .to_vec();
        message.extend_from_slice(&blob);
        message
    }

    #[test]
    fn good_signature_verifies_against_the_keyring() {
        let cert = alice();
        let (_dir, keyring) = keyring_with(&cert);
        let cases: [(&str, Vec<u8>); 2] = [
            ("pgp/mime", signed_mime(&cert)),
            ("cleartext", signed_cleartext(&cert)),
        ];
        for (name, raw) in cases {
            let signature = verify(&raw, &keyring);
            let SignatureStatus::Good { signer, key_id } =
                signature.status
            else {
                panic!("{name}: expected Good, got other");
            };
            assert_eq!(signer, ALICE, "{name}: signer");
            assert!(!key_id.is_empty(), "{name}: key id");
        }
    }

    #[test]
    fn unknown_when_the_key_is_not_in_the_keyring() {
        let cert = alice();
        let (_dir, keyring) = empty_keyring();
        let raw = signed_mime(&cert);
        let signature = verify(&raw, &keyring);
        assert!(matches!(
            signature.status,
            SignatureStatus::Unknown { .. }
        ));
    }

    #[test]
    fn bad_when_the_signed_body_is_tampered() {
        let cert = alice();
        let (_dir, keyring) = keyring_with(&cert);
        let raw = signed_mime(&cert);
        let tampered = String::from_utf8(raw)
            .unwrap()
            .replace("signed world", "forged world")
            .into_bytes();
        let signature = verify(&tampered, &keyring);
        assert!(matches!(
            signature.status,
            SignatureStatus::Bad { .. }
        ));
    }

    #[test]
    fn none_for_an_unsigned_message() {
        let cert = alice();
        let (_dir, keyring) = keyring_with(&cert);
        let signature = verify(UNSIGNED, &keyring);
        assert_eq!(signature.status, SignatureStatus::None);
    }
}
