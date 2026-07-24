use antiphon_config::SavedSearch;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SidebarEntry {
    Unified,
    Account(String),
    Saved { name: String, query: String },
}

impl SidebarEntry {
    pub fn label(&self) -> &str {
        match self {
            SidebarEntry::Unified => "unified",
            SidebarEntry::Account(account) => account,
            SidebarEntry::Saved { name, .. } => name,
        }
    }

    pub fn is_saved(&self) -> bool {
        matches!(self, SidebarEntry::Saved { .. })
    }
}

/// Built-in unified views; plain `*` is the unified entry
/// itself, so it is not listed here.
const BUILTIN_SEARCHES: &[(&str, &str)] = &[
    ("inbox", "tag:inbox"),
    ("unread", "tag:unread"),
    ("flagged", "tag:flagged"),
];

pub fn entries(
    accounts: &[String],
    saved: &[SavedSearch],
) -> Vec<SidebarEntry> {
    let mut items = vec![SidebarEntry::Unified];
    items.extend(accounts.iter().cloned().map(SidebarEntry::Account));
    items.extend(BUILTIN_SEARCHES.iter().map(|(name, query)| {
        SidebarEntry::Saved {
            name: (*name).to_string(),
            query: (*query).to_string(),
        }
    }));
    items.extend(saved.iter().map(|search| SidebarEntry::Saved {
        name: search.name.clone(),
        query: search.query.clone(),
    }));
    items
}

pub fn next_index(selected: usize, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    (selected + 1) % count
}

pub fn previous_index(selected: usize, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    (selected + count - 1) % count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(pairs: &[(&str, &str)]) -> Vec<SavedSearch> {
        pairs
            .iter()
            .map(|(name, query)| SavedSearch {
                name: (*name).to_string(),
                query: (*query).to_string(),
            })
            .collect()
    }

    #[test]
    fn entries_run_unified_accounts_builtins_then_config() {
        let accounts = vec!["work".to_string(), "personal".to_string()];
        let searches =
            saved(&[("patches", "diffstat"), ("boss", "from:boss")]);
        let items = entries(&accounts, &searches);
        let labels: Vec<&str> =
            items.iter().map(SidebarEntry::label).collect();
        assert_eq!(
            labels,
            [
                "unified", "work", "personal", "inbox", "unread",
                "flagged", "patches", "boss",
            ],
        );
    }

    #[test]
    fn config_searches_keep_their_queries_in_order() {
        let searches =
            saved(&[("patches", "diffstat"), ("boss", "from:boss")]);
        let items = entries(&[], &searches);
        let queries: Vec<&str> = items
            .iter()
            .filter_map(|entry| match entry {
                SidebarEntry::Saved { query, .. } => {
                    Some(query.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            queries,
            [
                "tag:inbox",
                "tag:unread",
                "tag:flagged",
                "diffstat",
                "from:boss",
            ],
        );
    }

    #[test]
    fn navigation_wraps_both_ways() {
        assert_eq!(next_index(0, 3), 1);
        assert_eq!(next_index(2, 3), 0);
        assert_eq!(previous_index(0, 3), 2);
        assert_eq!(previous_index(2, 3), 1);
        assert_eq!(next_index(0, 0), 0);
        assert_eq!(previous_index(0, 0), 0);
    }
}
