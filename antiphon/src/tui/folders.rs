use std::io;

use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::app::App;
use super::configedit::{persist_root_key, toml_string_array};
use super::folder_alias::begin_edit;
use super::settings::{self, SettingsOutcome};
use super::sidebar::{self, AccountEntry};

const FOLDER_ORDER_KEY: &str = "folder_order";
const FOLDERS_HIDDEN_KEY: &str = "folders_hidden";

/// One row of the Folders settings tab: a discovered folder,
/// its account, whatever alias is currently configured for it
/// (empty when none is), and whether the sidebar hides it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FolderRow {
    pub(super) account: String,
    pub(super) folder: String,
    pub(super) alias: String,
    pub(super) hidden: bool,
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
/// h hides or unhides it, enter begins editing its alias.
pub(super) fn feed(app: &mut App, key: KeyEvent) -> SettingsOutcome {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => move_selection(app, 1),
        KeyCode::Char('k') | KeyCode::Up => move_selection(app, -1),
        KeyCode::Char('J') => shift_folder_order(app, 1),
        KeyCode::Char('K') => shift_folder_order(app, -1),
        KeyCode::Char('h') => toggle_hidden(app),
        KeyCode::Enter => begin_edit(app),
        _ => {}
    }
    SettingsOutcome::Stay
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
mod tests {
    use super::super::testkit::TempDir;
    use super::*;

    fn account(name: &str, folders: &[&str]) -> AccountEntry {
        AccountEntry {
            name: name.to_string(),
            folders: folders
                .iter()
                .map(|folder| (*folder).to_string())
                .collect(),
            ..AccountEntry::default()
        }
    }

    fn row(account: &str, folder: &str, alias: &str) -> FolderRow {
        FolderRow {
            account: account.to_string(),
            folder: folder.to_string(),
            alias: alias.to_string(),
            hidden: false,
        }
    }

    #[test]
    fn rows_join_the_alias_by_account_and_folder() {
        let accounts = vec![
            account("work", &["archive", "lists/aerc"]),
            account("personal", &["archive"]),
        ];
        let aliases = vec![(
            "work".to_string(),
            "lists/aerc".to_string(),
            "aerc-list".to_string(),
        )];
        let built = rows(&accounts, &aliases);
        assert_eq!(
            built,
            vec![
                row("work", "inbox", ""),
                row("work", "archive", ""),
                row("work", "lists/aerc", "aerc-list"),
                row("personal", "inbox", ""),
                row("personal", "archive", ""),
            ]
        );
    }

    #[test]
    fn rows_follow_the_order_and_keep_hidden_folders() {
        let mut work = account("work", &["archive", "spam"]);
        work.order = vec!["spam".to_string()];
        work.hidden = vec!["spam".to_string()];
        let built = rows(&[work], &[]);
        let summary: Vec<(&str, bool)> = built
            .iter()
            .map(|row| (row.folder.as_str(), row.hidden))
            .collect();
        assert_eq!(
            summary,
            [("spam", true), ("inbox", false), ("archive", false)],
        );
    }

    #[test]
    fn an_alias_with_no_matching_folder_is_never_joined_in() {
        let accounts = vec![account("work", &["archive"])];
        let aliases = vec![(
            "work".to_string(),
            "gone".to_string(),
            "stale".to_string(),
        )];
        let built = rows(&accounts, &aliases);
        assert!(built.iter().all(|row| row.alias.is_empty()));
    }

    fn app_on_folders_tab(dir: &TempDir) -> App {
        use super::super::settings::{SettingsState, SettingsTab};

        let mut app = super::super::testkit::app_with_folders(&[(
            "work",
            &["archive", "lists/aerc", "spam"][..],
        )]);
        app.dirs.config = dir.path.clone();
        let accounts_dir = dir.path.join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();
        std::fs::write(
            accounts_dir.join("work.toml"),
            "[account]\nname = \"work\"\n",
        )
        .unwrap();
        app.settings = Some(SettingsState {
            tab: SettingsTab::Folders,
            accounts: Vec::new(),
            account_selected: 0,
            pending_delete: None,
            essentials_selected: 0,
            daemon_hint: None,
            folders: app.folder_rows(),
            folder_selected: 0,
        });
        app
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(
            code,
            ratatui::crossterm::event::KeyModifiers::NONE,
        )
    }

    fn folder_names(app: &App) -> Vec<String> {
        app.settings
            .as_ref()
            .unwrap()
            .folders
            .iter()
            .map(|row| row.folder.clone())
            .collect()
    }

    #[test]
    fn shift_j_reorders_and_persists_the_whole_account_order() {
        let dir = TempDir::new();
        let mut app = app_on_folders_tab(&dir);
        assert_eq!(
            folder_names(&app),
            ["inbox", "archive", "lists/aerc", "spam"]
        );

        feed(&mut app, key(KeyCode::Char('J')));
        assert_eq!(
            folder_names(&app),
            ["archive", "inbox", "lists/aerc", "spam"]
        );
        assert_eq!(
            app.settings.as_ref().unwrap().folder_selected,
            1,
            "the selection follows the moved row"
        );
        let text = std::fs::read_to_string(
            dir.path.join("accounts/work.toml"),
        )
        .unwrap();
        assert!(
            text.starts_with(
                "folder_order = [\"archive\", \"inbox\", \
                 \"lists/aerc\", \"spam\"]\n"
            ),
            "{text}"
        );
        assert!(text.contains("[account]"), "the rest survives");

        feed(&mut app, key(KeyCode::Char('K')));
        assert_eq!(
            folder_names(&app),
            ["inbox", "archive", "lists/aerc", "spam"]
        );
        feed(&mut app, key(KeyCode::Char('K')));
        assert_eq!(
            folder_names(&app),
            ["inbox", "archive", "lists/aerc", "spam"],
            "moving up from the top clamps"
        );
    }

    #[test]
    fn h_hides_a_folder_from_the_sidebar_and_back() {
        let dir = TempDir::new();
        let mut app = app_on_folders_tab(&dir);
        app.settings.as_mut().unwrap().folder_selected = 3;

        feed(&mut app, key(KeyCode::Char('h')));
        let state = app.settings.as_ref().unwrap();
        assert!(state.folders[3].hidden, "the row stays, marked");
        assert!(
            !app.sidebar_entries
                .iter()
                .any(|entry| entry.label() == "spam"),
            "the sidebar drops it"
        );
        let text = std::fs::read_to_string(
            dir.path.join("accounts/work.toml"),
        )
        .unwrap();
        assert!(text.contains("folders_hidden = [\"spam\"]"), "{text}");

        feed(&mut app, key(KeyCode::Char('h')));
        assert!(!app.settings.as_ref().unwrap().folders[3].hidden);
        assert!(
            app.sidebar_entries
                .iter()
                .any(|entry| entry.label() == "spam"),
        );
        let text = std::fs::read_to_string(
            dir.path.join("accounts/work.toml"),
        )
        .unwrap();
        assert!(text.contains("folders_hidden = []"), "{text}");
    }
}
