use std::collections::HashSet;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use antiphon_oauth::{TokenStore, refresh};
use antiphon_store::{Op, Outbox, StoreLayout};
use antiphon_sync::{
    RuleOutcome, SmtpAccount, SyncAccount, SyncError, SyncProgress,
    SyncReport, append_sent, apply_rules, replay, send, sync,
    write_progress,
};

use crate::accounts::{AccountSet, OauthAccount};
use crate::daemon::{SharedState, lock_set, lock_state};
use crate::notify;
use crate::tokens;

pub(crate) type SharedAccounts =
    std::sync::Arc<std::sync::Mutex<AccountSet>>;

/// A send failure with its retry verdict attached: permanent
/// means no later drain can succeed without the user changing
/// something, so the message leaves the retry queue.
pub(crate) struct ShipError {
    pub(crate) detail: String,
    pub(crate) permanent: bool,
}

impl ShipError {
    pub(crate) fn transient(
        detail: impl std::fmt::Display,
    ) -> ShipError {
        ShipError {
            detail: detail.to_string(),
            permanent: false,
        }
    }
}

/// A rejected message (5xx) or one the server can never parse
/// will fail identically on every retry; connection trouble
/// and 4xx throttling deserve another pass.
fn smtp_permanent(error: &SyncError) -> bool {
    match error {
        SyncError::Smtp { source, .. } => source.is_permanent(),
        SyncError::SmtpMessage { .. } => true,
        _ => false,
    }
}

pub(crate) struct Mailflow {
    pub(crate) layout: StoreLayout,
    pub(crate) set: SharedAccounts,
    pub(crate) state: SharedState,
}

impl Mailflow {
    /// One job runs against one snapshot: a reload landing
    /// mid-pass takes effect on the next job, never halfway
    /// through this one.
    pub(crate) fn snapshot(&self) -> AccountSet {
        lock_set(&self.set).clone()
    }

    pub(crate) fn sync_pass(&self, announce: bool) {
        let set = self.snapshot();
        self.drain_outbox_of(&set);
        self.drain_drafts_of(&set);
        self.sync_all(&set, announce);
        self.drain_ops_of(&set);
    }

    fn sync_all(&self, set: &AccountSet, announce: bool) {
        for account in &set.accounts {
            self.sync_one(set, account, announce);
        }
        for spec in &set.oauth {
            self.sync_oauth(set, spec, announce);
        }
        write_progress(&self.layout, &SyncProgress::idle());
        lock_state(&self.state).last_sync_unix = Some(now_unix());
    }

    fn sync_one(
        &self,
        set: &AccountSet,
        account: &SyncAccount,
        announce: bool,
    ) {
        match sync(account, &self.layout) {
            Ok(report) => {
                self.after_sync(set, &account.name, &report, announce);
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
    fn sync_oauth(
        &self,
        set: &AccountSet,
        spec: &OauthAccount,
        announce: bool,
    ) {
        let token = match self.oauth_token(spec, false) {
            Ok(token) => token,
            Err(message) => {
                self.mark_auth_failure(&spec.name);
                eprintln!("{message}");
                return;
            }
        };
        self.clear_auth_failure(&spec.name);
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
                    self.mark_auth_failure(&spec.name);
                    eprintln!("{message}");
                    return;
                }
            }
        }
        if matches!(outcome, Err(SyncError::Login { .. })) {
            self.mark_auth_failure(&spec.name);
        }
        match outcome {
            Ok(report) => {
                self.after_sync(set, &spec.name, &report, announce);
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

    fn mark_auth_failure(&self, account: &str) {
        lock_state(&self.state)
            .auth_failures
            .insert(account.to_string());
    }

    fn clear_auth_failure(&self, account: &str) {
        lock_state(&self.state).auth_failures.remove(account);
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
        set: &AccountSet,
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
        for error in &report.errors {
            eprintln!("sync {account}: {error}");
        }
        let rules = set.rules_for(account);
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
        if announce && set.notify {
            notify::new_mail(account, report);
        }
    }

    pub(crate) fn drain_outbox(&self) {
        self.drain_outbox_of(&self.snapshot());
    }

    fn drain_outbox_of(&self, set: &AccountSet) {
        let outbox = Outbox::open(&self.layout);
        let pending = match outbox.pending() {
            Ok(pending) => pending,
            Err(error) => {
                eprintln!("outbox: {error}");
                return;
            }
        };
        for queued in pending {
            self.send_queued(set, &outbox, queued);
        }
    }

    fn send_queued(
        &self,
        set: &AccountSet,
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
        if let Err(error) = self.ship(set, &queued.envelope, &raw) {
            self.settle_failure(outbox, queued.id, &error);
            return;
        }
        if let Err(error) = self.file_sent(&account, &raw) {
            eprintln!("sent copy {}: {error}", queued.id);
        }
        self.file_sent_on_server(set, &account, &raw);
        if let Err(error) = outbox.remove(queued.id) {
            eprintln!("outbox {}: {error}", queued.id);
            return;
        }
        println!("sent outbox message {}", queued.id);
    }

    /// A transient failure stays queued for the next drain; a
    /// permanent one moves aside into outbox/dead, so a message
    /// no server will ever take stops retrying forever.
    fn settle_failure(
        &self,
        outbox: &Outbox,
        queued_id: u64,
        error: &ShipError,
    ) {
        if !error.permanent {
            eprintln!("send {queued_id}: {}; will retry", error.detail);
            return;
        }
        match outbox.reject(queued_id) {
            Ok(()) => eprintln!(
                "send {queued_id}: {}; giving up, message moved \
                 to outbox/dead",
                error.detail
            ),
            Err(reject_error) => eprintln!(
                "send {queued_id}: {}; also failed to set it \
                 aside: {reject_error}",
                error.detail
            ),
        }
    }

    /// One message leaves the machine here: through Graph
    /// when the account opted in, else through SMTP. The queued
    /// envelope carries the true recipient list, Bcc included.
    fn ship(
        &self,
        set: &AccountSet,
        envelope: &antiphon_store::Envelope,
        raw: &[u8],
    ) -> Result<(), ShipError> {
        let account = &envelope.account;
        if let Some(spec) = set.graph_spec(account) {
            let token =
                self.graph_token(spec).map_err(ShipError::transient)?;
            let upload = crate::graph::with_envelope_bcc(
                raw,
                &envelope.recipients,
            );
            return crate::graph::send_raw(&token, &upload);
        }
        let Some(smtp) = self.smtp_for(set, account) else {
            return Err(ShipError {
                detail: format!("no smtp account for {account}"),
                permanent: true,
            });
        };
        send(&smtp, &envelope.from, &envelope.recipients, raw).map_err(
            |error| ShipError {
                permanent: smtp_permanent(&error),
                detail: error_chain(&error),
            },
        )
    }

    /// Best-effort mirror of the local sent twin into the
    /// server's own sent folder; Graph accounts skip it since
    /// Graph files Sent Items itself.
    fn file_sent_on_server(
        &self,
        set: &AccountSet,
        account: &str,
        raw: &[u8],
    ) {
        if set.graph_spec(account).is_some() {
            return;
        }
        let Some(sync_account) = self.imap_account(set, account) else {
            return;
        };
        match append_sent(&sync_account, raw) {
            Ok(folder) => {
                println!("sent copy filed on the server ({folder})")
            }
            Err(error) => {
                eprintln!("server sent copy {account}: {error}")
            }
        }
    }

    fn imap_account(
        &self,
        set: &AccountSet,
        account: &str,
    ) -> Option<SyncAccount> {
        if let Some(found) = set
            .accounts
            .iter()
            .find(|candidate| candidate.name == account)
        {
            return Some(found.clone());
        }
        let spec = set
            .oauth
            .iter()
            .find(|candidate| candidate.name == account)?;
        let token = match self.oauth_token(spec, false) {
            Ok(token) => token,
            Err(message) => {
                eprintln!("{message}");
                return None;
            }
        };
        Some(spec.sync_account(token))
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

    fn smtp_for(
        &self,
        set: &AccountSet,
        account: &str,
    ) -> Option<SmtpAccount> {
        let stored = set
            .smtp
            .iter()
            .find(|(name, _)| name == account)
            .map(|(_, smtp)| smtp.clone());
        if stored.is_some() {
            return stored;
        }
        let spec = set
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
        self.drain_ops_of(&self.snapshot());
    }

    fn drain_ops_of(&self, set: &AccountSet) {
        let pending = lock_state(&self.state).log.unsynced();
        if pending.is_empty() {
            return;
        }
        let mut resolved = HashSet::new();
        for account in &set.accounts {
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
    use antiphon_sync::DeliveryRule;

    use super::*;

    #[test]
    fn rules_lookup_picks_only_the_named_account() {
        let tagger = DeliveryRule {
            match_list: None,
            match_sender: Some("mara@example.com".to_owned()),
            move_to: None,
            tag: Some("mara".to_owned()),
        };
        let set = AccountSet {
            accounts: Vec::new(),
            oauth: Vec::new(),
            smtp: Vec::new(),
            rules: vec![("work".to_owned(), vec![tagger.clone()])],
            notify: false,
        };
        assert_eq!(set.rules_for("work"), &[tagger]);
        assert!(set.rules_for("home").is_empty());
    }
}
