use mail_parser::{
    ContentType, MessageParser, MessagePart, MimeHeaders,
};

use crate::parts::content_type_text;

const IMAGE_TYPE: &str = "image";
const UNNAMED_IMAGE: &str = "image";

/// One image part of a stored message, decoded: the name to
/// show, its Content-ID where it was referenced by `cid:`,
/// whether it arrived inline or as a plain attachment, its
/// declared content type, and the bytes ready to decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageImage {
    pub name: String,
    pub cid: Option<String>,
    pub inline: bool,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

/// Every `image/*` part carried by the message, inline or
/// attached. Render-only: the bytes are copied, never altered,
/// so viewing an image cannot change what a forward sends.
pub fn images(raw: &[u8]) -> Vec<MessageImage> {
    let Some(message) = MessageParser::default().parse(raw) else {
        return Vec::new();
    };
    message
        .attachments()
        .filter(|part| is_image(part))
        .map(to_image)
        .collect()
}

fn is_image(part: &MessagePart<'_>) -> bool {
    part.content_type()
        .is_some_and(|ct| ct.ctype().eq_ignore_ascii_case(IMAGE_TYPE))
}

fn to_image(part: &MessagePart<'_>) -> MessageImage {
    let cid = part.content_id().map(str::to_string);
    let inline = cid.is_some()
        || part
            .content_disposition()
            .is_some_and(ContentType::is_inline);
    MessageImage {
        name: image_name(part, cid.as_deref()),
        cid,
        inline,
        content_type: content_type_text(part),
        bytes: part.contents().to_vec(),
    }
}

fn image_name(part: &MessagePart<'_>, cid: Option<&str>) -> String {
    if let Some(name) = part.attachment_name() {
        return name.to_string();
    }
    if let Some(cid) = cid {
        return cid.to_string();
    }
    UNNAMED_IMAGE.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AttachmentPart, Draft, build_message};

    const PNG_BYTES: &[u8] =
        &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00];
    const PNG_BASE64: &str = "iVBORw0KGgoA";

    fn related_with_inline_png() -> Vec<u8> {
        format!(
            "From: a@example.com\r\n\
             To: b@example.com\r\n\
             Subject: Logo\r\n\
             MIME-Version: 1.0\r\n\
             Content-Type: multipart/related; boundary=\"B\"\r\n\
             \r\n\
             --B\r\n\
             Content-Type: text/html\r\n\
             \r\n\
             <img src=\"cid:logo@x\">\r\n\
             --B\r\n\
             Content-Type: image/png\r\n\
             Content-Transfer-Encoding: base64\r\n\
             Content-ID: <logo@x>\r\n\
             Content-Disposition: inline; filename=\"logo.png\"\r\n\
             \r\n\
             {PNG_BASE64}\r\n\
             --B--\r\n"
        )
        .into_bytes()
    }

    #[test]
    fn an_inline_cid_png_is_enumerated() {
        let found = images(&related_with_inline_png());
        assert_eq!(found.len(), 1);
        let image = &found[0];
        assert_eq!(image.name, "logo.png");
        assert_eq!(image.content_type, "image/png");
        assert!(image.inline, "cid part is inline");
        assert!(
            image
                .cid
                .as_deref()
                .is_some_and(|cid| cid.contains("logo")),
            "{:?}",
            image.cid
        );
        assert_eq!(image.bytes, PNG_BYTES);
    }

    fn draft_with_image_and_text<'a>() -> Draft<'a> {
        Draft {
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
        }
    }

    #[test]
    fn a_plain_attachment_image_is_enumerated_but_not_inline() {
        let raw = build_message(
            &draft_with_image_and_text(),
            "example.com",
            1_753_400_000,
        );
        let found = images(&raw);
        assert_eq!(found.len(), 1, "only the png, not the text part");
        assert_eq!(found[0].name, "mark.png");
        assert_eq!(found[0].content_type, "image/png");
        assert!(!found[0].inline, "attached, not inline");
        assert_eq!(found[0].cid, None);
        assert_eq!(found[0].bytes, PNG_BYTES);
    }

    #[test]
    fn a_plain_message_has_no_images() {
        let raw = b"From: a@example.com\r\n\
            Subject: bare\r\n\r\njust text\r\n";
        assert!(images(raw).is_empty());
        assert!(images(b"").is_empty());
    }
}
