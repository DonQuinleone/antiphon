use std::path::Path;

use antiphon_pgp::{Keyring, Signature, mime};
use antiphon_pgp_agent::GpgAgent;
use antiphon_render::RenderedBody;

/// A message opened for reading: the body text, the same
/// body with its link spans, the signature verdict, and any
/// calendar invite block.
pub struct Opened {
    pub body: String,
    pub rendered: RenderedBody,
    pub signature: Signature,
    pub invite: Vec<String>,
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
    read_message_preferring(
        raw,
        keyring,
        gnupg_home,
        antiphon_render::BodyPreference::Plain,
    )
}

pub fn read_message_preferring(
    raw: &[u8],
    keyring: &Keyring,
    gnupg_home: Option<&Path>,
    preference: antiphon_render::BodyPreference,
) -> Opened {
    let Some(ciphertext) = mime::encrypted_payload(raw) else {
        return Opened {
            body: antiphon_render::body_text_preferring(
                raw, preference,
            )
            .text,
            rendered: antiphon_render::rendered_body_preferring(
                raw, preference,
            ),
            signature: antiphon_pgp::verify(raw, keyring),
            invite: antiphon_render::invite_lines(raw),
        };
    };
    let decrypted = GpgAgent::connect(gnupg_home)
        .and_then(|agent| agent.decrypt(&ciphertext));
    let entity = match decrypted {
        Ok(entity) => entity,
        Err(error) => {
            let body = format!("cannot decrypt: {error}");
            return Opened {
                rendered: antiphon_render::scan_text(&body),
                body,
                signature: Signature::none(),
                invite: Vec::new(),
            };
        }
    };
    let merged = mime::merge_decrypted(raw, &entity);
    Opened {
        body: antiphon_render::body_text_preferring(
            &merged, preference,
        )
        .text,
        rendered: antiphon_render::rendered_body_preferring(
            &merged, preference,
        ),
        signature: antiphon_pgp::verify(&merged, keyring),
        invite: antiphon_render::invite_lines(&merged),
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
        assert!(opened.invite.is_empty());
    }

    #[test]
    fn an_html_only_message_renders_with_its_links() {
        let raw = concat!(
            "From: alba@example.com\r\n",
            "Subject: rich\r\n",
            "MIME-Version: 1.0\r\n",
            "Content-Type: text/html; charset=utf-8\r\n",
            "\r\n",
            "<p>See <a href=\"https://example.com/x\">",
            "the docs</a></p>\r\n",
        );
        let dir = TempDir::new();
        let keyring = Keyring::from_dir(&dir.path);
        let opened = read_message(
            raw.as_bytes(),
            &keyring,
            Some(Path::new("/nonexistent")),
        );
        assert_eq!(opened.body, "See the docs[1]");
        let links: Vec<(&str, &str)> = opened
            .rendered
            .links
            .iter()
            .map(|link| (link.url.as_str(), link.label.as_str()))
            .collect();
        assert_eq!(links, [("https://example.com/x", "the docs")]);
    }

    #[test]
    fn a_calendar_part_yields_an_invite_block() {
        let raw = concat!(
            "From: alba@example.com\r\n",
            "Subject: invite\r\n",
            "MIME-Version: 1.0\r\n",
            "Content-Type: text/calendar; method=REQUEST\r\n",
            "\r\n",
            "BEGIN:VCALENDAR\r\n",
            "VERSION:2.0\r\n",
            "PRODID:-//Example//EN\r\n",
            "METHOD:REQUEST\r\n",
            "BEGIN:VEVENT\r\n",
            "UID:2@example.com\r\n",
            "DTSTAMP:20260720T090000Z\r\n",
            "DTSTART:20260805T130000Z\r\n",
            "SUMMARY:Stand-up\r\n",
            "END:VEVENT\r\n",
            "END:VCALENDAR\r\n",
        );
        let dir = TempDir::new();
        let keyring = Keyring::from_dir(&dir.path);
        let opened = read_message(
            raw.as_bytes(),
            &keyring,
            Some(Path::new("/nonexistent")),
        );
        assert_eq!(
            opened.invite.first().map(String::as_str),
            Some("calendar invite: Stand-up")
        );
        assert!(
            opened
                .invite
                .last()
                .is_some_and(|line| line.contains(":accept"))
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
