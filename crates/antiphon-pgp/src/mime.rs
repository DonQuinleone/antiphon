use std::fmt;
use std::hash::{BuildHasher, Hasher, RandomState};

/// Produces a detached armoured signature over the given bytes;
/// the indirection keeps this crate free of any gpg-agent
/// dependency.
pub type SignFn<'a> = dyn Fn(&[u8]) -> Result<Vec<u8>, String> + 'a;

pub use crate::encrypt::{
    encrypt_and_sign, encrypt_message, encrypted_payload,
    merge_decrypted,
};

const MICALG: &str = "pgp-sha512";
const SIGNATURE_PROTOCOL: &str = "application/pgp-signature";
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

pub(crate) struct Entity {
    pub(crate) outer: Vec<String>,
    pub(crate) part: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Slot {
    Outer,
    Part,
    Dropped,
}

pub(crate) fn split(plain: &[u8]) -> Result<Entity, PgpMimeError> {
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

pub(crate) fn header_block(outer: &[String]) -> String {
    let mut block = String::new();
    for line in outer {
        block.push_str(line);
        block.push_str("\r\n");
    }
    block.push_str(MIME_VERSION);
    block.push_str("\r\n");
    block
}

pub(crate) fn canonical(text: &str) -> String {
    let lines: Vec<&str> = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    lines.join("\r\n")
}

pub(crate) fn armoured_text(bytes: &[u8]) -> Result<String, String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "not UTF-8".to_string())?;
    Ok(canonical(text.trim_end()))
}

pub(crate) fn boundary_for(content: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::sign_message;
    use crate::status::SignatureStatus;
    use crate::testkit::{
        ALICE, BODY, PLAIN, assert_outer_headers, cert, keyring_with,
        signer_for,
    };
    use crate::verify;

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
}
