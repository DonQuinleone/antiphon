use std::borrow::Cow;
use std::fmt;
use std::path::PathBuf;

use notmuch::{Database, DatabaseMode, Message, Sort};

use crate::layout::StoreLayout;

const UNREAD_TAG: &str = "unread";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageSummary {
    pub id: String,
    pub thread_id: String,
    pub subject: String,
    pub from: String,
    pub date_unix: i64,
    pub tags: Vec<String>,
    pub unread: bool,
    pub path: PathBuf,
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
        }
    }
}

impl std::error::Error for SearchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Open { source, .. } => Some(source),
            Self::Query { source, .. } => Some(source),
        }
    }
}

pub struct SearchIndex {
    db: Database,
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
        Ok(Self { db })
    }

    pub fn count(&self, query: &str) -> Result<u32, SearchError> {
        let wrap = |source| SearchError::Query {
            query: query.to_owned(),
            source,
        };
        let parsed = self.db.create_query(query).map_err(wrap)?;
        parsed.count_messages().map_err(wrap)
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
}

fn summarise(
    message: &Message,
) -> Result<MessageSummary, notmuch::Error> {
    let tags: Vec<String> = message.tags().collect();
    let unread = tags.iter().any(|tag| tag == UNREAD_TAG);
    Ok(MessageSummary {
        id: message.id().into_owned(),
        thread_id: message.thread_id().into_owned(),
        subject: header_or_empty(message, "subject")?,
        from: header_or_empty(message, "from")?,
        date_unix: message.date(),
        tags,
        unread,
        path: message.filename().to_path_buf(),
    })
}

fn header_or_empty(
    message: &Message,
    name: &str,
) -> Result<String, notmuch::Error> {
    let value = message.header(name)?;
    Ok(value.map(Cow::into_owned).unwrap_or_default())
}
