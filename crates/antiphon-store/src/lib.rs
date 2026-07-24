//! Maildir, notmuch and oplog store access.
//!
//! This crate owns the store directory layout inside the vault
//! and the read-only notmuch search path. Writes to the
//! Maildir and the index belong to antiphond (DESIGN.md
//! section 2); nothing here mutates mail.

pub mod layout;
pub mod search;

pub use layout::StoreLayout;
pub use search::{MessageSummary, SearchError, SearchIndex};
