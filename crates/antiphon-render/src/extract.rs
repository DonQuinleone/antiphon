use mail_parser::{
    ContentType, HeaderName, HeaderValue, MessageParser, MessagePart,
    MimeHeaders, PartType,
};

use crate::flowed::unflow;
use crate::html::html_body;
use crate::links::{RenderedBody, plain_body};

const HTML_ONLY_NOTICE: &str = "[HTML-only message: no plain-text \
     part; HTML rendering is not yet supported]";

enum RawBody {
    Plain(String),
    Html(String),
    Empty,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyText {
    pub text: String,
    pub kind: BodyKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    Plain,
    HtmlOnly,
    Empty,
}

pub fn body_text(raw: &[u8]) -> BodyText {
    match raw_body(raw) {
        RawBody::Plain(text) => BodyText {
            text,
            kind: BodyKind::Plain,
        },
        RawBody::Html(_) => BodyText {
            text: HTML_ONLY_NOTICE.to_owned(),
            kind: BodyKind::HtmlOnly,
        },
        RawBody::Empty => empty(),
    }
}

pub fn rendered_body(raw: &[u8]) -> RenderedBody {
    rendered_body_preferring(raw, BodyPreference::Plain)
}

/// The linked rendering of whichever part
/// body_text_preferring would pick, so the two stay aligned
/// line for line.
pub fn rendered_body_preferring(
    raw: &[u8],
    preference: BodyPreference,
) -> RenderedBody {
    match raw_body_preferring(raw, preference) {
        RawBody::Plain(text) => plain_body(&text),
        RawBody::Html(html) => html_body(&html),
        RawBody::Empty => RenderedBody::default(),
    }
}

fn raw_body(raw: &[u8]) -> RawBody {
    raw_body_preferring(raw, BodyPreference::Plain)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyPreference {
    Plain,
    Html,
}

/// Whether the message HAS an html part at all, so the UI can
/// offer the toggle only when it means something.
pub fn has_html_part(raw: &[u8]) -> bool {
    MessageParser::default().parse(raw).is_some_and(|message| {
        message
            .html_bodies()
            .any(|part| matches!(part.body, PartType::Html(_)))
    })
}

fn raw_body_preferring(
    raw: &[u8],
    preference: BodyPreference,
) -> RawBody {
    let Some(message) = MessageParser::default().parse(raw) else {
        return RawBody::Empty;
    };
    let html = message
        .html_bodies()
        .find(|part| matches!(part.body, PartType::Html(_)))
        .map(|part| {
            part.text_contents().unwrap_or_default().to_owned()
        });
    if preference == BodyPreference::Html
        && let Some(html) = html.clone()
    {
        return RawBody::Html(html);
    }
    let plain = message
        .text_bodies()
        .find(|part| matches!(part.body, PartType::Text(_)))
        .map(plain_text)
        .filter(|text| !text.trim().is_empty());
    if let Some(text) = plain {
        return RawBody::Plain(text);
    }
    match html {
        Some(html) => RawBody::Html(html),
        None => RawBody::Empty,
    }
}

/// Unlike body_text, an html part renders through the html
/// converter here instead of collapsing to a notice, so the
/// pager's html view shows real content.
pub fn body_text_preferring(
    raw: &[u8],
    preference: BodyPreference,
) -> BodyText {
    match raw_body_preferring(raw, preference) {
        RawBody::Plain(text) => BodyText {
            text,
            kind: BodyKind::Plain,
        },
        RawBody::Html(html) => BodyText {
            text: rendered_html_text(&html),
            kind: BodyKind::Plain,
        },
        RawBody::Empty => empty(),
    }
}

fn rendered_html_text(html: &str) -> String {
    let lines: Vec<String> = html_body(html)
        .lines
        .into_iter()
        .map(|line| line.text)
        .collect();
    lines.join("\n")
}

fn plain_text(part: &MessagePart) -> String {
    let text = part.text_contents().unwrap_or_default();
    if !is_flowed(part) {
        return text.replace("\r\n", "\n");
    }
    unflow(text, delsp(part))
}

fn is_flowed(part: &MessagePart) -> bool {
    part.content_type()
        .is_some_and(|ct| attribute_is(ct, "format", "flowed"))
}

fn delsp(part: &MessagePart) -> bool {
    part.content_type()
        .is_some_and(|ct| attribute_is(ct, "delsp", "yes"))
}

fn attribute_is(
    content_type: &ContentType,
    name: &str,
    value: &str,
) -> bool {
    content_type
        .attribute(name)
        .is_some_and(|found| found.eq_ignore_ascii_case(value))
}

fn empty() -> BodyText {
    BodyText {
        text: String::new(),
        kind: BodyKind::Empty,
    }
}

#[cfg(test)]
mod tests {
    use super::{BodyKind, HTML_ONLY_NOTICE, body_text};

    #[test]
    fn extracts_the_right_body() {
        let cases = [
            (
                "single-part plain",
                concat!(
                    "From: alice@example.com\r\n",
                    "To: bob@example.com\r\n",
                    "Subject: plain\r\n",
                    "Content-Type: text/plain; ",
                    "charset=utf-8\r\n",
                    "\r\n",
                    "Hello there\r\n",
                ),
                BodyKind::Plain,
                "Hello there\n",
            ),
            (
                "alternative prefers plain over html",
                concat!(
                    "From: alice@example.com\r\n",
                    "Subject: alt\r\n",
                    "MIME-Version: 1.0\r\n",
                    "Content-Type: multipart/alternative; ",
                    "boundary=\"b1\"\r\n",
                    "\r\n",
                    "--b1\r\n",
                    "Content-Type: text/plain\r\n",
                    "\r\n",
                    "Plain wins\r\n",
                    "--b1\r\n",
                    "Content-Type: text/html\r\n",
                    "\r\n",
                    "<html><body><p>Rich</p></body>",
                    "</html>\r\n",
                    "--b1--\r\n",
                ),
                BodyKind::Plain,
                "Plain wins",
            ),
            (
                "nested multipart finds the plain part",
                concat!(
                    "From: alice@example.com\r\n",
                    "Subject: nested\r\n",
                    "MIME-Version: 1.0\r\n",
                    "Content-Type: multipart/mixed; ",
                    "boundary=\"outer\"\r\n",
                    "\r\n",
                    "--outer\r\n",
                    "Content-Type: multipart/alternative; ",
                    "boundary=\"inner\"\r\n",
                    "\r\n",
                    "--inner\r\n",
                    "Content-Type: text/plain\r\n",
                    "\r\n",
                    "Deep plain\r\n",
                    "--inner\r\n",
                    "Content-Type: text/html\r\n",
                    "\r\n",
                    "<p>Deep rich</p>\r\n",
                    "--inner--\r\n",
                    "--outer\r\n",
                    "Content-Type: application/pdf\r\n",
                    "Content-Disposition: attachment; ",
                    "filename=\"a.pdf\"\r\n",
                    "\r\n",
                    "%PDF-1.4\r\n",
                    "--outer--\r\n",
                ),
                BodyKind::Plain,
                "Deep plain",
            ),
            (
                "html only yields a placeholder",
                concat!(
                    "From: alice@example.com\r\n",
                    "Subject: html\r\n",
                    "MIME-Version: 1.0\r\n",
                    "Content-Type: text/html\r\n",
                    "\r\n",
                    "<html><body><p>Only rich</p>",
                    "</body></html>\r\n",
                ),
                BodyKind::HtmlOnly,
                HTML_ONLY_NOTICE,
            ),
            (
                "flowed body is unflowed",
                concat!(
                    "From: alice@example.com\r\n",
                    "Subject: flowed\r\n",
                    "Content-Type: text/plain; ",
                    "format=flowed\r\n",
                    "\r\n",
                    "soft \r\n",
                    "wrap\r\n",
                ),
                BodyKind::Plain,
                "soft wrap\n",
            ),
            (
                "flowed delsp joins without a space",
                concat!(
                    "From: alice@example.com\r\n",
                    "Subject: delsp\r\n",
                    "Content-Type: text/plain; ",
                    "format=flowed; delsp=yes\r\n",
                    "\r\n",
                    "unbro \r\n",
                    "ken\r\n",
                ),
                BodyKind::Plain,
                "unbroken\n",
            ),
            (
                "headers only means empty",
                concat!(
                    "From: alice@example.com\r\n",
                    "Subject: nothing\r\n",
                    "\r\n",
                ),
                BodyKind::Empty,
                "",
            ),
        ];
        for (name, raw, kind, text) in cases {
            let body = body_text(raw.as_bytes());
            assert_eq!(body.kind, kind, "kind for `{name}`");
            assert_eq!(body.text, text, "text for `{name}`");
        }
    }

    #[test]
    fn latin_1_part_decodes() {
        let raw: &[u8] = concat!(
            "From: alice@example.com\r\n",
            "Subject: charset\r\n",
            "Content-Type: text/plain; ",
            "charset=iso-8859-1\r\n",
            "Content-Transfer-Encoding: 8bit\r\n",
            "\r\n",
        )
        .as_bytes();
        let mut message = raw.to_vec();
        message.extend_from_slice(b"caf\xE9 na\xEFve\r\n");

        let body = body_text(&message);
        assert_eq!(body.kind, BodyKind::Plain);
        assert_eq!(body.text, "caf\u{e9} na\u{ef}ve\n");
    }
}

pub fn delivered_addresses(raw: &[u8]) -> Vec<String> {
    let Some(message) = MessageParser::default().parse(raw) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for header in [HeaderName::To, HeaderName::Cc] {
        let Some(addresses) = message.header(header) else {
            continue;
        };
        collect_addresses(addresses, &mut out);
    }
    out
}

fn collect_addresses(value: &HeaderValue<'_>, out: &mut Vec<String>) {
    let HeaderValue::Address(address) = value else {
        return;
    };
    for entry in address.iter() {
        let Some(email) = entry.address.as_ref() else {
            continue;
        };
        out.push(email.to_string());
    }
}

#[cfg(test)]
mod delivered_tests {
    use super::delivered_addresses;

    #[test]
    fn to_and_cc_addresses_come_back_bare() {
        let raw = b"From: a@example.com\r\n\
            To: Mara Voss <mara@example.com>, b@example.com\r\n\
            Cc: shop-orders@quin.example.com\r\n\
            Subject: x\r\n\r\nbody";
        assert_eq!(
            delivered_addresses(raw),
            vec![
                "mara@example.com".to_string(),
                "b@example.com".to_string(),
                "shop-orders@quin.example.com".to_string(),
            ]
        );
    }

    #[test]
    fn garbage_yields_nothing() {
        assert!(delivered_addresses(b"").is_empty());
    }
}
