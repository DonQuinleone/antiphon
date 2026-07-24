use antiphon_render::{Draft, build_message};
use antiphon_store::Envelope;

use super::draw::sender_name;
use super::identity::ComposeIdentity;

pub const ATTRIBUTION_DATE_FORMAT: &str = "%a, %d %b %Y at %H:%M";
const REPLY_PREFIX: &str = "re:";

pub struct ReplySource<'a> {
    pub from: &'a str,
    pub subject: &'a str,
    pub message_id: &'a str,
    pub date: &'a str,
    pub body: &'a str,
}

pub fn fresh_draft(
    identity: &ComposeIdentity,
    template: Option<&str>,
    date: &str,
) -> String {
    let body = match template {
        Some(template) => expand_for(identity, template, "", date),
        None => String::new(),
    };
    draft_text(identity, "", "", "", &body, None)
}

pub fn unsubscribe_draft(
    identity: &ComposeIdentity,
    mailto: &antiphon_render::MailtoUnsubscribe,
) -> String {
    draft_text(
        identity,
        &mailto.address,
        "",
        mailto.subject.as_deref().unwrap_or(""),
        mailto.body.as_deref().unwrap_or(""),
        None,
    )
}

pub fn reply_draft_to(
    identity: &ComposeIdentity,
    source: &ReplySource<'_>,
    to: &str,
    cc: &str,
    template: Option<&str>,
) -> String {
    let quoted = quoted_body(source);
    let body = match template {
        Some(template) => {
            expand_for(identity, template, &quoted, source.date)
        }
        None => quoted,
    };
    draft_text(
        identity,
        to,
        cc,
        &reply_subject(source.subject),
        &body,
        Some(source.message_id),
    )
}

fn expand_for(
    identity: &ComposeIdentity,
    template: &str,
    quoted: &str,
    date: &str,
) -> String {
    antiphon_render::expand_template(
        template,
        &antiphon_render::TemplateVars {
            from: &identity.address,
            name: identity.name.as_deref().unwrap_or(""),
            date,
            quoted,
        },
    )
}

fn draft_text(
    identity: &ComposeIdentity,
    to: &str,
    cc: &str,
    subject: &str,
    quoted: &str,
    reply_to_id: Option<&str>,
) -> String {
    let mut text = format!(
        "From: {}\nTo: {to}\nCc: {cc}\nSubject: {subject}\n",
        from_header(identity),
    );
    if let Some(id) = reply_to_id {
        text.push_str(&format!(
            "In-Reply-To: <{id}>\nReferences: <{id}>\n"
        ));
    }
    text.push('\n');
    text.push_str(quoted);
    text.push('\n');
    text.push_str(&signature_block(identity));
    text
}

fn from_header(identity: &ComposeIdentity) -> String {
    match &identity.name {
        Some(name) => format!("{name} <{}>", identity.address),
        None => identity.address.clone(),
    }
}

fn signature_block(identity: &ComposeIdentity) -> String {
    let Some(signature) = &identity.signature else {
        return String::new();
    };
    let trimmed = signature.trim_end();
    if trimmed.is_empty() {
        return String::new();
    }
    format!("-- \n{trimmed}\n")
}

fn reply_subject(subject: &str) -> String {
    let trimmed = subject.trim();
    let lowered = trimmed.to_ascii_lowercase();
    if lowered.starts_with(REPLY_PREFIX) {
        return trimmed.to_string();
    }
    format!("Re: {trimmed}")
}

fn quoted_body(source: &ReplySource<'_>) -> String {
    let mut out = format!(
        "On {}, {} wrote:\n",
        source.date,
        sender_name(source.from),
    );
    for line in source.body.trim_end().lines() {
        out.push_str(&quote_line(line));
    }
    out
}

fn quote_line(line: &str) -> String {
    if line.is_empty() {
        return ">\n".to_string();
    }
    format!("> {line}\n")
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ParsedDraft {
    pub from_name: Option<String>,
    pub from: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub body: String,
}

pub fn parse_draft(text: &str) -> Result<ParsedDraft, String> {
    let Some((headers, body)) = text.split_once("\n\n") else {
        return Err(
            "draft has no blank line after the headers".to_string()
        );
    };
    let mut parsed = ParsedDraft {
        body: body.to_string(),
        ..ParsedDraft::default()
    };
    for line in headers.lines() {
        let Some((key, value)) = line.split_once(':') else {
            return Err(format!("malformed header line: {line}"));
        };
        apply_header(&mut parsed, key.trim(), value.trim())?;
    }
    validate(&parsed)?;
    Ok(parsed)
}

fn apply_header(
    parsed: &mut ParsedDraft,
    key: &str,
    value: &str,
) -> Result<(), String> {
    match key.to_ascii_lowercase().as_str() {
        "from" => set_from(parsed, value),
        "to" => parsed.to = address_list(value),
        "cc" => parsed.cc = address_list(value),
        "subject" => parsed.subject = value.to_string(),
        "in-reply-to" => parsed.in_reply_to = single_id(value),
        "references" => parsed.references = id_list(value),
        other => return Err(format!("unknown header: {other}")),
    }
    Ok(())
}

fn validate(parsed: &ParsedDraft) -> Result<(), String> {
    if !parsed.from.contains('@') {
        return Err("From needs a full address".to_string());
    }
    if parsed.to.is_empty() {
        return Err("no recipients in To".to_string());
    }
    Ok(())
}

fn set_from(parsed: &mut ParsedDraft, value: &str) {
    parsed.from = bare_address(value);
    let name = value
        .split_once('<')
        .map(|(name, _)| name.trim().trim_matches('"').trim())
        .unwrap_or("");
    parsed.from_name = (!name.is_empty()).then(|| name.to_string());
}

fn address_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(bare_address)
        .filter(|address| !address.is_empty())
        .collect()
}

pub(super) fn bare_address(value: &str) -> String {
    let bracketed = value
        .split_once('<')
        .and_then(|(_, rest)| rest.split_once('>'))
        .map(|(inner, _)| inner);
    bracketed.unwrap_or(value).trim().to_string()
}

fn single_id(value: &str) -> Option<String> {
    let id = value.trim().trim_matches(['<', '>']).to_string();
    (!id.is_empty()).then_some(id)
}

fn id_list(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .map(|id| id.trim_matches(['<', '>']).to_string())
        .filter(|id| !id.is_empty())
        .collect()
}

pub fn draft_unchanged(written: &str, edited: &str) -> bool {
    written == edited
}

pub fn assemble(parsed: &ParsedDraft, date_unix: i64) -> Vec<u8> {
    let (_, domain) =
        parsed.from.rsplit_once('@').expect("validated at parse");
    let draft = Draft {
        from_name: parsed.from_name.as_deref(),
        from: &parsed.from,
        to: as_strs(&parsed.to),
        cc: as_strs(&parsed.cc),
        subject: &parsed.subject,
        in_reply_to: parsed.in_reply_to.as_deref(),
        references: as_strs(&parsed.references),
        body: &parsed.body,
        signature: None,
    };
    build_message(&draft, domain, date_unix)
}

pub fn envelope(account: &str, parsed: &ParsedDraft) -> Envelope {
    Envelope {
        account: account.to_string(),
        from: parsed.from.clone(),
        recipients: parsed
            .to
            .iter()
            .chain(&parsed.cc)
            .cloned()
            .collect(),
    }
}

fn as_strs(values: &[String]) -> Vec<&str> {
    values.iter().map(String::as_str).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> ComposeIdentity {
        ComposeIdentity {
            name: Some("Tester".to_string()),
            address: "tester@example.com".to_string(),
            signature: Some("Kind regards\n".to_string()),
            pgp_sign: false,
            pgp_key: None,
        }
    }

    fn source() -> ReplySource<'static> {
        ReplySource {
            from: "Alba Fenwick <alba@example.com>",
            subject: "Greetings",
            message_id: "id-1@example.com",
            date: "Thu, 23 Jul 2026 at 09:00",
            body: "First line.\n\nSecond line.\n",
        }
    }

    fn reply(template: Option<&str>) -> String {
        reply_draft_to(
            &identity(),
            &source(),
            "alba@example.com",
            "",
            template,
        )
    }

    #[test]
    fn templates_shape_both_compose_paths() {
        let fresh = fresh_draft(
            &identity(),
            Some("Dear {name} on {date}:\n{quoted}"),
            "24 Jul",
        );
        assert!(fresh.contains("Dear Tester on 24 Jul:"));
        let reply = reply(Some("{quoted}\nRegards, {name}"));
        assert!(reply.contains("Regards, Tester"));
        assert!(reply.contains("> "));
    }

    #[test]
    fn reply_prefills_the_header_block() {
        let draft = reply(None);
        let expected = "From: Tester <tester@example.com>\n\
                        To: alba@example.com\n\
                        Cc: \n\
                        Subject: Re: Greetings\n\
                        In-Reply-To: <id-1@example.com>\n\
                        References: <id-1@example.com>\n\n";
        assert!(draft.starts_with(expected), "{draft}");
    }

    #[test]
    fn reply_quotes_below_an_attribution_line() {
        let draft = reply(None);
        let quoted = "On Thu, 23 Jul 2026 at 09:00, \
                      Alba Fenwick wrote:\n\
                      > First line.\n\
                      >\n\
                      > Second line.\n";
        assert!(draft.contains(quoted), "{draft}");
        assert!(draft.ends_with("\n-- \nKind regards\n"), "{draft}");
    }

    #[test]
    fn list_replies_carry_explicit_recipients() {
        let draft = reply_draft_to(
            &identity(),
            &source(),
            "devel@example.com",
            "mara@example.com, quin@example.com",
            None,
        );
        let parsed = parse_draft(&draft).unwrap();
        assert_eq!(parsed.to, ["devel@example.com"]);
        assert_eq!(parsed.cc, ["mara@example.com", "quin@example.com"]);
        assert_eq!(parsed.subject, "Re: Greetings");
        assert_eq!(
            parsed.in_reply_to.as_deref(),
            Some("id-1@example.com")
        );
    }

    #[test]
    fn unsubscribe_drafts_prefill_from_the_mailto_uri() {
        let mailto = antiphon_render::MailtoUnsubscribe {
            address: "leave@example.com".to_string(),
            subject: Some("unsubscribe me".to_string()),
            body: Some("please".to_string()),
        };
        let draft = unsubscribe_draft(&identity(), &mailto);
        assert!(draft.contains("To: leave@example.com\n"));
        assert!(draft.contains("Subject: unsubscribe me\n"));
        assert!(draft.contains("\n\nplease\n"), "{draft}");

        let bare = antiphon_render::MailtoUnsubscribe {
            address: "leave@example.com".to_string(),
            subject: None,
            body: None,
        };
        let draft = unsubscribe_draft(&identity(), &bare);
        assert!(draft.contains("Subject: \n"));
    }

    #[test]
    fn reply_subjects_gain_re_exactly_once() {
        let cases = [
            ("Greetings", "Re: Greetings"),
            ("Re: Greetings", "Re: Greetings"),
            ("RE: Greetings", "RE: Greetings"),
            ("re: Greetings", "re: Greetings"),
        ];
        for (subject, expected) in cases {
            assert_eq!(reply_subject(subject), expected, "{subject}");
        }
    }

    #[test]
    fn fresh_drafts_leave_recipients_and_subject_open() {
        let draft = fresh_draft(&identity(), None, "");
        assert!(
            draft.starts_with(
                "From: Tester <tester@example.com>\nTo: \n"
            )
        );
        assert!(!draft.contains("In-Reply-To"));
        assert!(draft.ends_with("-- \nKind regards\n"));
    }

    #[test]
    fn drafts_round_trip_through_the_parser() {
        let mut draft = reply(None);
        draft.push_str("Thanks, noted.\n");
        let parsed = parse_draft(&draft).unwrap();
        assert_eq!(parsed.from, "tester@example.com");
        assert_eq!(parsed.from_name.as_deref(), Some("Tester"));
        assert_eq!(parsed.to, ["alba@example.com"]);
        assert!(parsed.cc.is_empty());
        assert_eq!(parsed.subject, "Re: Greetings");
        assert_eq!(
            parsed.in_reply_to.as_deref(),
            Some("id-1@example.com")
        );
        assert_eq!(parsed.references, ["id-1@example.com"]);
        assert!(parsed.body.contains("> First line."));
        assert!(parsed.body.ends_with("Thanks, noted.\n"));
    }

    #[test]
    fn unknown_headers_are_an_error() {
        let draft = "From: a@b.c\nTo: d@e.f\nX-Loud: yes\n\nhi\n";
        let error = parse_draft(draft).unwrap_err();
        assert_eq!(error, "unknown header: x-loud");
    }

    #[test]
    fn recipient_lists_accept_commas_and_angle_brackets() {
        let draft = "From: a@b.c\n\
                     To: Mara <mara@example.com>, quin@example.com\n\
                     Cc: <cc@example.com>\n\nhi\n";
        let parsed = parse_draft(draft).unwrap();
        assert_eq!(parsed.to, ["mara@example.com", "quin@example.com"]);
        assert_eq!(parsed.cc, ["cc@example.com"]);
        let envelope = envelope("personal", &parsed);
        assert_eq!(envelope.recipients.len(), 3);
        assert_eq!(envelope.from, "a@b.c");
    }

    #[test]
    fn empty_recipients_and_headerless_drafts_fail() {
        let unfilled = fresh_draft(&identity(), None, "");
        let error = parse_draft(&unfilled).unwrap_err();
        assert_eq!(error, "no recipients in To");
        assert!(parse_draft("no headers at all").is_err());
        assert!(
            parse_draft("From: a@b.c\nnot a header\n\nhi\n")
                .unwrap_err()
                .contains("malformed header line")
        );
    }

    #[test]
    fn unchanged_drafts_are_detected() {
        let written = fresh_draft(&identity(), None, "");
        assert!(draft_unchanged(&written, &written.clone()));
        let edited = format!("{written}A new body line.\n");
        assert!(!draft_unchanged(&written, &edited));
    }

    #[test]
    fn assembly_derives_the_message_id_domain_from_the_sender() {
        let parsed = parse_draft(
            "From: Tester <tester@example.com>\n\
             To: alba@example.com\n\nBody here.\n",
        )
        .unwrap();
        let raw = assemble(&parsed, 1_753_380_000);
        let text = String::from_utf8(raw).unwrap();
        assert!(text.contains(".antiphon@example.com>"));
        assert!(text.contains("format=flowed"));
        assert!(text.contains("Body here."));
    }
}
