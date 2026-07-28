use antiphon_store::DraftSpool;
use antiphon_sync::{DraftPush, SyncError, push_drafts};

use crate::accounts::{AccountSet, OauthAccount};
use crate::mailflow::{Mailflow, error_chain};

impl Mailflow {
    /// Files spooled drafts per account. Accounts with nothing
    /// spooled are skipped before any connection or token
    /// refresh; anything that cannot be filed stays spooled
    /// for the next pass.
    pub(crate) fn drain_drafts(&self) {
        self.drain_drafts_of(&self.snapshot());
    }

    pub(crate) fn drain_drafts_of(&self, set: &AccountSet) {
        let spool = DraftSpool::open(&self.layout);
        for account in &set.accounts {
            if !has_pending(&spool, &account.name) {
                continue;
            }
            announce(&account.name, push_drafts(account, &self.layout));
        }
        for spec in &set.oauth {
            self.drain_oauth_drafts(&spool, spec);
        }
    }

    fn drain_oauth_drafts(
        &self,
        spool: &DraftSpool,
        spec: &OauthAccount,
    ) {
        if !has_pending(spool, &spec.name) {
            return;
        }
        let token = match self.oauth_token(spec, false) {
            Ok(token) => token,
            Err(message) => {
                eprintln!("{message}");
                return;
            }
        };
        let account = spec.sync_account(token);
        announce(&spec.name, push_drafts(&account, &self.layout));
    }
}

fn has_pending(spool: &DraftSpool, account: &str) -> bool {
    match spool.pending_for(account) {
        Ok(pending) => !pending.is_empty(),
        Err(error) => {
            eprintln!("draft spool: {error}");
            false
        }
    }
}

fn announce(account: &str, outcome: Result<DraftPush, SyncError>) {
    match outcome {
        Ok(push) => println!("{}", describe(account, &push)),
        Err(error) => {
            eprintln!("drafts {account}: {}", error_chain(&error));
        }
    }
}

fn describe(account: &str, push: &DraftPush) -> String {
    match &push.folder {
        None => format!(
            "drafts {account}: no server drafts folder; \
             {} left spooled",
            push.left
        ),
        Some(folder) => format!(
            "drafts {account}: filed {} into {folder}",
            push.filed
        ),
    }
}

#[cfg(test)]
mod tests {
    use antiphon_store::{DraftEnvelope, StoreLayout};

    use super::*;

    #[test]
    fn pending_is_per_account_and_calm_on_a_fresh_store() {
        let dir = tempfile::tempdir().unwrap();
        let layout = StoreLayout::new(dir.path().join("store"));
        let spool = DraftSpool::open(&layout);
        assert!(!has_pending(&spool, "personal"));
        spool
            .enqueue(
                &DraftEnvelope {
                    account: "personal".to_owned(),
                },
                b"Subject: kept",
            )
            .unwrap();
        assert!(has_pending(&spool, "personal"));
        assert!(!has_pending(&spool, "work"));
    }

    #[test]
    fn announcements_name_the_folder_or_the_missing_one() {
        let filed = DraftPush {
            filed: 2,
            left: 0,
            folder: Some("Drafts".to_owned()),
        };
        assert_eq!(
            describe("personal", &filed),
            "drafts personal: filed 2 into Drafts"
        );
        let held = DraftPush {
            filed: 0,
            left: 3,
            folder: None,
        };
        assert_eq!(
            describe("personal", &held),
            "drafts personal: no server drafts folder; \
             3 left spooled"
        );
    }
}
