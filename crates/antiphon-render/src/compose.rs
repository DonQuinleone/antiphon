use mail_builder::MessageBuilder;
use mail_builder::headers::address::Address;

use crate::attach::AttachmentPart;
use crate::flow;

pub struct Draft<'a> {
    pub from_name: Option<&'a str>,
    pub from: &'a str,
    pub to: Vec<&'a str>,
    pub cc: Vec<&'a str>,
    pub subject: &'a str,
    pub in_reply_to: Option<&'a str>,
    pub references: Vec<&'a str>,
    pub body: &'a str,
    pub signature: Option<&'a str>,
    pub attachments: Vec<AttachmentPart<'a>>,
}

pub fn build_message(
    draft: &Draft<'_>,
    message_id_domain: &str,
    date_unix: i64,
) -> Vec<u8> {
    let body = flow(&with_signature(draft.body, draft.signature));
    let mut builder = MessageBuilder::new()
        .message_id(message_id(message_id_domain, date_unix))
        .date(date_unix)
        .from(from_address(draft))
        .to(address_list(&draft.to))
        .subject(draft.subject)
        .text_body(body);
    if !draft.cc.is_empty() {
        builder = builder.cc(address_list(&draft.cc));
    }
    if let Some(parent) = draft.in_reply_to {
        builder = builder.in_reply_to(parent);
    }
    if !draft.references.is_empty() {
        builder = builder.references(draft.references.clone());
    }
    for part in &draft.attachments {
        builder = builder.attachment(
            part.content_type,
            part.filename,
            part.bytes,
        );
    }
    let raw = builder.write_to_vec().expect("in-memory write");
    mark_flowed(raw)
}

fn with_signature(body: &str, signature: Option<&str>) -> String {
    let Some(signature) = signature else {
        return body.to_string();
    };
    let trimmed = signature.trim_end();
    if trimmed.is_empty() {
        return body.to_string();
    }
    format!("{}\n-- \n{}", body.trim_end(), trimmed)
}

fn from_address<'a>(draft: &Draft<'a>) -> Address<'a> {
    match draft.from_name {
        Some(name) => Address::new_address(Some(name), draft.from),
        None => Address::new_address(None::<&str>, draft.from),
    }
}

fn address_list<'a>(addresses: &[&'a str]) -> Address<'a> {
    Address::new_list(
        addresses
            .iter()
            .map(|address| Address::new_address(None::<&str>, *address))
            .collect(),
    )
}

fn message_id(domain: &str, date_unix: i64) -> String {
    let entropy = std::process::id();
    format!("{date_unix}.{entropy}.antiphon@{domain}")
}

// mail-builder offers no per-part Content-Type parameters for
// text bodies, so the flowed declaration is patched into the
// generated header; the header is machine-written two lines
// above, making the textual match safe.
fn mark_flowed(raw: Vec<u8>) -> Vec<u8> {
    let text =
        String::from_utf8(raw).expect("mail-builder emits utf-8");
    text.replacen(
        "Content-Type: text/plain; charset=\"utf-8\"",
        "Content-Type: text/plain; charset=\"utf-8\"; \
         format=flowed",
        1,
    )
    .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft<'a>() -> Draft<'a> {
        Draft {
            from_name: Some("Q"),
            from: "quin@example.com",
            to: vec!["mara@example.com"],
            cc: Vec::new(),
            subject: "Rehearsal",
            in_reply_to: Some("parent@example.com"),
            references: vec!["root@example.com"],
            body: "See you Thursday.",
            signature: Some("Q\n"),
            attachments: Vec::new(),
        }
    }

    fn built() -> String {
        String::from_utf8(build_message(
            &draft(),
            "example.com",
            1_753_380_000,
        ))
        .unwrap()
    }

    #[test]
    fn headers_carry_the_draft() {
        let text = built();
        assert!(text.contains("From: \"Q\" <quin@example.com>"));
        assert!(text.contains("To: <mara@example.com>"));
        assert!(text.contains("Subject: Rehearsal"));
        assert!(text.contains("In-Reply-To: <parent@example.com>"));
        assert!(text.contains("References: <root@example.com>"));
        assert!(text.contains(".antiphon@example.com>"));
    }

    #[test]
    fn body_is_flowed_quoted_printable() {
        let text = built();
        assert!(text.contains("format=flowed"));
        assert!(
            text.contains(
                "Content-Transfer-Encoding: quoted-printable"
            )
        );
        assert!(text.contains("--=20"));
        assert!(text.contains("See you Thursday."));
    }

    #[test]
    fn parses_back_through_our_own_extractor() {
        let raw = build_message(&draft(), "example.com", 1_753_380_000);
        let body = crate::body_text(&raw);
        assert_eq!(body.kind, crate::BodyKind::Plain);
        assert!(body.text.contains("See you Thursday."));
        assert!(body.text.contains("-- \nQ"));
    }
}

pub struct TemplateVars<'a> {
    pub from: &'a str,
    pub name: &'a str,
    pub date: &'a str,
    pub quoted: &'a str,
}

pub fn expand_template(text: &str, vars: &TemplateVars<'_>) -> String {
    let pairs = [
        ("{from}", vars.from),
        ("{name}", vars.name),
        ("{date}", vars.date),
        ("{quoted}", vars.quoted),
    ];
    let mut out = text.to_string();
    for (token, value) in pairs {
        out = out.replace(token, value);
    }
    out
}

#[cfg(test)]
mod template_tests {
    use super::*;

    #[test]
    fn tokens_expand_and_unknown_braces_survive() {
        let vars = TemplateVars {
            from: "quin@example.com",
            name: "Q",
            date: "24 Jul",
            quoted: "> hi",
        };
        let text = "Dear {name} ({from}) on {date}:\n\
                    {quoted}\n{unknown} stays";
        assert_eq!(
            expand_template(text, &vars),
            "Dear Q (quin@example.com) on 24 Jul:\n\
             > hi\n{unknown} stays"
        );
    }
}
