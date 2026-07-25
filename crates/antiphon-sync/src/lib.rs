mod auth;
mod drafts;
mod engine;
mod error;
mod folders;
mod idle;
mod maildir;
mod progress;
mod reconcile;
mod replay;
mod report;
mod rules;
mod session;
mod smtp;
mod state;
mod tagging;

pub use auth::Auth;
pub use drafts::{DraftPush, push_drafts};
pub use engine::{SyncAccount, sync};
pub use error::SyncError;
pub use idle::{IdleSession, IdleWait};
pub use progress::{
    SyncProgress, SyncState, read_progress, write_progress,
};
pub use replay::{ReplayReport, replay};
pub use report::{FolderReport, SyncReport};
pub use rules::{DeliveryRule, RuleOutcome, apply_rules};
pub use smtp::{SmtpAccount, send};

pub fn test_retag(
    config: &std::path::Path,
    account: &str,
) -> Result<(), SyncError> {
    tagging::retag_folders(config, account)
}
