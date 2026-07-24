mod engine;
mod error;
mod folders;
mod maildir;
mod replay;
mod report;
mod rules;
mod session;
mod smtp;
mod state;

pub use engine::{SyncAccount, sync};
pub use error::SyncError;
pub use replay::{ReplayReport, replay};
pub use report::{FolderReport, SyncReport};
pub use rules::{DeliveryRule, RuleOutcome, apply_rules};
pub use smtp::{SmtpAccount, send};
