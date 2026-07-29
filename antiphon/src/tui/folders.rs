use std::io;

use antiphon_core::Action;

use super::app::App;
use super::configedit::{persist_root_key, toml_string_array};
use super::folder_alias::begin_edit;
use super::sidebar::{self, AccountEntry};
use crate::tui::settings;

const FOLDER_ORDER_KEY: &str = "folder_order";
const FOLDERS_HIDDEN_KEY: &str = "folders_hidden";
const FOLDERS_UNSYNCED_KEY: &str = "folders_unsynced";

/// One row of the Folders settings tab: a discovered folder,
/// its account, whatever alias is currently configured for it
/// (empty when none is), whether the sidebar hides it, and
/// whether the daemon skips syncing it. Hidden and unsynced are
/// independent: a hidden folder is still downloaded, an
/// unsynced one is not (and so is implicitly off the sidebar
/// too, whether or not it is also marked hidden).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FolderRow {
    pub(super) account: String,
    pub(super) folder: String,
    pub(super) alias: String,
    pub(super) hidden: bool,
    pub(super) unsynced: bool,
}

/// Every folder of every account in sidebar order, hidden
/// ones included so they can be unhidden, alias joined in;
/// pure over the discovered accounts and the loaded aliases,
/// so it needs neither `App` nor the disk to test.
pub(super) fn rows(
    accounts: &[AccountEntry],
    aliases: &[(String, String, String)],
) -> Vec<FolderRow> {
    accounts
        .iter()
        .flat_map(|account| {
            sidebar::ordered_names(account)
                .into_iter()
                .map(|name| folder_row(account, name, aliases))
        })
        .collect()
}

fn folder_row(
    account: &AccountEntry,
    name: String,
    aliases: &[(String, String, String)],
) -> FolderRow {
    let alias = aliases
        .iter()
        .find(|(acct, real, _)| *acct == account.name && *real == name)
        .map(|(_, _, alias)| alias.clone())
        .unwrap_or_default();
    FolderRow {
        account: account.name.clone(),
        hidden: sidebar::is_hidden(account, &name),
        unsynced: sidebar::is_unsynced(account, &name),
        folder: name,
        alias,
    }
}

impl App {
    pub(super) fn folder_rows(&self) -> Vec<FolderRow> {
        rows(&self.account_entries, &self.folder_aliases)
    }

    /// Re-derives the Folders tab's rows from the current
    /// sidebar and aliases; called after a save, so a rename
    /// or removal is reflected without re-opening settings.
    pub(super) fn refresh_settings_folders(&mut self) {
        let folders = self.folder_rows();
        let Some(state) = self.settings.as_mut() else {
            return;
        };
        let last = folders.len().saturating_sub(1);
        state.folders = folders;
        state.folder_selected = state.folder_selected.min(last);
    }
}

/// Keys on the Folders tab outside of an active alias edit:
/// j/k select a row, J/K move it through its account's order,
/// h hides or unhides it, u unsyncs or resyncs it, enter begins
/// editing its alias.
pub(super) fn apply(app: &mut App, action: Action) {
    match action {
        Action::MoveDown => move_selection(app, 1),
        Action::MoveUp => move_selection(app, -1),
        Action::ReorderDown => shift_folder_order(app, 1),
        Action::ReorderUp => shift_folder_order(app, -1),
        Action::FolderHide => toggle_hidden(app),
        Action::FolderUnsync => toggle_unsynced(app),
        Action::EditAlias => begin_edit(app),
        _ => {}
    }
}

/// Shift+J/K move the selected folder within its account and
/// persist the account's whole sidebar order (inbox included)
/// to `folder_order` in its config file; the sidebar follows
/// at once.
fn shift_folder_order(app: &mut App, step: i32) {
    let Some(row) = selected_row(app) else {
        return;
    };
    let Some(entry) = account_entry(app, &row.account) else {
        return;
    };
    let mut names = sidebar::ordered_names(entry);
    let Some(from) = account_row_offset(app, &row.account) else {
        return;
    };
    let last = names.len().saturating_sub(1) as i32;
    let to = (from as i32 + step).clamp(0, last) as usize;
    if to == from {
        return;
    }
    names.swap(from, to);
    apply_order(app, &row.account, names);
    move_selection(app, step);
}

/// The selected row's position within its own account's run of
/// rows, which mirrors its position in `ordered_names`.
fn account_row_offset(app: &App, account: &str) -> Option<usize> {
    let state = app.settings.as_ref()?;
    let first = state
        .folders
        .iter()
        .position(|row| row.account == account)?;
    Some(state.folder_selected - first)
}

fn account_entry<'a>(
    app: &'a App,
    account: &str,
) -> Option<&'a AccountEntry> {
    app.account_entries
        .iter()
        .find(|entry| entry.name == account)
}

fn apply_order(app: &mut App, account: &str, names: Vec<String>) {
    let value = toml_string_array(&names);
    let saved =
        persist_account_key(app, account, FOLDER_ORDER_KEY, &value);
    let Some(entry) = app
        .account_entries
        .iter_mut()
        .find(|entry| entry.name == account)
    else {
        return;
    };
    entry.order = names;
    refresh_after_edit(app, saved, "folder order saved");
}

/// h hides a visible folder or unhides a hidden one, persisting
/// `folders_hidden`; a hidden folder stays synced and
/// searchable, it just leaves the sidebar.
fn toggle_hidden(app: &mut App) {
    let Some(row) = selected_row(app) else {
        return;
    };
    let Some(entry) = app
        .account_entries
        .iter_mut()
        .find(|entry| entry.name == row.account)
    else {
        return;
    };
    if row.hidden {
        entry.hidden.retain(|name| *name != row.folder);
    } else {
        entry.hidden.push(row.folder.clone());
    }
    let hidden = entry.hidden.clone();
    let saved = persist_account_key(
        app,
        &row.account,
        FOLDERS_HIDDEN_KEY,
        &toml_string_array(&hidden),
    );
    let notice = match row.hidden {
        true => format!("{} shown again", row.folder),
        false => format!("{} hidden from the sidebar", row.folder),
    };
    refresh_after_edit(app, saved, &notice);
}

/// u tells the daemon to stop syncing a folder or to resume it,
/// persisting `folders_unsynced`; the inbox is never
/// excludable, matching the sync engine, so u refuses it. An
/// unsynced folder is never downloaded, so it leaves the
/// sidebar too; any local mail from before it was excluded
/// stays put until a manual clean.
fn toggle_unsynced(app: &mut App) {
    let Some(row) = selected_row(app) else {
        return;
    };
    if row.folder == sidebar::INBOX_LABEL {
        app.notice = Some("the inbox is always synced".to_string());
        return;
    }
    let Some(entry) = app
        .account_entries
        .iter_mut()
        .find(|entry| entry.name == row.account)
    else {
        return;
    };
    if row.unsynced {
        entry.unsynced.retain(|name| *name != row.folder);
    } else {
        entry.unsynced.push(row.folder.clone());
    }
    let unsynced = entry.unsynced.clone();
    let saved = persist_account_key(
        app,
        &row.account,
        FOLDERS_UNSYNCED_KEY,
        &toml_string_array(&unsynced),
    );
    let notice = match row.unsynced {
        true => format!("{} syncing again", row.folder),
        false => format!("{} will no longer sync", row.folder),
    };
    refresh_after_edit(app, saved, &notice);
}

fn persist_account_key(
    app: &App,
    account: &str,
    key: &str,
    value: &str,
) -> io::Result<()> {
    let path = app
        .dirs
        .config
        .join("accounts")
        .join(format!("{account}.toml"));
    persist_root_key(&path, key, value)
}

fn refresh_after_edit(
    app: &mut App,
    saved: io::Result<()>,
    notice: &str,
) {
    app.rebuild_sidebar();
    app.refresh_settings_folders();
    app.notice = Some(match saved {
        Ok(()) => notice.to_string(),
        Err(error) => format!("{notice} (not saved: {error})"),
    });
}

fn move_selection(app: &mut App, step: i32) {
    let Some(state) = app.settings.as_mut() else {
        return;
    };
    if state.folders.is_empty() {
        return;
    }
    state.folder_selected = settings::wrapped(
        state.folder_selected,
        state.folders.len(),
        step,
    );
}

pub(super) fn selected_row(app: &App) -> Option<FolderRow> {
    let state = app.settings.as_ref()?;
    state.folders.get(state.folder_selected).cloned()
}

#[cfg(test)]
#[path = "folders_tests.rs"]
mod tests;
