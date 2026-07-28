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
        unread: u32,
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

/// An account, the store folders discovered under it, and the
/// account's own sidebar preferences (`folder_order` and
/// `folders_hidden` from its config file).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccountEntry {
    pub name: String,
    pub folders: Vec<String>,
    pub order: Vec<String>,
    pub hidden: Vec<String>,
}

/// Refreshes each account's discovered folders, keeping the
/// name and preferences of the seed entries.
pub fn discover(
    layout: &StoreLayout,
    seeds: &[AccountEntry],
) -> Vec<AccountEntry> {
    seeds
        .iter()
        .map(|seed| AccountEntry {
            name: seed.name.clone(),
            folders: layout.account_folders(&seed.name),
            order: seed.order.clone(),
            hidden: seed.hidden.clone(),
        })
        .collect()
}

/// Every sidebar name of the account (inbox included, hidden
/// ones too, for the settings Folders tab): names listed in
/// `folder_order` lead in that order, the rest keep their
/// alphabetical rank with inbox first among them.
pub fn ordered_names(account: &AccountEntry) -> Vec<String> {
    let mut names = Vec::with_capacity(account.folders.len() + 1);
    names.push(INBOX_LABEL.to_string());
    names.extend(account.folders.iter().cloned());
    names.sort_by_key(|name| order_rank(&account.order, name));
    names
}

fn order_rank(order: &[String], name: &str) -> usize {
    order
        .iter()
        .position(|listed| listed == name)
        .unwrap_or(order.len())
}

pub fn is_hidden(account: &AccountEntry, name: &str) -> bool {
    account.hidden.iter().any(|hidden| hidden == name)
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
        items.extend(folder_entries(account));
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

/// The account's folder rows, `folder_order` applied and the
/// hidden ones dropped; sorting the built entries rather than
/// bare names keeps a subdirectory literally named inbox
/// distinct from the root inbox.
pub fn folder_entries(account: &AccountEntry) -> Vec<SidebarEntry> {
    let mut items = vec![inbox_entry(&account.name)];
    items.extend(
        account
            .folders
            .iter()
            .map(|folder| folder_entry(&account.name, folder)),
    );
    items
        .sort_by_key(|entry| order_rank(&account.order, entry.label()));
    items.retain(|entry| !is_hidden(account, entry.label()));
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
        unread: 0,
    }
}

fn folder_entry(account: &str, folder: &str) -> SidebarEntry {
    SidebarEntry::Folder {
        account: account.to_string(),
        name: folder.to_string(),
        query: format!("path:\"{account}/{folder}/**\""),
        unread: 0,
    }
}

/// Stamps unread counts onto the folder entries; the counter
/// answers "how many unread match this query", normally the
/// notmuch index, injectable so the maths tests offline.
pub fn fill_unread(
    entries: &mut [SidebarEntry],
    mut count: impl FnMut(&str) -> Option<u32>,
) {
    for entry in entries {
        let SidebarEntry::Folder { query, unread, .. } = entry else {
            continue;
        };
        *unread = count(query).unwrap_or(0);
    }
}

pub fn unread_of(entry: &SidebarEntry) -> u32 {
    match entry {
        SidebarEntry::Folder { unread, .. } => *unread,
        _ => 0,
    }
}

/// Startup lands in the first account's inbox, falling back
/// to the first saved search, then the top of the list.
pub fn default_selection(entries: &[SidebarEntry]) -> usize {
    entries
        .iter()
        .position(SidebarEntry::is_folder)
        .or_else(|| entries.iter().position(SidebarEntry::is_saved))
        .unwrap_or(0)
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
                ..AccountEntry::default()
            })
            .collect()
    }

    fn strings(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn folder_order_leads_and_the_rest_stay_alphabetical() {
        let mut account = accounts(&[(
            "work",
            &["archive", "drafts", "lists/aerc"][..],
        )])
        .remove(0);
        account.order = strings(&["lists/aerc", "ghost", "inbox"]);
        let names = ordered_names(&account);
        assert_eq!(names, ["lists/aerc", "inbox", "archive", "drafts"]);
        let labels: Vec<String> = folder_entries(&account)
            .iter()
            .map(|entry| entry.label().to_string())
            .collect();
        assert_eq!(
            labels,
            ["lists/aerc", "inbox", "archive", "drafts"]
        );
    }

    #[test]
    fn hidden_folders_leave_the_sidebar_but_not_the_names() {
        let mut account =
            accounts(&[("work", &["archive", "spam"][..])]).remove(0);
        account.hidden = strings(&["spam"]);
        let labels: Vec<String> = folder_entries(&account)
            .iter()
            .map(|entry| entry.label().to_string())
            .collect();
        assert_eq!(labels, ["inbox", "archive"]);
        assert!(is_hidden(&account, "spam"));
        assert_eq!(
            ordered_names(&account),
            ["inbox", "archive", "spam"],
            "the settings tab still lists hidden folders"
        );
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
    fn the_default_selection_is_the_first_inbox() {
        let items = entries(
            &accounts(&[("work", &["archive"])]),
            &saved(&[("boss", "from:boss")]),
        );
        let index = default_selection(&items);
        assert_eq!(items[index].label(), INBOX_LABEL);
        assert!(items[index].is_folder());

        let searches_only = entries(&[], &saved(&[]));
        let fallback = default_selection(&searches_only);
        assert_eq!(searches_only[fallback].label(), ALL_LABEL);
        assert_eq!(default_selection(&[]), 0);
    }

    #[test]
    fn fill_unread_stamps_folders_and_skips_the_rest() {
        let mut items = entries(
            &accounts(&[("work", &["archive"])]),
            &saved(&[("boss", "from:boss")]),
        );
        fill_unread(&mut items, |query| {
            match query.contains("archive") {
                true => Some(3),
                false => None,
            }
        });
        let counts: Vec<u32> = items.iter().map(unread_of).collect();
        assert_eq!(counts, [0, 0, 0, 3, 0, 0, 0, 0, 0]);
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
