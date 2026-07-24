use mail_parser::MessageParser;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListHeaders {
    pub list_id: Option<String>,
    pub list_post: ListPost,
    pub followup_to: Vec<String>,
    pub unsubscribe: Vec<String>,
    pub one_click_post: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListPost {
    Mailto(String),
    No,
    Absent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListReply {
    Post(Vec<String>),
    Refused,
    ReplyAll,
    NotList,
}

const MAILTO_SCHEME: &str = "mailto:";
const POSTING_FORBIDDEN: &str = "NO";
const ONE_CLICK_TOKEN: &str = "one-click";

pub fn list_headers(raw: &[u8]) -> ListHeaders {
    let message = MessageParser::default().parse(raw);
    let value = |name: &str| {
        message
            .as_ref()
            .and_then(|parsed| parsed.header_raw(name))
            .map(unfold)
    };
    ListHeaders {
        list_id: value("List-Id").and_then(|text| list_id_of(&text)),
        list_post: value("List-Post")
            .map(|text| list_post_of(&text))
            .unwrap_or(ListPost::Absent),
        followup_to: value("Mail-Followup-To")
            .map(|text| addresses_of(&text))
            .unwrap_or_default(),
        unsubscribe: value("List-Unsubscribe")
            .map(|text| bracketed_uris(&text))
            .unwrap_or_default(),
        one_click_post: value("List-Unsubscribe-Post").is_some_and(
            |text| text.to_ascii_lowercase().contains(ONE_CLICK_TOKEN),
        ),
    }
}

/// The reply-to-list ruling for a message. Mail-Followup-To is
/// the author's explicit routing and always wins; a List-Post
/// mailto is the list's own posting address; `List-Post: NO`
/// refuses; a list without a usable List-Post falls back to
/// reply-all rather than guessing an address.
pub fn reply_to_list(headers: &ListHeaders) -> ListReply {
    if !headers.followup_to.is_empty() {
        return ListReply::Post(headers.followup_to.clone());
    }
    match &headers.list_post {
        ListPost::Mailto(address) => {
            ListReply::Post(vec![address.clone()])
        }
        ListPost::No => ListReply::Refused,
        ListPost::Absent if headers.list_id.is_some() => {
            ListReply::ReplyAll
        }
        ListPost::Absent => ListReply::NotList,
    }
}

fn unfold(raw: &str) -> String {
    raw.replace(['\r', '\n'], "")
}

fn list_id_of(value: &str) -> Option<String> {
    let id =
        angle_content(value).unwrap_or_else(|| strip_comments(value));
    let id = id.trim();
    (!id.is_empty()).then(|| id.to_string())
}

fn list_post_of(value: &str) -> ListPost {
    let plain = strip_comments(value);
    if plain.trim().eq_ignore_ascii_case(POSTING_FORBIDDEN) {
        return ListPost::No;
    }
    bracketed_uris(value)
        .iter()
        .find_map(|uri| mailto_address(uri))
        .map(ListPost::Mailto)
        .unwrap_or(ListPost::Absent)
}

fn mailto_address(uri: &str) -> Option<String> {
    let (scheme, rest) = uri.split_at_checked(MAILTO_SCHEME.len())?;
    if !scheme.eq_ignore_ascii_case(MAILTO_SCHEME) {
        return None;
    }
    let address = rest.split('?').next().unwrap_or(rest).trim();
    (!address.is_empty()).then(|| address.to_string())
}

fn addresses_of(value: &str) -> Vec<String> {
    value
        .split(',')
        .filter_map(|entry| {
            let bare = angle_content(entry)
                .unwrap_or_else(|| entry.to_string());
            let bare = bare.trim().to_string();
            (!bare.is_empty()).then_some(bare)
        })
        .collect()
}

fn bracketed_uris(value: &str) -> Vec<String> {
    let mut uris = Vec::new();
    let mut rest = value;
    while let Some((_, tail)) = rest.split_once('<') {
        let Some((uri, after)) = tail.split_once('>') else {
            break;
        };
        let uri = uri.trim();
        if !uri.is_empty() {
            uris.push(uri.to_string());
        }
        rest = after;
    }
    uris
}

fn angle_content(value: &str) -> Option<String> {
    let (_, tail) = value.split_once('<')?;
    let (inner, _) = tail.split_once('>')?;
    Some(inner.to_string())
}

fn strip_comments(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut depth = 0u32;
    for ch in value.chars() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(headers: &str) -> Vec<u8> {
        format!(
            "From: sender@example.com\r\n\
             To: list@example.com\r\n\
             Subject: x\r\n\
             {headers}\r\n\
             \r\n\
             body\r\n"
        )
        .into_bytes()
    }

    #[test]
    fn list_headers_parse_every_shape() {
        let cases: &[(&str, &str, ListHeaders)] = &[
            (
                "the full set",
                "List-Id: Devel <devel.lists.example.com>\r\n\
                 List-Post: <mailto:devel@lists.example.com>\r\n\
                 List-Unsubscribe: \
                 <mailto:devel-leave@lists.example.com>, \
                 <https://lists.example.com/u/1>\r\n\
                 List-Unsubscribe-Post: \
                 List-Unsubscribe=One-Click",
                ListHeaders {
                    list_id: Some(
                        "devel.lists.example.com".to_string(),
                    ),
                    list_post: ListPost::Mailto(
                        "devel@lists.example.com".to_string(),
                    ),
                    followup_to: Vec::new(),
                    unsubscribe: vec![
                        "mailto:devel-leave@lists.example.com"
                            .to_string(),
                        "https://lists.example.com/u/1".to_string(),
                    ],
                    one_click_post: true,
                },
            ),
            (
                "posting refused with a comment",
                "List-Id: <announce.example.com>\r\n\
                 List-Post: NO (announcements only)",
                ListHeaders {
                    list_id: Some("announce.example.com".to_string()),
                    list_post: ListPost::No,
                    followup_to: Vec::new(),
                    unsubscribe: Vec::new(),
                    one_click_post: false,
                },
            ),
            (
                "folded list-post with a query and comment",
                "List-Id: bare.example.com\r\n\
                 List-Post: <mailto:devel@example.com\
                 ?subject=post>\r\n (moderated)",
                ListHeaders {
                    list_id: Some("bare.example.com".to_string()),
                    list_post: ListPost::Mailto(
                        "devel@example.com".to_string(),
                    ),
                    followup_to: Vec::new(),
                    unsubscribe: Vec::new(),
                    one_click_post: false,
                },
            ),
            (
                "mail-followup-to with display names",
                "Mail-Followup-To: Devel \
                 <devel@example.com>, keep@example.com",
                ListHeaders {
                    list_id: None,
                    list_post: ListPost::Absent,
                    followup_to: vec![
                        "devel@example.com".to_string(),
                        "keep@example.com".to_string(),
                    ],
                    unsubscribe: Vec::new(),
                    one_click_post: false,
                },
            ),
            (
                "https-only unsubscribe without one-click",
                "List-Id: <shop.example.com>\r\n\
                 List-Unsubscribe: \
                 <https://shop.example.com/unsub>",
                ListHeaders {
                    list_id: Some("shop.example.com".to_string()),
                    list_post: ListPost::Absent,
                    followup_to: Vec::new(),
                    unsubscribe: vec![
                        "https://shop.example.com/unsub".to_string(),
                    ],
                    one_click_post: false,
                },
            ),
            (
                "no list headers at all",
                "X-Nothing: here",
                ListHeaders {
                    list_id: None,
                    list_post: ListPost::Absent,
                    followup_to: Vec::new(),
                    unsubscribe: Vec::new(),
                    one_click_post: false,
                },
            ),
        ];
        for (name, headers, expected) in cases {
            assert_eq!(
                &list_headers(&message(headers)),
                expected,
                "{name}"
            );
        }
    }

    #[test]
    fn non_mailto_list_post_is_not_guessed_at() {
        let parsed = list_headers(&message(
            "List-Id: <web.example.com>\r\n\
             List-Post: <https://example.com/post>",
        ));
        assert_eq!(parsed.list_post, ListPost::Absent);
        assert_eq!(reply_to_list(&parsed), ListReply::ReplyAll);
    }

    #[test]
    fn garbage_input_yields_no_list() {
        let parsed = list_headers(b"");
        assert_eq!(reply_to_list(&parsed), ListReply::NotList);
    }

    #[test]
    fn reply_resolution_covers_every_ruling() {
        let base = ListHeaders {
            list_id: None,
            list_post: ListPost::Absent,
            followup_to: Vec::new(),
            unsubscribe: Vec::new(),
            one_click_post: false,
        };
        let cases: &[(&str, ListHeaders, ListReply)] = &[
            (
                "list-post mailto posts to the list",
                ListHeaders {
                    list_id: Some("devel.example.com".to_string()),
                    list_post: ListPost::Mailto(
                        "devel@example.com".to_string(),
                    ),
                    ..base.clone()
                },
                ListReply::Post(vec!["devel@example.com".to_string()]),
            ),
            (
                "mail-followup-to wins over list-post",
                ListHeaders {
                    list_post: ListPost::Mailto(
                        "devel@example.com".to_string(),
                    ),
                    followup_to: vec![
                        "devel@example.com".to_string(),
                        "author@example.com".to_string(),
                    ],
                    ..base.clone()
                },
                ListReply::Post(vec![
                    "devel@example.com".to_string(),
                    "author@example.com".to_string(),
                ]),
            ),
            (
                "mail-followup-to wins even over a refusal",
                ListHeaders {
                    list_post: ListPost::No,
                    followup_to: vec!["author@example.com".to_string()],
                    ..base.clone()
                },
                ListReply::Post(vec!["author@example.com".to_string()]),
            ),
            (
                "list-post no refuses",
                ListHeaders {
                    list_id: Some("announce.example.com".to_string()),
                    list_post: ListPost::No,
                    ..base.clone()
                },
                ListReply::Refused,
            ),
            (
                "a list without list-post falls back",
                ListHeaders {
                    list_id: Some("old.example.com".to_string()),
                    ..base.clone()
                },
                ListReply::ReplyAll,
            ),
            (
                "no list markers at all",
                base.clone(),
                ListReply::NotList,
            ),
        ];
        for (name, headers, expected) in cases {
            assert_eq!(&reply_to_list(headers), expected, "{name}");
        }
    }
}
