use std::borrow::Cow;
use std::fmt;
use std::path::{Path, PathBuf};

use notmuch::{Database, DatabaseMode, Message, Sort};

use crate::layout::StoreLayout;
use crate::scope::{Scope, ScopeError, scoped_query};

const UNREAD_TAG: &str = "unread";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageSummary {
    pub id: String,
    pub thread_id: String,
    pub subject: String,
    pub from: String,
    pub to: String,
    pub date_unix: i64,
    pub tags: Vec<String>,
    pub unread: bool,
    pub path: PathBuf,
    /// Every file notmuch holds for this message-id. A message
    /// delivered to two synced accounts at once (sent from one,
    /// Bcc'd to another) has one file per account, so callers can
    /// act on the copy in the account being viewed rather than
    /// whichever `path` notmuch happened to return first.
    pub paths: Vec<PathBuf>,
    /// The message this one answers, as a bare Message-ID (no
    /// angle brackets, matching `id`); None when it opens a
    /// thread. Drives the reply tree.
    pub in_reply_to: Option<String>,
    /// The ancestor chain from References, oldest first, each a
    /// bare Message-ID.
    pub references: Vec<String>,
}

#[derive(Debug)]
pub enum SearchError {
    Open {
        path: PathBuf,
        source: notmuch::Error,
    },
    Query {
        query: String,
        source: notmuch::Error,
    },
    Scope {
        source: ScopeError,
    },
}

impl fmt::Display for SearchError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open { path, source } => write!(
                out,
                "opening notmuch index at {}: {source}",
                path.display()
            ),
            Self::Query { query, source } => {
                write!(out, "notmuch query `{query}`: {source}")
            }
            Self::Scope { source } => {
                write!(out, "building scoped query: {source}")
            }
        }
    }
}

impl std::error::Error for SearchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Open { source, .. } => Some(source),
            Self::Query { source, .. } => Some(source),
            Self::Scope { source } => Some(source),
        }
    }
}

pub struct SearchIndex {
    db: Database,
    maildir_root: PathBuf,
}

impl SearchIndex {
    pub fn open(layout: &StoreLayout) -> Result<Self, SearchError> {
        let path = layout.notmuch_dir();
        // The store's own config supplies mail_root, so message
        // filenames resolve under maildir/ for every opener;
        // None here would read the user's configuration
        // instead.
        let db = Database::open_with_config(
            Some(&path),
            DatabaseMode::ReadOnly,
            Some(layout.notmuch_config_path()),
            None,
        )
        .map_err(|source| SearchError::Open { path, source })?;
        Ok(Self {
            db,
            maildir_root: layout.maildir_root(),
        })
    }

    pub fn count(&self, query: &str) -> Result<u32, SearchError> {
        let wrap = |source| SearchError::Query {
            query: query.to_owned(),
            source,
        };
        let parsed = self.db.create_query(query).map_err(wrap)?;
        parsed.count_messages().map_err(wrap)
    }

    /// Finds the message's current file by notmuch id. The
    /// index can hold several filenames for one id and lag
    /// behind renames, so only a path that still exists counts.
    pub fn locate(
        &self,
        message_id: &str,
    ) -> Result<Option<PathBuf>, SearchError> {
        let hits = self.query(&id_query(message_id), None)?;
        Ok(hits
            .into_iter()
            .map(|hit| hit.path)
            .find(|path| path.exists()))
    }

    pub fn query(
        &self,
        query: &str,
        limit: Option<usize>,
    ) -> Result<Vec<MessageSummary>, SearchError> {
        let wrap = |source| SearchError::Query {
            query: query.to_owned(),
            source,
        };
        let parsed = self.db.create_query(query).map_err(wrap)?;
        parsed.set_sort(Sort::NewestFirst);
        let messages = parsed.search_messages().map_err(wrap)?;
        messages
            .into_iter()
            .take(limit.unwrap_or(usize::MAX))
            .map(|message| summarise(&message).map_err(wrap))
            .collect()
    }

    pub fn query_scoped(
        &self,
        scope: &Scope,
        user_query: &str,
        limit: Option<usize>,
    ) -> Result<Vec<MessageSummary>, SearchError> {
        let query = scoped_query(scope, user_query)
            .map_err(|source| SearchError::Scope { source })?;
        let hits = self.query(&query, limit)?;
        // The textual scope holds only as far as the index is
        // honest: a message-id duplicated across accounts, or a
        // file moved since the last reindex, can surface with a
        // filename outside the scope. Anything not on disk
        // under a permitted account is dropped rather than
        // shown.
        Ok(hits
            .into_iter()
            .filter(|hit| self.path_in_scope(scope, &hit.path))
            .collect())
    }

    fn path_in_scope(&self, scope: &Scope, path: &Path) -> bool {
        let Ok(relative) = path.strip_prefix(&self.maildir_root) else {
            return false;
        };
        let Some(account) = relative
            .components()
            .next()
            .map(|c| c.as_os_str().to_string_lossy())
        else {
            return false;
        };
        scope.permits(&account)
    }

    pub fn count_scoped(
        &self,
        scope: &Scope,
        user_query: &str,
    ) -> Result<u32, SearchError> {
        let query = scoped_query(scope, user_query)
            .map_err(|source| SearchError::Scope { source })?;
        self.count(&query)
    }
}

/// One notmuch query selecting a message by id, with embedded
/// double quotes doubled per Xapian quoting.
pub fn id_query(message_id: &str) -> String {
    let quoted = message_id.replace('"', "\"\"");
    format!("id:\"{quoted}\"")
}

fn summarise(
    message: &Message,
) -> Result<MessageSummary, notmuch::Error> {
    let tags: Vec<String> = message.tags().collect();
    let unread = tags.iter().any(|tag| tag == UNREAD_TAG);
    let references =
        message_ids(&header_or_empty(message, "references")?);
    let in_reply_to =
        sole_message_id(&header_or_empty(message, "in-reply-to")?);
    Ok(MessageSummary {
        id: message.id().into_owned(),
        thread_id: message.thread_id().into_owned(),
        subject: header_or_empty(message, "subject")?,
        from: header_or_empty(message, "from")?,
        to: header_or_empty(message, "to")?,
        date_unix: message.date(),
        tags,
        unread,
        path: message.filename().to_path_buf(),
        paths: message.filenames().collect(),
        in_reply_to,
        references,
    })
}

/// The bare Message-IDs inside a References or In-Reply-To
/// header, in order, with the angle brackets stripped so they
/// compare equal to notmuch's `id`.
fn message_ids(value: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut rest = value;
    while let Some(open) = rest.find('<') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('>') else {
            break;
        };
        let id = after[..close].trim();
        if !id.is_empty() {
            ids.push(id.to_string());
        }
        rest = &after[close + 1..];
    }
    ids
}

/// The single parent id from an In-Reply-To: the last bracketed
/// id, or a lone unbracketed token where a client omitted the
/// brackets.
fn sole_message_id(value: &str) -> Option<String> {
    if let Some(last) = message_ids(value).pop() {
        return Some(last);
    }
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.contains(char::is_whitespace) {
        return None;
    }
    Some(trimmed.to_string())
}

fn header_or_empty(
    message: &Message,
    name: &str,
) -> Result<String, notmuch::Error> {
    let value = message.header(name)?;
    Ok(value.map(Cow::into_owned).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_strip_brackets_and_keep_order() {
        let ids = message_ids("<a@x>  <b@y>\n <c@z>");
        assert_eq!(ids, ["a@x", "b@y", "c@z"]);
        assert!(message_ids("no brackets here").is_empty());
    }

    #[test]
    fn in_reply_to_takes_the_last_id_or_a_lone_token() {
        assert_eq!(
            sole_message_id("<old@x> <parent@y>").as_deref(),
            Some("parent@y")
        );
        assert_eq!(
            sole_message_id("bare@x").as_deref(),
            Some("bare@x")
        );
        assert_eq!(sole_message_id("  "), None);
        assert_eq!(sole_message_id("two bare@x"), None);
    }
}
