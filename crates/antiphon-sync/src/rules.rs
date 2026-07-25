use std::path::{Path, PathBuf};
use std::process::Command;

use antiphon_store::{
    OpKind, OpLog, SearchIndex, StoreLayout, apply_op, id_query,
};
use mail_parser::{HeaderName, MessageParser};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeliveryRule {
    pub match_list: Option<String>,
    pub match_sender: Option<String>,
    pub move_to: Option<String>,
    pub tag: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RuleOutcome {
    pub tagged: usize,
    pub moved: usize,
}

struct Headers {
    message_id: Option<String>,
    list_id: Option<String>,
    sender: Option<String>,
}

/// Applies an account's delivery rules to the messages a sync
/// pass just delivered. Every failure is logged and skipped: a
/// rule must never fail the pass that carried it.
pub fn apply_rules(
    account: &str,
    rules: &[DeliveryRule],
    delivered: &[PathBuf],
    layout: &StoreLayout,
    log: &mut OpLog,
) -> RuleOutcome {
    let mut pass = RulePass {
        account,
        layout,
        log,
        outcome: RuleOutcome::default(),
    };
    for path in delivered {
        pass.run(rules, path);
    }
    pass.outcome
}

fn first_match<'r>(
    rules: &'r [DeliveryRule],
    list_id: Option<&str>,
    sender: Option<&str>,
) -> Option<&'r DeliveryRule> {
    rules.iter().find(|rule| matches(rule, list_id, sender))
}

fn matches(
    rule: &DeliveryRule,
    list_id: Option<&str>,
    sender: Option<&str>,
) -> bool {
    if rule.match_list.is_none() && rule.match_sender.is_none() {
        return false;
    }
    criterion_met(rule.match_list.as_deref(), list_id)
        && criterion_met(rule.match_sender.as_deref(), sender)
}

fn criterion_met(wanted: Option<&str>, header: Option<&str>) -> bool {
    let Some(needle) = wanted else {
        return true;
    };
    let Some(haystack) = header else {
        return false;
    };
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

struct RulePass<'a> {
    account: &'a str,
    layout: &'a StoreLayout,
    log: &'a mut OpLog,
    outcome: RuleOutcome,
}

impl RulePass<'_> {
    fn run(&mut self, rules: &[DeliveryRule], path: &Path) {
        let headers = match read_headers(path) {
            Ok(headers) => headers,
            Err(detail) => {
                self.complain(path, &detail);
                return;
            }
        };
        let Some(rule) = first_match(
            rules,
            headers.list_id.as_deref(),
            headers.sender.as_deref(),
        ) else {
            return;
        };
        let Some(message_id) = headers.message_id.as_deref() else {
            self.complain(path, "no Message-ID, rule skipped");
            return;
        };
        if let Some(tag) = rule.tag.as_deref() {
            match self.add_tag(message_id, tag) {
                Ok(()) => self.outcome.tagged += 1,
                Err(detail) => self.complain(path, &detail),
            }
        }
        let Some(folder) = rule.move_to.as_deref() else {
            return;
        };
        match self.move_message(message_id, folder) {
            Ok(()) => self.outcome.moved += 1,
            Err(detail) => self.complain(path, &detail),
        }
    }

    fn add_tag(
        &self,
        message_id: &str,
        tag: &str,
    ) -> Result<(), String> {
        let output = Command::new("notmuch")
            .args(["tag", &format!("+{tag}"), "--"])
            .arg(id_query(message_id))
            .env("NOTMUCH_CONFIG", self.layout.notmuch_config_path())
            .output()
            .map_err(|error| error.to_string())?;
        if output.status.success() {
            return Ok(());
        }
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }

    /// A rule move is a client move: append to the oplog, apply
    /// locally, and let the daemon's usual drain replay it to
    /// the server. One mutation path, whoever asks.
    fn move_message(
        &mut self,
        message_id: &str,
        folder: &str,
    ) -> Result<(), String> {
        let op = self
            .log
            .append(
                self.account,
                message_id,
                OpKind::Move {
                    to_folder: folder.to_owned(),
                    // Rules act on fresh inbox deliveries, so
                    // the source is always the account root.
                    from_folder: Some(String::new()),
                },
            )
            .map_err(|error| error.to_string())?;
        let index = SearchIndex::open(self.layout)
            .map_err(|error| error.to_string())?;
        apply_op(self.layout, &index, &op)
            .map_err(|error| error.to_string())?;
        self.log
            .mark_applied(op.id)
            .map_err(|error| error.to_string())
    }

    fn complain(&self, path: &Path, detail: &str) {
        eprintln!(
            "rules {}: {}: {detail}",
            self.account,
            path.display()
        );
    }
}

fn read_headers(path: &Path) -> Result<Headers, String> {
    let raw = std::fs::read(path).map_err(|error| error.to_string())?;
    let Some(message) = MessageParser::default().parse(&raw) else {
        return Err(String::from("unparseable message"));
    };
    let sender = message
        .from()
        .and_then(|address| address.first())
        .and_then(|addr| addr.address())
        .map(str::to_owned);
    let list_id = message
        .header_raw(HeaderName::ListId)
        .map(|value| value.trim().to_owned());
    Ok(Headers {
        message_id: message.message_id().map(str::to_owned),
        list_id,
        sender,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(
        match_list: Option<&str>,
        match_sender: Option<&str>,
    ) -> DeliveryRule {
        DeliveryRule {
            match_list: match_list.map(str::to_owned),
            match_sender: match_sender.map(str::to_owned),
            move_to: None,
            tag: None,
        }
    }

    struct MatchCase {
        name: &'static str,
        match_list: Option<&'static str>,
        match_sender: Option<&'static str>,
        list_id: Option<&'static str>,
        sender: Option<&'static str>,
        matched: bool,
    }

    const LIST: &str = "aerc-devel <~sircmpwn/aerc-devel.lists.sr.ht>";
    const SENDER: &str = "mara@example.com";

    const MATCH_CASES: [MatchCase; 9] = [
        MatchCase {
            name: "list substring matches",
            match_list: Some("~sircmpwn/aerc-devel"),
            match_sender: None,
            list_id: Some(LIST),
            sender: Some(SENDER),
            matched: true,
        },
        MatchCase {
            name: "sender substring matches",
            match_list: None,
            match_sender: Some("@example.com"),
            list_id: None,
            sender: Some(SENDER),
            matched: true,
        },
        MatchCase {
            name: "both set need both to hold",
            match_list: Some("aerc-devel"),
            match_sender: Some("mara@"),
            list_id: Some(LIST),
            sender: Some(SENDER),
            matched: true,
        },
        MatchCase {
            name: "both set fails on one miss",
            match_list: Some("aerc-devel"),
            match_sender: Some("nobody@"),
            list_id: Some(LIST),
            sender: Some(SENDER),
            matched: false,
        },
        MatchCase {
            name: "neither set never matches",
            match_list: None,
            match_sender: None,
            list_id: Some(LIST),
            sender: Some(SENDER),
            matched: false,
        },
        MatchCase {
            name: "list criterion needs the header",
            match_list: Some("aerc-devel"),
            match_sender: None,
            list_id: None,
            sender: Some(SENDER),
            matched: false,
        },
        MatchCase {
            name: "sender criterion needs the header",
            match_list: None,
            match_sender: Some("mara@"),
            list_id: Some(LIST),
            sender: None,
            matched: false,
        },
        MatchCase {
            name: "list match ignores case",
            match_list: Some("~SIRCMPWN/AERC-Devel"),
            match_sender: None,
            list_id: Some(LIST),
            sender: None,
            matched: true,
        },
        MatchCase {
            name: "sender match ignores case",
            match_list: None,
            match_sender: Some("MARA@Example.COM"),
            list_id: None,
            sender: Some(SENDER),
            matched: true,
        },
    ];

    #[test]
    fn matcher_follows_the_semantics_table() {
        for case in MATCH_CASES {
            let candidate = rule(case.match_list, case.match_sender);
            assert_eq!(
                matches(&candidate, case.list_id, case.sender),
                case.matched,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn first_matching_rule_wins() {
        let rules = [
            rule(Some("no-such-list"), None),
            rule(None, Some("mara@")),
            rule(None, Some("@example.com")),
        ];
        let winner =
            first_match(&rules, Some(LIST), Some(SENDER)).unwrap();
        assert_eq!(winner, &rules[1]);
        assert!(
            first_match(&rules, None, Some("other@else.net")).is_none()
        );
    }
}
