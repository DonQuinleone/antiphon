use std::io::Write;

use mail_parser::{MessageParser, MessagePart};
use sequoia_openpgp::Cert;
use sequoia_openpgp::armor;
use sequoia_openpgp::policy::StandardPolicy;
use sequoia_openpgp::serialize::stream::{
    Armorer, Encryptor, LiteralWriter, Message,
};

use crate::mime::{
    PgpMimeError, SignFn, armoured_text, boundary_for, canonical,
    header_block, sign_message, split,
};
use crate::verify::subtype_is;

const ENCRYPTED_PROTOCOL: &str = "application/pgp-encrypted";
const ENCRYPTED_VERSION: &str = "Version: 1";

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
    use super::{
        encrypt_and_sign, encrypt_message, encrypted_payload,
        merge_decrypted,
    };
    use crate::mime::PgpMimeError;
    use crate::status::SignatureStatus;
    use crate::testkit::{
        ALICE, BOB, BODY, PLAIN, assert_outer_headers, cert,
        decrypt_with, keyring_with, signer_for, signing_only_cert,
    };
    use crate::verify;

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
