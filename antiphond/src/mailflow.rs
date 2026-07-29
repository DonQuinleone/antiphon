use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use antiphon_oauth::{TokenStore, refresh};
use antiphon_store::{Op, StoreLayout};
use antiphon_sync::{
    RuleOutcome, SyncAccount, SyncError, SyncProgress, SyncReport,
    apply_rules, replay, sync, write_progress,
};

use crate::accounts::{AccountSet, OauthAccount};
use crate::daemon::{SharedState, lock_set, lock_state};
use crate::notify;
use crate::tokens;

pub(crate) type SharedAccounts =
    std::sync::Arc<std::sync::Mutex<AccountSet>>;

/// How many accounts one pass syncs at once. A slow or large
/// account then holds only its own worker; the rest keep
/// syncing. Each account opens its own IMAP connections, so this
/// bounds the daemon's concurrent connection fan-out across
/// accounts (multiplied, per account, by the engine's own
/// per-folder connection bound).
const MAX_CONCURRENT_SYNC_JOBS: usize = 4;

enum AccountJob<'a> {
    Plain(&'a SyncAccount),
    Oauth(&'a OauthAccount),
}

fn account_jobs(set: &AccountSet) -> Vec<AccountJob<'_>> {
    let plain = set.accounts.iter().map(AccountJob::Plain);
    let oauth = set.oauth.iter().map(AccountJob::Oauth);
    plain.chain(oauth).collect()
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
        let jobs = account_jobs(set);
        crate::pool::run_bounded(
            &jobs,
            MAX_CONCURRENT_SYNC_JOBS,
            |job| self.sync_job(set, job, announce),
        );
        write_progress(&self.layout, &SyncProgress::idle());
        lock_state(&self.state).last_sync_unix = Some(now_unix());
    }

    fn sync_job(
        &self,
        set: &AccountSet,
        job: &AccountJob<'_>,
        announce: bool,
    ) {
        match job {
            AccountJob::Plain(account) => {
                self.sync_one(set, account, announce)
            }
            AccountJob::Oauth(spec) => {
                self.sync_oauth(set, spec, announce)
            }
        }
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
                Some(spec.user.as_str()),
                &refresh,
            );
        }
        tokens::access_token(
            &store,
            &spec.grant_name(),
            &spec.name,
            Some(spec.user.as_str()),
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
        if announce && set.notify.enabled {
            notify::new_mail(account, report, &set.notify);
        }
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
    use antiphon_sync::{Auth, DeliveryRule};

    use super::*;

    fn plain(name: &str) -> SyncAccount {
        SyncAccount {
            name: name.to_owned(),
            host: "imap.example.com".to_owned(),
            port: 993,
            user: "quin@example.com".to_owned(),
            auth: Auth::Password("never-used".to_owned()),
            excluded_folders: Vec::new(),
        }
    }

    fn oauth(name: &str) -> OauthAccount {
        OauthAccount {
            name: name.to_owned(),
            user: "quin@example.com".to_owned(),
            imap_host: "imap.example.com".to_owned(),
            imap_port: 993,
            smtp: None,
            graph: None,
            excluded_folders: Vec::new(),
        }
    }

    fn set_with(
        accounts: Vec<SyncAccount>,
        oauth: Vec<OauthAccount>,
    ) -> AccountSet {
        AccountSet {
            accounts,
            oauth,
            smtp: Vec::new(),
            rules: Vec::new(),
            notify: crate::notify::NotifyPrefs::default(),
        }
    }

    #[test]
    fn account_jobs_cover_every_plain_and_oauth_account() {
        let set = set_with(
            vec![plain("home"), plain("work")],
            vec![oauth("cloud")],
        );
        let jobs = account_jobs(&set);
        assert_eq!(jobs.len(), 3);
        let names: Vec<&str> = jobs
            .iter()
            .map(|job| match job {
                AccountJob::Plain(account) => account.name.as_str(),
                AccountJob::Oauth(spec) => spec.name.as_str(),
            })
            .collect();
        assert_eq!(names, ["home", "work", "cloud"]);
    }

    #[test]
    fn no_accounts_means_no_jobs() {
        assert!(account_jobs(&set_with(Vec::new(), Vec::new())).is_empty());
    }

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
            notify: crate::notify::NotifyPrefs::default(),
        };
        assert_eq!(set.rules_for("work"), &[tagger]);
        assert!(set.rules_for("home").is_empty());
    }
}
