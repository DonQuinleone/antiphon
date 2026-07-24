use antiphon_render::{ListHeaders, ListReply, reply_to_list};

use super::compose::bare_address;

#[derive(Debug)]
pub(super) struct ListRecipients {
    pub(super) to: Vec<String>,
    pub(super) cc: Vec<String>,
    pub(super) warning: Option<String>,
}

/// Recipients for a reply-to-list, or the status message
/// explaining why there are none. The reply-all fallback keeps
/// the sender in To, everyone else delivered in Cc, and warns
/// with the recipient count so a wide blast is never silent.
pub(super) fn list_recipients(
    headers: &ListHeaders,
    from: &str,
    delivered: &[String],
    own_address: &str,
) -> Result<ListRecipients, String> {
    match reply_to_list(headers) {
        ListReply::Post(addresses) => Ok(ListRecipients {
            to: addresses,
            cc: Vec::new(),
            warning: None,
        }),
        ListReply::Refused => Err(format!(
            "{} does not accept posts (List-Post: NO)",
            list_name(headers),
        )),
        ListReply::NotList => {
            Err("not a mailing-list message".to_string())
        }
        ListReply::ReplyAll => {
            Ok(reply_all(headers, from, delivered, own_address))
        }
    }
}

pub(super) fn list_name(headers: &ListHeaders) -> &str {
    headers.list_id.as_deref().unwrap_or("this list")
}

fn reply_all(
    headers: &ListHeaders,
    from: &str,
    delivered: &[String],
    own_address: &str,
) -> ListRecipients {
    let sender = bare_address(from);
    let mut cc: Vec<String> = Vec::new();
    for address in delivered {
        let skip = address.eq_ignore_ascii_case(&sender)
            || address.eq_ignore_ascii_case(own_address)
            || cc.iter().any(|kept| kept.eq_ignore_ascii_case(address));
        if skip {
            continue;
        }
        cc.push(address.clone());
    }
    let count = 1 + cc.len();
    let warning = format!(
        "{} has no List-Post header; replying to all \
         {count} recipient(s)",
        list_name(headers),
    );
    ListRecipients {
        to: vec![sender],
        cc,
        warning: Some(warning),
    }
}

#[cfg(test)]
mod tests {
    use antiphon_render::ListPost;

    use super::*;

    fn headers(
        list_id: Option<&str>,
        list_post: ListPost,
        followup_to: &[&str],
    ) -> ListHeaders {
        ListHeaders {
            list_id: list_id.map(str::to_string),
            list_post,
            followup_to: followup_to
                .iter()
                .map(|address| (*address).to_string())
                .collect(),
            unsubscribe: Vec::new(),
            one_click_post: false,
        }
    }

    #[test]
    fn list_post_and_followup_to_route_the_reply() {
        let posted = list_recipients(
            &headers(
                Some("devel.example.com"),
                ListPost::Mailto("devel@example.com".to_string()),
                &[],
            ),
            "Mara <mara@example.com>",
            &["devel@example.com".to_string()],
            "me@example.com",
        )
        .expect("a posting list resolves");
        assert_eq!(posted.to, ["devel@example.com"]);
        assert!(posted.cc.is_empty());
        assert!(posted.warning.is_none());

        let followed = list_recipients(
            &headers(
                None,
                ListPost::Mailto("devel@example.com".to_string()),
                &["devel@example.com", "mara@example.com"],
            ),
            "Mara <mara@example.com>",
            &[],
            "me@example.com",
        )
        .expect("followup-to resolves");
        assert_eq!(
            followed.to,
            ["devel@example.com", "mara@example.com"]
        );
    }

    #[test]
    fn refusal_and_non_lists_name_the_problem() {
        let refused = list_recipients(
            &headers(Some("announce.example.com"), ListPost::No, &[]),
            "a@example.com",
            &[],
            "me@example.com",
        )
        .expect_err("List-Post: NO refuses");
        assert_eq!(
            refused,
            "announce.example.com does not accept posts \
             (List-Post: NO)"
        );

        let not_list = list_recipients(
            &headers(None, ListPost::Absent, &[]),
            "a@example.com",
            &[],
            "me@example.com",
        )
        .expect_err("no list markers");
        assert_eq!(not_list, "not a mailing-list message");
    }

    #[test]
    fn missing_list_post_falls_back_to_reply_all() {
        let delivered = [
            "old-list@example.com".to_string(),
            "me@example.com".to_string(),
            "mara@example.com".to_string(),
            "Old-List@example.com".to_string(),
        ];
        let plan = list_recipients(
            &headers(Some("old.example.com"), ListPost::Absent, &[]),
            "Mara Voss <mara@example.com>",
            &delivered,
            "me@example.com",
        )
        .expect("reply-all fallback");
        assert_eq!(plan.to, ["mara@example.com"]);
        assert_eq!(plan.cc, ["old-list@example.com"]);
        assert_eq!(
            plan.warning.as_deref(),
            Some(
                "old.example.com has no List-Post header; \
                 replying to all 2 recipient(s)"
            )
        );
    }
}
