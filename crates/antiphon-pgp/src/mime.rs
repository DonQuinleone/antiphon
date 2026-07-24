use std::fmt;
use std::hash::{BuildHasher, Hasher, RandomState};
use std::io::Write;

use mail_parser::{MessageParser, MessagePart};
use sequoia_openpgp::policy::StandardPolicy;
use sequoia_openpgp::serialize::stream::{
    Armorer, Encryptor, LiteralWriter, Message,
};
use sequoia_openpgp::{Cert, armor};

use crate::verify::subtype_is;

/// Produces a detached armoured signature over the given bytes;
/// the indirection keeps this crate free of any gpg-agent
/// dependency.
pub type SignFn<'a> = dyn Fn(&[u8]) -> Result<Vec<u8>, String> + 'a;

const MICALG: &str = "pgp-sha512";
const SIGNATURE_PROTOCOL: &str = "application/pgp-signature";
const ENCRYPTED_PROTOCOL: &str = "application/pgp-encrypted";
const ENCRYPTED_VERSION: &str = "Version: 1";
const BOUNDARY_PREFIX: &str = "=-antiphon-";
const MIME_VERSION: &str = "MIME-Version: 1.0";
const CONTENT_HEADER_PREFIX: &str = "content-";
const MIME_VERSION_HEADER: &str = "mime-version:";

#[derive(Debug)]
pub enum PgpMimeError {
    Malformed(String),
    Sign(String),
    Encrypt(String),
    NoEncryptionKey(String),
}

impl fmt::Display for PgpMimeError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PgpMimeError::Malformed(cause) => {
                write!(out, "malformed message: {cause}")
            }
            PgpMimeError::Sign(cause) => {
                write!(out, "signing failed: {cause}")
            }
            PgpMimeError::Encrypt(cause) => {
                write!(out, "encryption failed: {cause}")
            }
            PgpMimeError::NoEncryptionKey(who) => {
                write!(
                    out,
                    "certificate for {who} has no usable \
                     encryption key"
                )
            }
        }
    }
}

impl std::error::Error for PgpMimeError {}

/// Wraps a plain RFC 5322 message into RFC 3156
/// multipart/signed, calling `sign` for the detached signature
/// over the canonicalised (CRLF) signed part. Non-MIME headers
/// are preserved on the outer message.
pub fn sign_message(
    plain: &[u8],
    sign: &SignFn,
) -> Result<Vec<u8>, PgpMimeError> {
    let entity = split(plain)?;
    let signature =
        sign(entity.part.as_bytes()).map_err(PgpMimeError::Sign)?;
    let signature = armoured_text(&signature).map_err(|cause| {
        PgpMimeError::Sign(format!("bad signature armour: {cause}"))
    })?;
    let boundary = boundary_for(&entity.part);
    let mut out = header_block(&entity.outer);
    out.push_str(&format!(
        "Content-Type: multipart/signed; \
         boundary=\"{boundary}\";\r\n\
         \tprotocol=\"{SIGNATURE_PROTOCOL}\"; \
         micalg=\"{MICALG}\"\r\n\r\n"
    ));
    out.push_str(&format!(
        "--{boundary}\r\n{}\r\n\
         --{boundary}\r\n\
         Content-Type: application/pgp-signature; \
         name=\"signature.asc\"\r\n\r\n\
         {signature}\r\n\
         --{boundary}--\r\n",
        entity.part,
    ));
    Ok(out.into_bytes())
}

/// Wraps a plain RFC 5322 message into RFC 3156
/// multipart/encrypted, encrypting the body part to every
/// recipient certificate.
pub fn encrypt_message(
    plain: &[u8],
    recipients: &[Cert],
) -> Result<Vec<u8>, PgpMimeError> {
    let entity = split(plain)?;
    assemble_encrypted(&entity.outer, &entity.part, recipients)
}

/// Signs the message into multipart/signed, then encrypts that
/// whole signed body to the recipients: the RFC 3156 section
/// 6.2 combined format, so tampering is detectable only after
/// decryption by the intended reader.
pub fn encrypt_and_sign(
    plain: &[u8],
    recipients: &[Cert],
    sign: &SignFn,
) -> Result<Vec<u8>, PgpMimeError> {
    let signed = sign_message(plain, sign)?;
    let entity = split(&signed)?;
    assemble_encrypted(&entity.outer, &entity.part, recipients)
}

/// The armoured ciphertext of a multipart/encrypted message,
/// or `None` when the message is not encrypted.
pub fn encrypted_payload(raw: &[u8]) -> Option<Vec<u8>> {
    let message = MessageParser::default().parse(raw)?;
    let encrypted = message
        .parts
        .iter()
        .find(|part| is_multipart_encrypted(part))?;
    let children = encrypted.sub_parts()?;
    children
        .iter()
        .map(|id| &message.parts[*id as usize])
        .find(|part| subtype_is(part, "application", "octet-stream"))
        .map(|part| part.contents().to_vec())
}

/// Recombines the outer headers of an encrypted message with
/// the decrypted MIME entity, yielding a complete message the
/// renderer and verifier can treat like any other.
pub fn merge_decrypted(raw: &[u8], entity: &[u8]) -> Vec<u8> {
    let entity = canonical(&String::from_utf8_lossy(entity));
    let Ok(outer) = split(raw) else {
        return entity.into_bytes();
    };
    let mut out = header_block(&outer.outer);
    out.push_str(&entity);
    out.into_bytes()
}

struct Entity {
    outer: Vec<String>,
    part: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Slot {
    Outer,
    Part,
    Dropped,
}

fn split(plain: &[u8]) -> Result<Entity, PgpMimeError> {
    let text = std::str::from_utf8(plain).map_err(|_| {
        PgpMimeError::Malformed("message is not UTF-8".to_string())
    })?;
    let text = canonical(text);
    let Some((head, body)) = text.split_once("\r\n\r\n") else {
        return Err(PgpMimeError::Malformed(
            "no blank line after the headers".to_string(),
        ));
    };
    let mut outer = Vec::new();
    let mut content = Vec::new();
    let mut slot = Slot::Outer;
    for line in head.split("\r\n") {
        if !is_continuation(line) {
            slot = slot_for(line);
        }
        match slot {
            Slot::Outer => outer.push(line.to_string()),
            Slot::Part => content.push(line.to_string()),
            Slot::Dropped => {}
        }
    }
    Ok(Entity {
        outer,
        part: part_entity(&content, body),
    })
}

fn is_continuation(line: &str) -> bool {
    line.starts_with(' ') || line.starts_with('\t')
}

fn slot_for(line: &str) -> Slot {
    let lowered = line.to_ascii_lowercase();
    if lowered.starts_with(CONTENT_HEADER_PREFIX) {
        return Slot::Part;
    }
    if lowered.starts_with(MIME_VERSION_HEADER) {
        return Slot::Dropped;
    }
    Slot::Outer
}

fn part_entity(headers: &[String], body: &str) -> String {
    if headers.is_empty() {
        return format!("\r\n{body}");
    }
    format!("{}\r\n\r\n{body}", headers.join("\r\n"))
}

fn header_block(outer: &[String]) -> String {
    let mut block = String::new();
    for line in outer {
        block.push_str(line);
        block.push_str("\r\n");
    }
    block.push_str(MIME_VERSION);
    block.push_str("\r\n");
    block
}

fn canonical(text: &str) -> String {
    let lines: Vec<&str> = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    lines.join("\r\n")
}

fn armoured_text(bytes: &[u8]) -> Result<String, String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "not UTF-8".to_string())?;
    Ok(canonical(text.trim_end()))
}

fn boundary_for(content: &str) -> String {
    loop {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write(content.as_bytes());
        let candidate =
            format!("{BOUNDARY_PREFIX}{:016x}", hasher.finish());
        if !content.contains(&candidate) {
            return candidate;
        }
    }
}

fn assemble_encrypted(
    outer: &[String],
    part: &str,
    recipients: &[Cert],
) -> Result<Vec<u8>, PgpMimeError> {
    let ciphertext = encrypt_to(part.as_bytes(), recipients)?;
    let boundary = boundary_for(&ciphertext);
    let mut out = header_block(outer);
    out.push_str(&format!(
        "Content-Type: multipart/encrypted; \
         boundary=\"{boundary}\";\r\n\
         \tprotocol=\"{ENCRYPTED_PROTOCOL}\"\r\n\r\n"
    ));
    out.push_str(&format!(
        "--{boundary}\r\n\
         Content-Type: {ENCRYPTED_PROTOCOL}\r\n\r\n\
         {ENCRYPTED_VERSION}\r\n\
         --{boundary}\r\n\
         Content-Type: application/octet-stream; \
         name=\"encrypted.asc\"\r\n\r\n\
         {ciphertext}\r\n\
         --{boundary}--\r\n"
    ));
    Ok(out.into_bytes())
}

fn encrypt_to(
    data: &[u8],
    recipients: &[Cert],
) -> Result<String, PgpMimeError> {
    if recipients.is_empty() {
        return Err(PgpMimeError::Encrypt(
            "no recipient certificates".to_string(),
        ));
    }
    let policy = StandardPolicy::new();
    let mut valids = Vec::with_capacity(recipients.len());
    for cert in recipients {
        let valid =
            cert.with_policy(&policy, None).map_err(|error| {
                PgpMimeError::Encrypt(format!(
                    "{}: {error:#}",
                    identify(cert)
                ))
            })?;
        valids.push(valid);
    }
    let mut keys = Vec::new();
    for valid in &valids {
        let usable: Vec<_> = valid
            .keys()
            .alive()
            .revoked(false)
            .supported()
            .for_transport_encryption()
            .collect();
        if usable.is_empty() {
            return Err(PgpMimeError::NoEncryptionKey(identify(
                valid.cert(),
            )));
        }
        keys.extend(usable);
    }
    let mut out = Vec::new();
    let message = Message::new(&mut out);
    let message = Armorer::new(message)
        .kind(armor::Kind::Message)
        .build()
        .map_err(encrypt_error)?;
    let message = Encryptor::for_recipients(message, keys)
        .build()
        .map_err(encrypt_error)?;
    let mut writer =
        LiteralWriter::new(message).build().map_err(encrypt_error)?;
    writer
        .write_all(data)
        .map_err(|error| encrypt_error(error.into()))?;
    writer.finalize().map_err(encrypt_error)?;
    armoured_text(&out).map_err(PgpMimeError::Encrypt)
}

fn encrypt_error(error: anyhow::Error) -> PgpMimeError {
    PgpMimeError::Encrypt(format!("{error:#}"))
}

fn identify(cert: &Cert) -> String {
    cert.userids()
        .next()
        .map(|uid| uid.userid().to_string())
        .unwrap_or_else(|| cert.fingerprint().to_string())
}

fn is_multipart_encrypted(part: &MessagePart) -> bool {
    subtype_is(part, "multipart", "encrypted")
}

#[cfg(test)]
mod tests {
    use sequoia_openpgp::Cert;

    use super::{
        PgpMimeError, encrypt_and_sign, encrypt_message,
        encrypted_payload, merge_decrypted, sign_message,
    };
    use crate::status::SignatureStatus;
    use crate::testkit::{
        cert, decrypt_with, detached_signature, keyring_with,
        signing_only_cert,
    };
    use crate::verify;

    const ALICE: &str = "Alice <alice@example.com>";
    const BOB: &str = "Bob <bob@example.com>";
    const BODY: &str = "A body line for the round trip.";

    const PLAIN: &str = concat!(
        "From: Alice <alice@example.com>\r\n",
        "To: Bob <bob@example.com>\r\n",
        "Subject: sealed\r\n",
        "Message-ID: <1.antiphon@example.com>\r\n",
        "MIME-Version: 1.0\r\n",
        "Content-Type: text/plain; charset=\"utf-8\"; ",
        "format=flowed\r\n",
        "Content-Transfer-Encoding: 8bit\r\n",
        "\r\n",
        "A body line for the round trip.\r\n",
    );

    fn signer_for(
        cert: &Cert,
    ) -> impl Fn(&[u8]) -> Result<Vec<u8>, String> {
        let cert = cert.clone();
        move |data: &[u8]| Ok(detached_signature(&cert, data))
    }

    fn assert_outer_headers(text: &str) {
        assert!(text.starts_with("From: Alice <alice@example.com>"));
        assert!(text.contains("Subject: sealed"));
        assert!(text.contains("Message-ID: <1.antiphon@"));
        assert!(text.contains("MIME-Version: 1.0"));
    }

    #[test]
    fn signed_message_verifies_good_and_keeps_headers() {
        let alice = cert(ALICE);
        let (_dir, keyring) = keyring_with(&alice);
        let signed =
            sign_message(PLAIN.as_bytes(), &signer_for(&alice))
                .unwrap();
        let text = String::from_utf8(signed.clone()).unwrap();
        assert_outer_headers(&text);
        assert!(text.contains("multipart/signed"));
        assert!(text.contains("micalg=\"pgp-sha512\""));
        assert!(
            text.contains("protocol=\"application/pgp-signature\"")
        );
        assert!(text.contains(BODY));
        let signature = verify(&signed, &keyring);
        assert!(matches!(
            signature.status,
            SignatureStatus::Good { ref signer, .. }
                if signer == ALICE
        ));
    }

    #[test]
    fn tampering_flips_a_good_signature_to_bad() {
        let alice = cert(ALICE);
        let (_dir, keyring) = keyring_with(&alice);
        let signed =
            sign_message(PLAIN.as_bytes(), &signer_for(&alice))
                .unwrap();
        let tampered = String::from_utf8(signed)
            .unwrap()
            .replace("round trip", "forged trip")
            .into_bytes();
        let signature = verify(&tampered, &keyring);
        assert!(matches!(
            signature.status,
            SignatureStatus::Bad { .. }
        ));
    }

    #[test]
    fn signing_failures_surface_verbatim() {
        let failing = |_: &[u8]| Err("pinentry dismissed".to_string());
        let error =
            sign_message(PLAIN.as_bytes(), &failing).unwrap_err();
        assert_eq!(
            error.to_string(),
            "signing failed: pinentry dismissed"
        );
    }

    #[test]
    fn encrypted_message_decrypts_back_to_the_body() {
        let bob = cert(BOB);
        let sealed = encrypt_message(
            PLAIN.as_bytes(),
            std::slice::from_ref(&bob),
        )
        .unwrap();
        let text = String::from_utf8(sealed.clone()).unwrap();
        assert_outer_headers(&text);
        assert!(text.contains("multipart/encrypted"));
        assert!(
            text.contains("protocol=\"application/pgp-encrypted\"")
        );
        assert!(!text.contains(BODY), "plaintext leaked: {text}");
        let payload = encrypted_payload(&sealed).unwrap();
        let entity = decrypt_with(&bob, &payload);
        let entity = String::from_utf8(entity).unwrap();
        assert!(entity.starts_with("Content-Type: text/plain"));
        assert!(entity.contains(BODY));
    }

    #[test]
    fn encrypt_and_sign_decrypts_then_verifies_good() {
        let alice = cert(ALICE);
        let bob = cert(BOB);
        let (_dir, keyring) = keyring_with(&alice);
        let sealed = encrypt_and_sign(
            PLAIN.as_bytes(),
            std::slice::from_ref(&bob),
            &signer_for(&alice),
        )
        .unwrap();
        let payload = encrypted_payload(&sealed).unwrap();
        let entity = decrypt_with(&bob, &payload);
        assert!(
            String::from_utf8_lossy(&entity)
                .starts_with("Content-Type: multipart/signed")
        );
        let merged = merge_decrypted(&sealed, &entity);
        let text = String::from_utf8_lossy(&merged);
        assert_outer_headers(&text);
        assert!(text.contains(BODY));
        let signature = verify(&merged, &keyring);
        assert!(matches!(
            signature.status,
            SignatureStatus::Good { ref signer, .. }
                if signer == ALICE
        ));
    }

    #[test]
    fn tampering_inside_the_encryption_flips_to_bad() {
        let alice = cert(ALICE);
        let bob = cert(BOB);
        let (_dir, keyring) = keyring_with(&alice);
        let sealed = encrypt_and_sign(
            PLAIN.as_bytes(),
            std::slice::from_ref(&bob),
            &signer_for(&alice),
        )
        .unwrap();
        let entity =
            decrypt_with(&bob, &encrypted_payload(&sealed).unwrap());
        let tampered = String::from_utf8(entity)
            .unwrap()
            .replace("round trip", "forged trip")
            .into_bytes();
        let merged = merge_decrypted(&sealed, &tampered);
        let signature = verify(&merged, &keyring);
        assert!(matches!(
            signature.status,
            SignatureStatus::Bad { .. }
        ));
    }

    #[test]
    fn a_recipient_without_an_encryption_key_is_an_error() {
        let signing_only = signing_only_cert(BOB);
        let error = encrypt_message(PLAIN.as_bytes(), &[signing_only])
            .unwrap_err();
        let PgpMimeError::NoEncryptionKey(who) = error else {
            panic!("expected NoEncryptionKey");
        };
        assert_eq!(who, BOB);
    }

    #[test]
    fn plain_messages_carry_no_encrypted_payload() {
        assert!(encrypted_payload(PLAIN.as_bytes()).is_none());
    }
}
