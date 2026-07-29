use mail_parser::{MessageParser, MessagePart, MimeHeaders};

use crate::attach::OCTET_STREAM;

const UNNAMED: &str = "unnamed";

/// One attachment of a stored message, decoded: the name it
/// was sent under, its declared content type, and the bytes
/// ready to save or view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageAttachment {
    pub filename: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

impl MessageAttachment {
    pub fn size(&self) -> usize {
        self.bytes.len()
    }

    pub fn label(&self) -> String {
        format!(
            "{} ({}, {} bytes)",
            self.filename,
            self.content_type,
            self.size()
        )
    }
}

pub fn attachments(raw: &[u8]) -> Vec<MessageAttachment> {
    let Some(message) = MessageParser::default().parse(raw) else {
        return Vec::new();
    };
    message
        .attachments()
        .map(|part| MessageAttachment {
            filename: part
                .attachment_name()
                .unwrap_or(UNNAMED)
                .to_string(),
            content_type: content_type_text(part),
            bytes: part.contents().to_vec(),
        })
        .collect()
}

pub(crate) fn content_type_text(part: &MessagePart<'_>) -> String {
    let Some(content_type) = part.content_type() else {
        return OCTET_STREAM.to_string();
    };
    match &content_type.c_subtype {
        Some(subtype) => {
            format!("{}/{subtype}", content_type.c_type)
        }
        None => content_type.c_type.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AttachmentPart, Draft, build_message};

    const PNG_BYTES: &[u8] =
        &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00];

    fn raw_with_attachments() -> Vec<u8> {
        let draft = Draft {
            from_name: Some("Q"),
            from: "quin@example.com",
            to: vec!["mara@example.com"],
            cc: Vec::new(),
            subject: "Files",
            in_reply_to: None,
            references: Vec::new(),
            body: "Attached.",
            signature: None,
            attachments: vec![
                AttachmentPart {
                    filename: "mark.png",
                    content_type: "image/png",
                    bytes: PNG_BYTES,
                },
                AttachmentPart {
                    filename: "notes.txt",
                    content_type: "text/plain",
                    bytes: b"plain words\n",
                },
            ],
            read_receipt: false,
        };
        build_message(
            &draft,
            "example.com",
            1_753_400_000,
            "Antiphon 9.9.9",
        )
    }

    #[test]
    fn attachments_decode_round_trip() {
        let found = attachments(&raw_with_attachments());
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].filename, "mark.png");
        assert_eq!(found[0].content_type, "image/png");
        assert_eq!(found[0].bytes, PNG_BYTES);
        assert_eq!(found[0].size(), PNG_BYTES.len());
        assert_eq!(found[1].filename, "notes.txt");
        assert_eq!(found[1].bytes, b"plain words\n");
        assert_eq!(
            found[1].label(),
            "notes.txt (text/plain, 12 bytes)"
        );
    }

    #[test]
    fn a_plain_message_has_no_attachments() {
        let raw = b"From: a@example.com\r\n\
            Subject: bare\r\n\r\njust text\r\n";
        assert!(attachments(raw).is_empty());
        assert!(attachments(b"").is_empty());
    }
}
