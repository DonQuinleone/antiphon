use std::path::Path;

use antiphon_pgp::{Keyring, Signature, mime};
use antiphon_pgp_agent::GpgAgent;

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
    use std::path::Path;

    use antiphon_pgp::{Keyring, SignatureStatus};

    use super::super::crypto::seal;
    use super::super::testkit::{
        BODY, EphemeralHome, PLAIN, TEST_ADDRESS, TempDir,
        assert_good_signature, plan,
    };
    use super::read_message;

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
