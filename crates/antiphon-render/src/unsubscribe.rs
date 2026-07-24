use crate::list::ListHeaders;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MailtoUnsubscribe {
    pub address: String,
    pub subject: Option<String>,
    pub body: Option<String>,
}

/// How to honour an unsubscribe request, in order of
/// preference: an RFC 8058 one-click POST (confirmed in-app,
/// executed by antiphond), a mailto compose, or a web URL shown
/// for the user to open. Nothing here touches the network.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unsubscribe {
    OneClick { url: String },
    Mailto(MailtoUnsubscribe),
    Browse { url: String },
    None,
}

const MAILTO_SCHEME: &str = "mailto:";
const HTTPS_SCHEME: &str = "https:";
const HTTP_SCHEME: &str = "http:";
const PERCENT_ESCAPE_LEN: usize = 3;

pub fn unsubscribe_method(headers: &ListHeaders) -> Unsubscribe {
    let https = headers
        .unsubscribe
        .iter()
        .find(|uri| has_scheme(uri, HTTPS_SCHEME));
    if headers.one_click_post
        && let Some(url) = https
    {
        return Unsubscribe::OneClick { url: url.clone() };
    }
    let mailto =
        headers.unsubscribe.iter().find_map(|uri| parse_mailto(uri));
    if let Some(mailto) = mailto {
        return Unsubscribe::Mailto(mailto);
    }
    let web = headers.unsubscribe.iter().find(|uri| {
        has_scheme(uri, HTTPS_SCHEME) || has_scheme(uri, HTTP_SCHEME)
    });
    match web {
        Some(url) => Unsubscribe::Browse { url: url.clone() },
        None => Unsubscribe::None,
    }
}

fn has_scheme(uri: &str, scheme: &str) -> bool {
    uri.split_at_checked(scheme.len())
        .is_some_and(|(head, _)| head.eq_ignore_ascii_case(scheme))
}

fn parse_mailto(uri: &str) -> Option<MailtoUnsubscribe> {
    let (scheme, rest) = uri.split_at_checked(MAILTO_SCHEME.len())?;
    if !scheme.eq_ignore_ascii_case(MAILTO_SCHEME) {
        return None;
    }
    let (address, query) = rest.split_once('?').unwrap_or((rest, ""));
    let address = percent_decode(address.trim());
    if address.is_empty() {
        return None;
    }
    let mut parsed = MailtoUnsubscribe {
        address,
        subject: None,
        body: None,
    };
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        apply_query(&mut parsed, key, value);
    }
    Some(parsed)
}

fn apply_query(parsed: &mut MailtoUnsubscribe, key: &str, value: &str) {
    match key.to_ascii_lowercase().as_str() {
        "subject" => parsed.subject = Some(percent_decode(value)),
        "body" => parsed.body = Some(percent_decode(value)),
        _ => {}
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match decoded_escape(bytes, index) {
            Some(byte) => {
                out.push(byte);
                index += PERCENT_ESCAPE_LEN;
            }
            None => {
                out.push(bytes[index]);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn decoded_escape(bytes: &[u8], index: usize) -> Option<u8> {
    if bytes[index] != b'%' {
        return None;
    }
    let digits = bytes.get(index + 1..index + PERCENT_ESCAPE_LEN)?;
    let digits = std::str::from_utf8(digits).ok()?;
    u8::from_str_radix(digits, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(uris: &[&str], one_click_post: bool) -> ListHeaders {
        ListHeaders {
            list_id: Some("devel.example.com".to_string()),
            list_post: crate::list::ListPost::Absent,
            followup_to: Vec::new(),
            unsubscribe: uris
                .iter()
                .map(|uri| (*uri).to_string())
                .collect(),
            one_click_post,
        }
    }

    #[test]
    fn method_resolution_prefers_one_click_then_mailto() {
        let cases: &[(&str, ListHeaders, Unsubscribe)] = &[
            (
                "one-click needs the post header and https",
                headers(
                    &[
                        "mailto:leave@example.com",
                        "https://example.com/u/1",
                    ],
                    true,
                ),
                Unsubscribe::OneClick {
                    url: "https://example.com/u/1".to_string(),
                },
            ),
            (
                "post header without https is not one-click",
                headers(&["mailto:leave@example.com"], true),
                Unsubscribe::Mailto(MailtoUnsubscribe {
                    address: "leave@example.com".to_string(),
                    subject: None,
                    body: None,
                }),
            ),
            (
                "mailto beats a plain https link",
                headers(
                    &[
                        "https://example.com/u/1",
                        "mailto:leave@example.com",
                    ],
                    false,
                ),
                Unsubscribe::Mailto(MailtoUnsubscribe {
                    address: "leave@example.com".to_string(),
                    subject: None,
                    body: None,
                }),
            ),
            (
                "https-only is shown, never fetched",
                headers(&["https://example.com/u/1"], false),
                Unsubscribe::Browse {
                    url: "https://example.com/u/1".to_string(),
                },
            ),
            (
                "http-only still counts as a link",
                headers(&["http://example.com/u/1"], false),
                Unsubscribe::Browse {
                    url: "http://example.com/u/1".to_string(),
                },
            ),
            (
                "nothing to act on",
                headers(&[], false),
                Unsubscribe::None,
            ),
        ];
        for (name, given, expected) in cases {
            assert_eq!(&unsubscribe_method(given), expected, "{name}");
        }
    }

    #[test]
    fn mailto_queries_carry_subject_and_body() {
        let cases: &[(&str, Option<MailtoUnsubscribe>)] = &[
            (
                "mailto:leave@example.com?subject=unsubscribe%20me\
                 &body=please%2C%20now",
                Some(MailtoUnsubscribe {
                    address: "leave@example.com".to_string(),
                    subject: Some("unsubscribe me".to_string()),
                    body: Some("please, now".to_string()),
                }),
            ),
            (
                "MAILTO:leave@example.com?Subject=stop",
                Some(MailtoUnsubscribe {
                    address: "leave@example.com".to_string(),
                    subject: Some("stop".to_string()),
                    body: None,
                }),
            ),
            (
                "mailto:leave@example.com?x=1&subject=",
                Some(MailtoUnsubscribe {
                    address: "leave@example.com".to_string(),
                    subject: Some(String::new()),
                    body: None,
                }),
            ),
            ("mailto:?subject=empty", None),
            ("https://example.com/", None),
        ];
        for (uri, expected) in cases {
            assert_eq!(&parse_mailto(uri), expected, "{uri}");
        }
    }

    #[test]
    fn percent_decoding_survives_malformed_escapes() {
        let cases = [
            ("plain", "plain"),
            ("a%20b%2Cc", "a b,c"),
            ("bad%2", "bad%2"),
            ("bad%zz", "bad%zz"),
            ("%41%42", "AB"),
            ("", ""),
        ];
        for (given, expected) in cases {
            assert_eq!(percent_decode(given), expected, "{given}");
        }
    }
}
