/// One attachment ready for assembly: the name shown to the
/// recipient, its declared content type, and the raw bytes
/// (base64-encoded by the builder).
pub struct AttachmentPart<'a> {
    pub filename: &'a str,
    pub content_type: &'a str,
    pub bytes: &'a [u8],
}

pub(crate) const OCTET_STREAM: &str = "application/octet-stream";

const CONTENT_TYPES: &[(&str, &str)] = &[
    ("asc", "text/plain"),
    ("csv", "text/csv"),
    ("gif", "image/gif"),
    ("gz", "application/gzip"),
    ("html", "text/html"),
    ("ics", "text/calendar"),
    ("jpeg", "image/jpeg"),
    ("jpg", "image/jpeg"),
    ("json", "application/json"),
    ("md", "text/plain"),
    ("mp3", "audio/mpeg"),
    ("mp4", "video/mp4"),
    ("ogg", "audio/ogg"),
    ("patch", "text/x-patch"),
    ("pdf", "application/pdf"),
    ("png", "image/png"),
    ("svg", "image/svg+xml"),
    ("tar", "application/x-tar"),
    ("txt", "text/plain"),
    ("wav", "audio/wav"),
    ("webp", "image/webp"),
    ("xml", "application/xml"),
    ("zip", "application/zip"),
];

/// The content type an attachment is declared with, inferred
/// from its extension; anything unknown ships as opaque
/// bytes.
pub fn content_type_for(filename: &str) -> &'static str {
    let Some((_, extension)) = filename.rsplit_once('.') else {
        return OCTET_STREAM;
    };
    let extension = extension.to_ascii_lowercase();
    CONTENT_TYPES
        .iter()
        .find(|(known, _)| *known == extension)
        .map(|(_, content_type)| *content_type)
        .unwrap_or(OCTET_STREAM)
}

#[cfg(test)]
mod tests {
    use mail_parser::{MessageParser, MimeHeaders};

    use super::*;
    use crate::{Draft, build_message};

    const PNG_BYTES: &[u8] =
        &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00];

    fn draft_with_attachments<'a>() -> Draft<'a> {
        Draft {
            from_name: Some("Q"),
            from: "quin@example.com",
            to: vec!["mara@example.com"],
            cc: Vec::new(),
            subject: "Sketches",
            in_reply_to: None,
            references: Vec::new(),
            body: "Two files attached.",
            signature: None,
            attachments: vec![
                AttachmentPart {
                    filename: "mark.png",
                    content_type: content_type_for("mark.png"),
                    bytes: PNG_BYTES,
                },
                AttachmentPart {
                    filename: "notes.txt",
                    content_type: content_type_for("notes.txt"),
                    bytes: b"plain words\n",
                },
            ],
            read_receipt: false,
        }
    }

    #[test]
    fn attachments_round_trip_through_multipart_mixed() {
        let raw = build_message(
            &draft_with_attachments(),
            "example.com",
            1_753_380_000,
            "Antiphon 9.9.9",
        );
        let text = String::from_utf8(raw.clone()).unwrap();
        assert!(text.contains("multipart/mixed"), "{text}");
        assert!(
            text.contains("Content-Transfer-Encoding: base64"),
            "{text}"
        );
        assert!(text.contains("format=flowed"), "{text}");

        let message = MessageParser::default().parse(&raw).unwrap();
        assert_eq!(message.attachment_count(), 2);
        let png = message.attachment(0).unwrap();
        assert_eq!(png.attachment_name(), Some("mark.png"));
        assert_eq!(
            png.content_type().unwrap().ctype(),
            "image",
            "png keeps its declared type"
        );
        assert_eq!(png.contents(), PNG_BYTES);
        let notes = message.attachment(1).unwrap();
        assert_eq!(notes.attachment_name(), Some("notes.txt"));
        assert_eq!(notes.contents(), b"plain words\n");
        let body = crate::body_text(&raw);
        assert!(body.text.contains("Two files attached."));
    }

    #[test]
    fn content_types_follow_the_extension_table() {
        let cases = [
            ("notes.txt", "text/plain"),
            ("scan.PDF", "application/pdf"),
            ("photo.jpeg", "image/jpeg"),
            ("archive.tar", "application/x-tar"),
            ("series.patch", "text/x-patch"),
            ("noextension", "application/octet-stream"),
            ("weird.xyz", "application/octet-stream"),
            ("dotted.name.png", "image/png"),
        ];
        for (filename, expected) in cases {
            assert_eq!(
                content_type_for(filename),
                expected,
                "{filename}"
            );
        }
    }
}
