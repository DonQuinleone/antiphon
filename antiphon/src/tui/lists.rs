use antiphon_render::{
    ListHeaders, ListReply, Unsubscribe, list_headers, reply_to_list,
    unsubscribe_method,
};

use super::actions::account_of;
use super::app::App;
use super::commands::{Prompt, PromptKind};
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

impl App {
    /// The :unsubscribe command: RFC 8058 one-click asks for
    /// confirmation first; a mailto entry arms a compose; a
    /// web-only entry is displayed, never fetched.
    pub(super) fn unsubscribe_command(&mut self) {
        let Some(message) = self.selected_message() else {
            self.notice = Some("no message selected".to_string());
            return;
        };
        let path = message.path.clone();
        let raw = match std::fs::read(&path) {
            Ok(raw) => raw,
            Err(error) => {
                self.notice = Some(format!(
                    "cannot read {}: {error}",
                    path.display()
                ));
                return;
            }
        };
        let headers = list_headers(&raw);
        let list = list_name(&headers).to_string();
        match unsubscribe_method(&headers) {
            Unsubscribe::OneClick { url } => {
                self.confirm_one_click(url, list)
            }
            Unsubscribe::Mailto(mailto) => {
                self.pending_unsubscribe =
                    Some((account_of(&path), mailto))
            }
            Unsubscribe::Browse { url } => {
                self.notice =
                    Some(format!("open to unsubscribe: {url}"))
            }
            Unsubscribe::None => {
                self.notice = Some(
                    "no unsubscribe header on this message".to_string(),
                )
            }
        }
    }

    fn confirm_one_click(&mut self, url: String, list: String) {
        self.pending_one_click = Some(url);
        self.prompt = Some(Prompt {
            kind: PromptKind::ConfirmUnsubscribe,
            buffer: list,
        });
    }

    pub(super) fn confirming_unsubscribe(&self) -> bool {
        self.prompt.as_ref().is_some_and(|prompt| {
            prompt.kind == PromptKind::ConfirmUnsubscribe
        })
    }

    pub(super) fn confirm_unsubscribe(&mut self, confirmed: bool) {
        self.prompt = None;
        let Some(url) = self.pending_one_click.take() else {
            return;
        };
        if !confirmed {
            self.notice = Some("unsubscribe cancelled".to_string());
            return;
        }
        self.pending_unsub_post = Some(url);
    }
}

#[cfg(test)]
mod tests {
    use antiphon_render::ListPost;

    use super::super::testkit::TempDir;
    use super::super::testkit::app_with_messages;
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

    fn app_with_message(
        extra_headers: &str,
    ) -> (TempDir, super::super::app::App) {
        let dir = TempDir::new();
        let path = dir.path.join("msg.eml");
        std::fs::write(
            &path,
            format!(
                "From: news@example.com\r\n\
                 To: me@example.com\r\n\
                 Subject: weekly\r\n\
                 {extra_headers}\
                 \r\n\
                 body\r\n"
            ),
        )
        .unwrap();
        let mut app = app_with_messages(1);
        app.messages[0].path = path;
        (dir, app)
    }

    #[test]
    fn one_click_confirms_by_name_then_queues() {
        let (_dir, mut app) = app_with_message(
            "List-Id: <news.example.com>\r\n\
             List-Unsubscribe: <https://example.com/u/1>\r\n\
             List-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n",
        );
        app.run_command("unsubscribe");
        let prompt = app.prompt.clone().expect("a confirmation");
        assert_eq!(prompt.kind, PromptKind::ConfirmUnsubscribe);
        assert_eq!(prompt.buffer, "news.example.com");
        assert!(app.confirming_unsubscribe());

        app.confirm_unsubscribe(true);
        assert!(app.prompt.is_none());
        assert!(app.pending_one_click.is_none());
        assert_eq!(
            app.pending_unsub_post.as_deref(),
            Some("https://example.com/u/1")
        );
    }

    #[test]
    fn declining_the_confirmation_queues_nothing() {
        let (_dir, mut app) = app_with_message(
            "List-Id: <news.example.com>\r\n\
             List-Unsubscribe: <https://example.com/u/1>\r\n\
             List-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n",
        );
        app.run_command("unsubscribe");
        app.confirm_unsubscribe(false);
        assert!(app.prompt.is_none());
        assert!(app.pending_one_click.is_none());
        assert_eq!(
            app.notice.as_deref(),
            Some("unsubscribe cancelled")
        );
    }

    #[test]
    fn mailto_unsubscribe_arms_a_compose() {
        let (_dir, mut app) = app_with_message(
            "List-Unsubscribe: <mailto:leave@example.com\
             ?subject=unsubscribe>\r\n",
        );
        app.run_command("unsubscribe");
        assert!(app.prompt.is_none());
        let (_, mailto) =
            app.pending_unsubscribe.clone().expect("armed compose");
        assert_eq!(mailto.address, "leave@example.com");
        assert_eq!(mailto.subject.as_deref(), Some("unsubscribe"));
    }

    #[test]
    fn web_only_unsubscribe_is_shown_not_fetched() {
        let (_dir, mut app) = app_with_message(
            "List-Unsubscribe: <https://example.com/u/1>\r\n",
        );
        app.run_command("unsubscribe");
        assert_eq!(
            app.notice.as_deref(),
            Some("open to unsubscribe: https://example.com/u/1")
        );
        assert!(app.pending_one_click.is_none());
    }

    #[test]
    fn no_unsubscribe_header_says_so() {
        let (_dir, mut app) = app_with_message("");
        app.run_command("unsubscribe");
        assert_eq!(
            app.notice.as_deref(),
            Some("no unsubscribe header on this message")
        );
    }
}
