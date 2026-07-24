use std::collections::HashSet;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use antiphon_ipc::Response;
use antiphon_store::{Op, Outbox};
use antiphon_sync::{
    DeliveryRule, RuleOutcome, apply_rules, replay, send, sync,
};

use crate::daemon::Daemon;
use crate::notify;

impl Daemon {
    pub(crate) fn sync_pass(&mut self, announce: bool) -> usize {
        self.drain_outbox();
        let failures = self.sync_all(announce);
        self.drain_ops();
        failures
    }

    pub(crate) fn sync_now(&mut self) -> Response {
        let failures = self.sync_pass(true);
        if failures == 0 {
            return Response::Ack;
        }
        Response::Error(format!(
            "sync failed for {failures} of {} accounts",
            self.accounts.len()
        ))
    }

    fn sync_all(&mut self, announce: bool) -> usize {
        let mut failures = 0;
        for account in &self.accounts {
            match sync(account, &self.layout) {
                Ok(report) => {
                    println!(
                        "synced {}: {} new, {} updated",
                        account.name,
                        report.total_new(),
                        report.total_updated(),
                    );
                    let rules =
                        account_rules(&self.rules, &account.name);
                    if !rules.is_empty() {
                        let outcome = apply_rules(
                            &account.name,
                            rules,
                            &report.delivered(),
                            &self.layout,
                            &mut self.log,
                        );
                        announce_rules(&account.name, outcome);
                    }
                    if announce && self.notify {
                        notify::new_mail(&account.name, &report);
                    }
                }
                Err(error) => {
                    failures += 1;
                    eprintln!(
                        "sync {}: {}",
                        account.name,
                        error_chain(&error)
                    );
                }
            }
        }
        self.last_sync_unix = Some(now_unix());
        failures
    }

    fn drain_outbox(&mut self) {
        let outbox = Outbox::open(&self.layout);
        let pending = match outbox.pending() {
            Ok(pending) => pending,
            Err(error) => {
                eprintln!("outbox: {error}");
                return;
            }
        };
        for queued in pending {
            self.send_queued(&outbox, queued);
        }
    }

    fn send_queued(
        &self,
        outbox: &Outbox,
        queued: antiphon_store::QueuedMessage,
    ) {
        let account = queued.envelope.account.clone();
        let Some((_, smtp)) =
            self.smtp.iter().find(|(name, _)| *name == account)
        else {
            eprintln!(
                "outbox {}: no smtp account for {account}",
                queued.id
            );
            return;
        };
        let raw = match std::fs::read(&queued.message_path) {
            Ok(raw) => raw,
            Err(error) => {
                eprintln!("outbox {}: {error}", queued.id);
                return;
            }
        };
        if let Err(error) = send(smtp, &raw) {
            eprintln!("send {}: {error}", queued.id);
            return;
        }
        if let Err(error) = self.file_sent(&account, &raw) {
            eprintln!("sent copy {}: {error}", queued.id);
        }
        if let Err(error) = outbox.remove(queued.id) {
            eprintln!("outbox {}: {error}", queued.id);
            return;
        }
        println!("sent outbox message {}", queued.id);
    }

    fn file_sent(
        &self,
        account: &str,
        raw: &[u8],
    ) -> std::io::Result<()> {
        let sent =
            self.layout.account_maildir(account).join("sent/cur");
        std::fs::create_dir_all(&sent)?;
        let name = format!(
            "{}.P{}.antiphon:2,S",
            now_unix(),
            std::process::id()
        );
        std::fs::write(sent.join(name), raw)?;
        let status = Command::new("notmuch")
            .arg("new")
            .env("NOTMUCH_CONFIG", self.layout.notmuch_config_path())
            .output()?;
        if !status.status.success() {
            eprintln!("notmuch new failed after sent copy");
        }
        // new.tags stamps every indexed message unread+inbox; a
        // sent copy is neither.
        let retag = Command::new("notmuch")
            .args([
                "tag",
                "+sent",
                "-inbox",
                "-unread",
                "--",
                &format!("path:{account}/sent/**"),
            ])
            .env("NOTMUCH_CONFIG", self.layout.notmuch_config_path())
            .output()?;
        if !retag.status.success() {
            eprintln!("retagging the sent copy failed");
        }
        Ok(())
    }

    /// Replays unsynced ops per account and advances the synced
    /// cursor over the resolved prefix. Synced and dropped ops
    /// are resolved (dropped means the server won and the op is
    /// discarded); unsupported ops stay pending and hold the
    /// cursor, since mark_synced covers everything below it.
    pub(crate) fn drain_ops(&mut self) {
        let pending = self.log.unsynced();
        if pending.is_empty() {
            return;
        }
        let mut resolved = HashSet::new();
        for account in &self.accounts {
            let ops: Vec<Op> = pending
                .iter()
                .filter(|op| op.account == account.name)
                .cloned()
                .collect();
            if ops.is_empty() {
                continue;
            }
            match replay(account, &self.layout, &ops) {
                Ok(report) => {
                    println!(
                        "replayed {}: {} synced, {} dropped, \
                         {} deferred",
                        account.name,
                        report.synced.len(),
                        report.dropped.len(),
                        report.unsupported.len(),
                    );
                    if !report.dropped.is_empty() {
                        eprintln!(
                            "replay {}: server wins, dropped \
                             ops {:?}",
                            account.name, report.dropped
                        );
                    }
                    resolved.extend(report.synced);
                    resolved.extend(report.dropped);
                }
                Err(error) => {
                    eprintln!("replay {}: {error}", account.name);
                }
            }
        }
        let mut cursor = None;
        for op in &pending {
            if !resolved.contains(&op.id) {
                break;
            }
            cursor = Some(op.id);
        }
        let Some(id) = cursor else {
            return;
        };
        if let Err(error) = self.log.mark_synced(id) {
            eprintln!("oplog: {error}");
        }
    }
}

fn account_rules<'a>(
    rules: &'a [(String, Vec<DeliveryRule>)],
    account: &str,
) -> &'a [DeliveryRule] {
    rules
        .iter()
        .find(|(name, _)| name == account)
        .map(|(_, rules)| rules.as_slice())
        .unwrap_or(&[])
}

fn announce_rules(account: &str, outcome: RuleOutcome) {
    if outcome.tagged == 0 && outcome.moved == 0 {
        return;
    }
    println!(
        "rules {account}: {} tagged, {} moved",
        outcome.tagged, outcome.moved
    );
}

fn error_chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

pub(crate) fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_rules_picks_only_the_named_account() {
        let tagger = DeliveryRule {
            match_list: None,
            match_sender: Some("mara@example.com".to_owned()),
            move_to: None,
            tag: Some("mara".to_owned()),
        };
        let rules = vec![("work".to_owned(), vec![tagger.clone()])];
        assert_eq!(account_rules(&rules, "work"), &[tagger]);
        assert!(account_rules(&rules, "home").is_empty());
        assert!(account_rules(&[], "work").is_empty());
    }
}
