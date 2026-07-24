mod auth;
mod engine;
mod error;
mod folders;
mod maildir;
mod progress;
mod replay;
mod report;
mod rules;
mod session;
mod smtp;
mod state;
mod tagging;

pub use auth::Auth;
pub use engine::{SyncAccount, sync};
pub use error::SyncError;
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
