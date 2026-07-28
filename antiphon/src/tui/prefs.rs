//! Collections the TUI keeps from the loaded configuration:
//! per-account folder names, aliases, identities and sidebar
//! preferences, each flattened into the shape its consumer
//! reads every frame.

use antiphon_config::Loaded;

use super::sidebar::AccountEntry;

pub(super) fn archive_folders(
    loaded: &Loaded,
) -> Vec<(String, String)> {
    named_folders(loaded, |account| account.archive.clone())
}

pub(super) fn trash_folders(loaded: &Loaded) -> Vec<(String, String)> {
    named_folders(loaded, |account| account.trash.clone())
}

fn named_folders(
    loaded: &Loaded,
    pick: fn(&antiphon_config::Account) -> Option<String>,
) -> Vec<(String, String)> {
    loaded
        .accounts
        .iter()
        .filter_map(|entry| {
            let folder = pick(&entry.account.account)?;
            Some((entry.account.account.name.clone(), folder))
        })
        .collect()
}

pub(super) fn folder_aliases(
    loaded: &Loaded,
) -> Vec<(String, String, String)> {
    loaded
        .accounts
        .iter()
        .flat_map(|entry| {
            let account = entry.account.account.name.clone();
            entry.account.folder_names.iter().map(
                move |(real, alias)| {
                    (account.clone(), real.clone(), alias.clone())
                },
            )
        })
        .collect()
}

pub(super) fn own_addresses(loaded: &Loaded) -> Vec<String> {
    loaded
        .accounts
        .iter()
        .flat_map(|entry| entry.account.identities.iter())
        .map(|identity| identity.address.to_lowercase())
        .collect()
}

/// One sidebar seed per account, folders still undiscovered:
/// `sidebar::discover` fills those in from the store.
pub(super) fn account_seeds(loaded: &Loaded) -> Vec<AccountEntry> {
    loaded
        .accounts
        .iter()
        .map(|entry| AccountEntry {
            name: entry.account.account.name.clone(),
            folders: Vec::new(),
            order: entry.account.folder_order.clone(),
            hidden: entry.account.folders_hidden.clone(),
            unsynced: entry.account.folders_unsynced.clone(),
        })
        .collect()
}
