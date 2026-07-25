use super::identity::ComposeIdentity;
use super::message_list::sender_name;

pub const ATTRIBUTION_DATE_FORMAT: &str = "%a, %d %b %Y at %H:%M";
const REPLY_PREFIX: &str = "re:";

pub struct ReplySource<'a> {
    pub from: &'a str,
    pub subject: &'a str,
    pub message_id: &'a str,
    pub date: &'a str,
    pub body: &'a str,
}

/// What a compose starts from: header field values and the
/// body text handed to the editor. Headers never travel
/// through the editor; they are edited as fields.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct DraftFields {
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub body: String,
}

pub fn fresh_fields(
    identity: &ComposeIdentity,
    template: Option<&str>,
    date: &str,
) -> DraftFields {
    let body = match template {
        Some(template) => expand_for(identity, template, "", date),
        None => String::new(),
    };
    DraftFields {
        body: with_signature(identity, &body),
        ..DraftFields::default()
    }
}

pub fn unsubscribe_fields(
    identity: &ComposeIdentity,
    mailto: &antiphon_render::MailtoUnsubscribe,
) -> DraftFields {
    DraftFields {
        to: mailto.address.clone(),
        subject: mailto.subject.clone().unwrap_or_default(),
        body: with_signature(
            identity,
            mailto.body.as_deref().unwrap_or(""),
        ),
        ..DraftFields::default()
    }
}

pub fn reply_fields(
    identity: &ComposeIdentity,
    source: &ReplySource<'_>,
    to: &str,
    cc: &str,
    template: Option<&str>,
) -> DraftFields {
    let quoted = quoted_body(source);
    let body = match template {
        Some(template) => {
            expand_for(identity, template, &quoted, source.date)
        }
        None => quoted,
    };
    DraftFields {
        to: to.to_string(),
        cc: cc.to_string(),
        subject: reply_subject(source.subject),
        in_reply_to: Some(source.message_id.to_string()),
        references: vec![source.message_id.to_string()],
        body: with_signature(identity, &body),
        ..DraftFields::default()
    }
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

fn with_signature(identity: &ComposeIdentity, body: &str) -> String {
    format!("{body}\n{}", signature_block(identity))
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

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::testkit::tester_identity as identity;

    fn source() -> ReplySource<'static> {
        ReplySource {
            from: "Alba Fenwick <alba@example.com>",
            subject: "Greetings",
            message_id: "id-1@example.com",
            date: "Thu, 23 Jul 2026 at 09:00",
            body: "First line.\n\nSecond line.\n",
        }
    }

    fn reply(template: Option<&str>) -> DraftFields {
        reply_fields(
            &identity(),
            &source(),
            "alba@example.com",
            "",
            template,
        )
    }

    #[test]
    fn templates_shape_both_compose_paths() {
        let fresh = fresh_fields(
            &identity(),
            Some("Dear {name} on {date}:\n{quoted}"),
            "24 Jul",
        );
        assert!(fresh.body.contains("Dear Tester on 24 Jul:"));
        let reply = reply(Some("{quoted}\nRegards, {name}"));
        assert!(reply.body.contains("Regards, Tester"));
        assert!(reply.body.contains("> "));
    }

    #[test]
    fn replies_prefill_the_fields_and_thread_headers() {
        let fields = reply(None);
        assert_eq!(fields.to, "alba@example.com");
        assert_eq!(fields.cc, "");
        assert_eq!(fields.subject, "Re: Greetings");
        assert_eq!(
            fields.in_reply_to.as_deref(),
            Some("id-1@example.com")
        );
        assert_eq!(fields.references, ["id-1@example.com"]);
    }

    #[test]
    fn replies_quote_below_an_attribution_line() {
        let fields = reply(None);
        let quoted = "On Thu, 23 Jul 2026 at 09:00, \
                      Alba Fenwick wrote:\n\
                      > First line.\n\
                      >\n\
                      > Second line.\n";
        assert!(fields.body.contains(quoted), "{}", fields.body);
        assert!(
            fields.body.ends_with("\n-- \nKind regards\n"),
            "{}",
            fields.body
        );
    }

    #[test]
    fn unsubscribe_fields_prefill_from_the_mailto_uri() {
        let mailto = antiphon_render::MailtoUnsubscribe {
            address: "leave@example.com".to_string(),
            subject: Some("unsubscribe me".to_string()),
            body: Some("please".to_string()),
        };
        let fields = unsubscribe_fields(&identity(), &mailto);
        assert_eq!(fields.to, "leave@example.com");
        assert_eq!(fields.subject, "unsubscribe me");
        assert!(fields.body.starts_with("please\n"), "{}", fields.body);

        let bare = antiphon_render::MailtoUnsubscribe {
            address: "leave@example.com".to_string(),
            subject: None,
            body: None,
        };
        let fields = unsubscribe_fields(&identity(), &bare);
        assert_eq!(fields.subject, "");
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
    fn fresh_fields_leave_recipients_and_subject_open() {
        let fields = fresh_fields(&identity(), None, "");
        assert_eq!(fields.to, "");
        assert_eq!(fields.subject, "");
        assert!(fields.in_reply_to.is_none());
        assert!(fields.body.ends_with("-- \nKind regards\n"));
    }
}
