use antiphon_config::SavedSearch;
use antiphon_store::StoreLayout;

pub const ALL_LABEL: &str = "all";
const INBOX_LABEL: &str = "inbox";
const ALL_QUERY: &str = "*";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SidebarEntry {
    Unified,
    Account(String),
    Folder {
        account: String,
        name: String,
        query: String,
    },
    Saved {
        name: String,
        query: String,
    },
}

impl SidebarEntry {
    pub fn label(&self) -> &str {
        match self {
            SidebarEntry::Unified => "unified",
            SidebarEntry::Account(account) => account,
            SidebarEntry::Folder { name, .. } => name,
            SidebarEntry::Saved { name, .. } => name,
        }
    }

    pub fn is_saved(&self) -> bool {
        matches!(self, SidebarEntry::Saved { .. })
    }

    pub fn is_folder(&self) -> bool {
        matches!(self, SidebarEntry::Folder { .. })
    }
}

/// An account and the store folders discovered under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountEntry {
    pub name: String,
    pub folders: Vec<String>,
}

pub fn discover(
    layout: &StoreLayout,
    accounts: &[String],
) -> Vec<AccountEntry> {
    accounts
        .iter()
        .map(|name| AccountEntry {
            name: name.clone(),
            folders: layout.account_folders(name),
        })
        .collect()
}

/// Built-in unified views, `all` first so an inbox-zero
/// account still shows its mail on startup.
const BUILTIN_SEARCHES: &[(&str, &str)] = &[
    (ALL_LABEL, ALL_QUERY),
    (INBOX_LABEL, "tag:inbox"),
    ("unread", "tag:unread"),
    ("flagged", "tag:flagged"),
];

pub fn entries(
    accounts: &[AccountEntry],
    saved: &[SavedSearch],
) -> Vec<SidebarEntry> {
    let mut items = vec![SidebarEntry::Unified];
    for account in accounts {
        items.push(SidebarEntry::Account(account.name.clone()));
        items.push(inbox_entry(&account.name));
        items.extend(
            account
                .folders
                .iter()
                .map(|folder| folder_entry(&account.name, folder)),
        );
    }
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

/// The account root's cur/new pair is its inbox; a maildir
/// subdirectory literally named inbox stays a plain folder
/// with its own path query.
fn inbox_entry(account: &str) -> SidebarEntry {
    SidebarEntry::Folder {
        account: account.to_string(),
        name: INBOX_LABEL.to_string(),
        query: format!(
            "path:\"{account}/cur\" or path:\"{account}/new\""
        ),
    }
}

fn folder_entry(account: &str, folder: &str) -> SidebarEntry {
    SidebarEntry::Folder {
        account: account.to_string(),
        name: folder.to_string(),
        query: format!("path:\"{account}/{folder}/**\""),
    }
}

/// Startup lands on the `all` built-in, the first saved entry,
/// so a user at inbox zero still sees their mail.
pub fn default_selection(entries: &[SidebarEntry]) -> usize {
    entries.iter().position(SidebarEntry::is_saved).unwrap_or(0)
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

    fn accounts(entries: &[(&str, &[&str])]) -> Vec<AccountEntry> {
        entries
            .iter()
            .map(|(name, folders)| AccountEntry {
                name: (*name).to_string(),
                folders: folders
                    .iter()
                    .map(|folder| (*folder).to_string())
                    .collect(),
            })
            .collect()
    }

    #[test]
    fn entries_nest_folders_under_their_account() {
        let items = entries(
            &accounts(&[
                ("work", &["archive", "lists/aerc"]),
                ("personal", &[]),
            ]),
            &saved(&[("boss", "from:boss")]),
        );
        let labels: Vec<&str> =
            items.iter().map(SidebarEntry::label).collect();
        assert_eq!(
            labels,
            [
                "unified",
                "work",
                "inbox",
                "archive",
                "lists/aerc",
                "personal",
                "inbox",
                "all",
                "inbox",
                "unread",
                "flagged",
                "boss",
            ],
        );
    }

    #[test]
    fn folder_queries_follow_the_store_path_semantics() {
        let items = entries(&accounts(&[("work", &["archive"])]), &[]);
        let queries: Vec<(&str, &str)> = items
            .iter()
            .filter_map(|entry| match entry {
                SidebarEntry::Folder { name, query, .. } => {
                    Some((name.as_str(), query.as_str()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            queries,
            [
                ("inbox", "path:\"work/cur\" or path:\"work/new\""),
                ("archive", "path:\"work/archive/**\""),
            ],
        );
    }

    #[test]
    fn config_searches_follow_the_builtins_in_order() {
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
                "*",
                "tag:inbox",
                "tag:unread",
                "tag:flagged",
                "diffstat",
                "from:boss",
            ],
        );
    }

    #[test]
    fn the_default_selection_is_the_all_search() {
        let items = entries(
            &accounts(&[("work", &["archive"])]),
            &saved(&[("boss", "from:boss")]),
        );
        let index = default_selection(&items);
        assert_eq!(items[index].label(), ALL_LABEL);
        assert_eq!(default_selection(&[]), 0);
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
