use std::collections::HashSet;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use antiphon_oauth::{TokenStore, refresh};
use antiphon_store::{Op, Outbox, StoreLayout};
use antiphon_sync::{
    DeliveryRule, RuleOutcome, SmtpAccount, SyncAccount, SyncError,
    SyncProgress, SyncReport, apply_rules, replay, send, sync,
    write_progress,
};

use crate::accounts::OauthAccount;
use crate::daemon::{SharedState, lock_state};
use crate::notify;
use crate::tokens;

pub(crate) struct Mailflow {
    pub(crate) layout: StoreLayout,
    pub(crate) accounts: Vec<SyncAccount>,
    pub(crate) oauth: Vec<OauthAccount>,
    pub(crate) smtp: Vec<(String, SmtpAccount)>,
    pub(crate) rules: Vec<(String, Vec<DeliveryRule>)>,
    pub(crate) notify: bool,
    pub(crate) state: SharedState,
}

impl Mailflow {
    pub(crate) fn sync_pass(&self, announce: bool) {
        self.drain_outbox();
        self.drain_drafts();
        self.sync_all(announce);
        self.drain_ops();
    }

    fn sync_all(&self, announce: bool) {
        for account in &self.accounts {
            self.sync_one(account, announce);
        }
        for spec in &self.oauth {
            self.sync_oauth(spec, announce);
        }
        write_progress(&self.layout, &SyncProgress::idle());
        lock_state(&self.state).last_sync_unix = Some(now_unix());
    }

    fn sync_one(&self, account: &SyncAccount, announce: bool) {
        match sync(account, &self.layout) {
            Ok(report) => {
                self.after_sync(&account.name, &report, announce);
            }
            Err(error) => {
                eprintln!(
                    "sync {}: {}",
                    account.name,
                    error_chain(&error)
                );
            }
        }
    }

    /// One AUTHENTICATIONFAILED gets one forced refresh and one
    /// retry; a second failure waits for the next pass.
    fn sync_oauth(&self, spec: &OauthAccount, announce: bool) {
        let token = match self.oauth_token(spec, false) {
            Ok(token) => token,
            Err(message) => {
                eprintln!("{message}");
                return;
            }
        };
        let mut outcome = sync(&spec.sync_account(token), &self.layout);
        if matches!(outcome, Err(SyncError::Login { .. })) {
            eprintln!(
                "sync {}: authentication failed; refreshing the \
                 token and retrying once",
                spec.name
            );
            match self.oauth_token(spec, true) {
                Ok(token) => {
                    outcome =
                        sync(&spec.sync_account(token), &self.layout);
                }
                Err(message) => {
                    eprintln!("{message}");
                    return;
                }
            }
        }
        match outcome {
            Ok(report) => {
                self.after_sync(&spec.name, &report, announce);
            }
            Err(error) => {
                eprintln!(
                    "sync {}: {}",
                    spec.name,
                    error_chain(&error)
                );
            }
        }
    }

    pub(crate) fn oauth_token(
        &self,
        spec: &OauthAccount,
        force_refresh: bool,
    ) -> Result<String, String> {
        let store = TokenStore::open(self.layout.tokens_dir())
            .map_err(|error| {
                format!("{}: token store: {error}", spec.name)
            })?;
        if force_refresh {
            return tokens::refreshed_token(
                &store,
                &spec.grant_name(),
                &spec.name,
                &refresh,
            );
        }
        tokens::access_token(
            &store,
            &spec.grant_name(),
            &spec.name,
            now_unix(),
            &refresh,
        )
    }

    fn after_sync(
        &self,
        account: &str,
        report: &SyncReport,
        announce: bool,
    ) {
        println!(
            "synced {}: {} new, {} updated, {} removed",
            account,
            report.total_new(),
            report.total_updated(),
            report.total_removed(),
        );
        let rules = account_rules(&self.rules, account);
        if !rules.is_empty() {
            let mut state = lock_state(&self.state);
            let outcome = apply_rules(
                account,
                rules,
                &report.delivered(),
                &self.layout,
                &mut state.log,
            );
            drop(state);
            announce_rules(account, outcome);
        }
        if announce && self.notify {
            notify::new_mail(account, report);
        }
    }

    pub(crate) fn drain_outbox(&self) {
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
        let raw = match std::fs::read(&queued.message_path) {
            Ok(raw) => raw,
            Err(error) => {
                eprintln!("outbox {}: {error}", queued.id);
                return;
            }
        };
        if let Err(error) = self.ship(&account, &raw, queued.id) {
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

    /// One message leaves the machine here: through Graph
    /// when the account opted in, else through SMTP.
    fn ship(
        &self,
        account: &str,
        raw: &[u8],
        queued_id: u64,
    ) -> Result<(), String> {
        if let Some(spec) = self.graph_spec(account) {
            let token = self.graph_token(spec)?;
            return crate::graph::send_raw(&token, raw);
        }
        let Some(smtp) = self.smtp_for(account) else {
            return Err(format!(
                "outbox {queued_id}: no smtp account for {account}"
            ));
        };
        send(&smtp, raw).map_err(|error| error.to_string())
    }

    fn graph_spec(&self, account: &str) -> Option<&OauthAccount> {
        self.oauth
            .iter()
            .find(|spec| spec.name == account && spec.graph_send)
    }

    fn graph_token(
        &self,
        spec: &OauthAccount,
    ) -> Result<String, String> {
        let store = TokenStore::open(self.layout.tokens_dir())
            .map_err(|error| {
                format!("{}: token store: {error}", spec.name)
            })?;
        tokens::access_token(
            &store,
            &antiphon_oauth::graph_grant(&spec.name),
            &spec.name,
            now_unix(),
            &refresh,
        )
    }

    fn smtp_for(&self, account: &str) -> Option<SmtpAccount> {
        let stored = self
            .smtp
            .iter()
            .find(|(name, _)| name == account)
            .map(|(_, smtp)| smtp.clone());
        if stored.is_some() {
            return stored;
        }
        let spec = self
            .oauth
            .iter()
            .find(|spec| spec.name == account && spec.smtp.is_some())?;
        let token = match self.oauth_token(spec, false) {
            Ok(token) => token,
            Err(message) => {
                eprintln!("{message}");
                return None;
            }
        };
        spec.smtp_account(token)
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
    /// are resolved (dropped means the server won and the op
    /// is discarded).
    ///
    /// The lock is held only to snapshot and to mark: replay
    /// itself talks to the server and must not block IPC. Ops
    /// appended meanwhile get ids above the snapshot, so the
    /// cursor can never advance over an op replay did not see.
    pub(crate) fn drain_ops(&self) {
        let pending = lock_state(&self.state).log.unsynced();
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
                        "replayed {}: {} synced, {} dropped",
                        account.name,
                        report.synced.len(),
                        report.dropped.len(),
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
        if let Err(error) = lock_state(&self.state).log.mark_synced(id)
        {
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

pub(crate) fn error_chain(error: &dyn std::error::Error) -> String {
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
