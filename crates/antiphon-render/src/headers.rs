use mail_parser::parsers::MessageStream;
use mail_parser::{HeaderForm, HeaderName, MessageParser};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageHeader {
    pub name: String,
    pub value: String,
}

/// The headers a reader asked to see, in the asked order:
/// every instance of each name, matched case-insensitively,
/// encoded words decoded and folding unfolded.
pub fn selected_headers(
    raw: &[u8],
    names: &[String],
) -> Vec<MessageHeader> {
    let Some(message) = MessageParser::default().parse(raw) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for name in names {
        let display = display_name(name);
        let values = message.header_as(name.as_str(), HeaderForm::Text);
        out.extend(values.iter().map(|value| MessageHeader {
            name: display.clone(),
            value: value.as_text().unwrap_or_default().to_string(),
        }));
    }
    out
}

/// Every header of the message in wire order, decoded the
/// same way, for the show-everything toggle.
pub fn all_headers(raw: &[u8]) -> Vec<MessageHeader> {
    let Some(message) = MessageParser::default().parse(raw) else {
        return Vec::new();
    };
    message
        .headers_raw()
        .map(|(name, value)| MessageHeader {
            name: display_name(name),
            value: MessageStream::new(value.as_bytes())
                .parse_unstructured()
                .as_text()
                .unwrap_or_default()
                .to_string(),
        })
        .collect()
}

/// "from" displays as From and "x-mailer" as X-Mailer: known
/// names take their canonical RFC form, anything else
/// capitalises each hyphenated word.
fn display_name(name: &str) -> String {
    match HeaderName::parse(name) {
        None | Some(HeaderName::Other(_)) => capitalised(name),
        Some(known) => known.as_str().to_string(),
    }
}

fn capitalised(name: &str) -> String {
    name.split('-')
        .map(capitalised_word)
        .collect::<Vec<_>>()
        .join("-")
}

fn capitalised_word(word: &str) -> String {
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    first.to_ascii_uppercase().to_string()
        + &chars.as_str().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW: &str = concat!(
        "From: Mara Voss <mara@example.com>\r\n",
        "To: quin@example.com\r\n",
        "Received: from a.example.com\r\n",
        "Received: from b.example.com\r\n",
        "Subject: =?utf-8?q?caf=C3=A9_notes?=\r\n",
        "Date: Fri, 24 Jul 2026 09:00:00 +0000\r\n",
        "X-MAILER: antiphon 0.0.0\r\n",
        "Message-Id: <1@example.com>\r\n",
        "\r\n",
        "body\r\n",
    );

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_string()).collect()
    }

    type Expected<'a> = &'a [(&'a str, &'a str)];

    #[test]
    fn selection_follows_the_configured_order() {
        let cases: &[(&[&str], Expected)] = &[
            (
                &["from", "to", "date", "subject"],
                &[
                    ("From", "Mara Voss <mara@example.com>"),
                    ("To", "quin@example.com"),
                    ("Date", "Fri, 24 Jul 2026 09:00:00 +0000"),
                    ("Subject", "caf\u{e9} notes"),
                ],
            ),
            (
                &["x-mailer", "from"],
                &[
                    ("X-Mailer", "antiphon 0.0.0"),
                    ("From", "Mara Voss <mara@example.com>"),
                ],
            ),
            (
                &["received"],
                &[
                    ("Received", "from a.example.com"),
                    ("Received", "from b.example.com"),
                ],
            ),
            (&["x-absent", "cc"], &[]),
        ];
        for (asked, expected) in cases {
            let selected =
                selected_headers(RAW.as_bytes(), &names(asked));
            let got: Vec<(&str, &str)> = selected
                .iter()
                .map(|header| {
                    (header.name.as_str(), header.value.as_str())
                })
                .collect();
            assert_eq!(&got, expected, "{asked:?}");
        }
    }

    #[test]
    fn header_names_match_case_insensitively() {
        for asked in ["X-Mailer", "x-mailer", "X-MAILER"] {
            let selected =
                selected_headers(RAW.as_bytes(), &names(&[asked]));
            assert_eq!(selected.len(), 1, "{asked}");
            assert_eq!(selected[0].value, "antiphon 0.0.0");
        }
    }

    #[test]
    fn all_headers_keep_wire_order_and_decode() {
        let all = all_headers(RAW.as_bytes());
        let names: Vec<&str> =
            all.iter().map(|header| header.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "From",
                "To",
                "Received",
                "Received",
                "Subject",
                "Date",
                "X-Mailer",
                "Message-ID",
            ],
        );
        let subject = all
            .iter()
            .find(|header| header.name == "Subject")
            .expect("subject present");
        assert_eq!(subject.value, "caf\u{e9} notes");
    }

    #[test]
    fn garbage_yields_no_headers() {
        assert!(selected_headers(b"", &names(&["from"])).is_empty());
        assert!(all_headers(b"").is_empty());
    }

    #[test]
    fn display_names_take_canonical_forms() {
        let cases = [
            ("from", "From"),
            ("MESSAGE-ID", "Message-ID"),
            ("x-mailer", "X-Mailer"),
            ("x-spam-score", "X-Spam-Score"),
            ("weird name", "Weird name"),
        ];
        for (asked, expected) in cases {
            assert_eq!(display_name(asked), expected, "{asked}");
        }
    }
}
