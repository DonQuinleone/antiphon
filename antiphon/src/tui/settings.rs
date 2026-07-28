use antiphon_config::{Dirs, NamedAccount};
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::app::{App, View};
use super::settingscmd;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SettingsTab {
    Accounts,
    Essentials,
    Folders,
}

impl SettingsTab {
    fn next(self) -> SettingsTab {
        match self {
            SettingsTab::Accounts => SettingsTab::Essentials,
            SettingsTab::Essentials => SettingsTab::Folders,
            SettingsTab::Folders => SettingsTab::Accounts,
        }
    }

    fn previous(self) -> SettingsTab {
        match self {
            SettingsTab::Accounts => SettingsTab::Folders,
            SettingsTab::Essentials => SettingsTab::Accounts,
            SettingsTab::Folders => SettingsTab::Essentials,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AccountSummary {
    pub(super) name: String,
    pub(super) address: String,
    pub(super) host: String,
}

pub(super) struct SettingsState {
    pub(super) tab: SettingsTab,
    pub(super) accounts: Vec<AccountSummary>,
    pub(super) account_selected: usize,
    pub(super) pending_delete: Option<String>,
    pub(super) essentials_selected: usize,
    pub(super) daemon_hint: Option<String>,
    pub(super) folders: Vec<super::folders::FolderRow>,
    pub(super) folder_selected: usize,
}

/// What a settings key asks of the event loop; add and edit
/// open the account form in place, so only closing the whole
/// settings view bubbles up.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum SettingsOutcome {
    Stay,
    Close,
}

impl App {
    pub(super) fn open_settings(&mut self) {
        let folders = self.folder_rows();
        self.settings = Some(SettingsState {
            tab: SettingsTab::Accounts,
            accounts: account_summaries(&self.dirs),
            account_selected: 0,
            pending_delete: None,
            essentials_selected: 0,
            daemon_hint: None,
            folders,
            folder_selected: 0,
        });
        self.view = View::Settings;
    }

    /// Re-reads `accounts/*.toml` off disk: the only account
    /// listing kept in memory, so add, edit and remove all
    /// settle here rather than each patching their own copy.
    pub(super) fn refresh_settings_accounts(&mut self) {
        let accounts = account_summaries(&self.dirs);
        let Some(state) = self.settings.as_mut() else {
            return;
        };
        let last = accounts.len().saturating_sub(1);
        state.accounts = accounts;
        state.account_selected = state.account_selected.min(last);
    }
}

pub(super) fn feed(app: &mut App, key: KeyEvent) -> SettingsOutcome {
    let Some(state) = app.settings.as_ref() else {
        return SettingsOutcome::Close;
    };
    let pending_delete = state.pending_delete.clone();
    let tab = state.tab;
    if app.oauth_flow.is_some() && key.code == KeyCode::Esc {
        super::oauthflow::cancel(app);
        return SettingsOutcome::Stay;
    }
    if let Some(name) = pending_delete {
        return feed_confirm_delete(app, key, &name);
    }
    match key.code {
        KeyCode::Esc => SettingsOutcome::Close,
        KeyCode::Tab => {
            switch_tab(app, SettingsTab::next);
            SettingsOutcome::Stay
        }
        KeyCode::BackTab => {
            switch_tab(app, SettingsTab::previous);
            SettingsOutcome::Stay
        }
        _ => match tab {
            SettingsTab::Accounts => feed_accounts(app, key),
            SettingsTab::Essentials => settingscmd::feed(app, key),
            SettingsTab::Folders => super::folders::feed(app, key),
        },
    }
}

fn switch_tab(app: &mut App, step: fn(SettingsTab) -> SettingsTab) {
    if let Some(state) = app.settings.as_mut() {
        state.tab = step(state.tab);
    }
}

fn feed_accounts(app: &mut App, key: KeyEvent) -> SettingsOutcome {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            move_account_selection(app, 1);
            SettingsOutcome::Stay
        }
        KeyCode::Char('k') | KeyCode::Up => {
            move_account_selection(app, -1);
            SettingsOutcome::Stay
        }
        KeyCode::Char('J') => {
            shift_account_order(app, 1);
            SettingsOutcome::Stay
        }
        KeyCode::Char('K') => {
            shift_account_order(app, -1);
            SettingsOutcome::Stay
        }
        KeyCode::Char('a') => {
            app.open_account_form_add();
            SettingsOutcome::Stay
        }
        KeyCode::Char('e') => {
            if let Some(name) = selected_account_name(app) {
                app.open_account_form_edit(&name);
            }
            SettingsOutcome::Stay
        }
        KeyCode::Char('d') => {
            arm_delete(app);
            SettingsOutcome::Stay
        }
        KeyCode::Char('o') => {
            if let Some(name) = selected_account_name(app) {
                super::oauthflow::authorise(app, &name);
            }
            SettingsOutcome::Stay
        }
        _ => SettingsOutcome::Stay,
    }
}

fn feed_confirm_delete(
    app: &mut App,
    key: KeyEvent,
    name: &str,
) -> SettingsOutcome {
    if matches!(key.code, KeyCode::Char('y' | 'Y')) {
        remove_account(app, name);
    } else if let Some(state) = app.settings.as_mut() {
        state.pending_delete = None;
    }
    SettingsOutcome::Stay
}

fn move_account_selection(app: &mut App, step: i32) {
    let Some(state) = app.settings.as_mut() else {
        return;
    };
    if state.accounts.is_empty() {
        return;
    }
    state.account_selected =
        wrapped(state.account_selected, state.accounts.len(), step);
}

const ACCOUNTS_TABLE: &str = "accounts";
const ORDER_KEY: &str = "order";

/// Shift+J/K move the selected account through the list and
/// persist the whole order to `[accounts] order`, so the new
/// order survives a restart and the first account becomes the
/// primary. The running session follows at once.
fn shift_account_order(app: &mut App, step: i32) {
    let Some(state) = app.settings.as_mut() else {
        return;
    };
    let from = state.account_selected;
    let last = state.accounts.len().saturating_sub(1) as i32;
    let to = (from as i32 + step).clamp(0, last) as usize;
    if state.accounts.is_empty() || to == from {
        return;
    }
    state.accounts.swap(from, to);
    state.account_selected = to;
    let order: Vec<String> = state
        .accounts
        .iter()
        .map(|account| account.name.clone())
        .collect();
    let value = super::configedit::toml_string_array(&order);
    let result =
        settingscmd::persist(app, ACCOUNTS_TABLE, ORDER_KEY, &value);
    reorder_live_accounts(app, &order);
    app.notice = Some(match result {
        Ok(()) => format!("account order: {}", order.join(", ")),
        Err(error) => format!("account order not saved: {error}"),
    });
}

/// Mirrors the persisted order onto the running session's
/// account list and sidebar, which every scope cycle, unified
/// query and refresh reads.
fn reorder_live_accounts(app: &mut App, order: &[String]) {
    let rank = |name: &str| {
        order
            .iter()
            .position(|entry| entry == name)
            .unwrap_or(order.len())
    };
    app.accounts.sort_by_key(|name| rank(name));
    app.account_entries.sort_by_key(|entry| rank(&entry.name));
    app.rebuild_sidebar();
}

fn selected_account_name(app: &App) -> Option<String> {
    let state = app.settings.as_ref()?;
    state
        .accounts
        .get(state.account_selected)
        .map(|account| account.name.clone())
}

fn arm_delete(app: &mut App) {
    let Some(name) = selected_account_name(app) else {
        return;
    };
    if let Some(state) = app.settings.as_mut() {
        state.pending_delete = Some(name);
    }
}

/// Removes the account's config file only: the mail store
/// under its maildir is left exactly as it was.
fn remove_account(app: &mut App, name: &str) {
    let path = app
        .dirs
        .config
        .join("accounts")
        .join(format!("{name}.toml"));
    app.notice = Some(match std::fs::remove_file(&path) {
        Ok(()) => match super::request_reload() {
            None => format!("removed account {name}"),
            Some(notice) => {
                format!("removed account {name} ({notice})")
            }
        },
        Err(error) => format!("remove {name}: {error}"),
    });
    if let Some(state) = app.settings.as_mut() {
        state.pending_delete = None;
    }
    app.refresh_settings_accounts();
}

/// `current` moved by `step`, wrapping within `[0, len)`; a
/// zero length always lands on 0.
pub(super) fn wrapped(current: usize, len: usize, step: i32) -> usize {
    if len == 0 {
        return 0;
    }
    let len = len as i32;
    let moved = ((current as i32 + step) % len + len) % len;
    moved as usize
}

fn account_summaries(dirs: &Dirs) -> Vec<AccountSummary> {
    let Ok(loaded) = antiphon_config::load(dirs) else {
        return Vec::new();
    };
    loaded.accounts.iter().map(summary_of).collect()
}

fn summary_of(entry: &NamedAccount) -> AccountSummary {
    AccountSummary {
        name: entry.file_stem.clone(),
        address: entry
            .account
            .identities
            .first()
            .map(|identity| identity.address.clone())
            .unwrap_or_default(),
        host: entry.account.imap.host.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::testkit::app_with_settings;
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(
            code,
            ratatui::crossterm::event::KeyModifiers::NONE,
        )
    }

    #[test]
    fn wrapped_wraps_both_directions_and_handles_zero() {
        assert_eq!(wrapped(0, 3, 1), 1);
        assert_eq!(wrapped(2, 3, 1), 0);
        assert_eq!(wrapped(0, 3, -1), 2);
        assert_eq!(wrapped(0, 0, 1), 0);
    }

    #[test]
    fn tab_switches_and_esc_closes() {
        let mut app = app_with_settings(&["work"]);
        assert_eq!(
            feed(&mut app, key(KeyCode::Tab)),
            SettingsOutcome::Stay
        );
        assert_eq!(
            app.settings.as_ref().unwrap().tab,
            SettingsTab::Essentials
        );
        assert_eq!(
            feed(&mut app, key(KeyCode::Esc)),
            SettingsOutcome::Close
        );
    }

    #[test]
    fn accounts_selection_moves_and_wraps() {
        let mut app = app_with_settings(&["a", "b", "c"]);
        feed(&mut app, key(KeyCode::Char('j')));
        assert_eq!(app.settings.as_ref().unwrap().account_selected, 1);
        feed(&mut app, key(KeyCode::Char('k')));
        assert_eq!(app.settings.as_ref().unwrap().account_selected, 0);
        feed(&mut app, key(KeyCode::Char('k')));
        assert_eq!(app.settings.as_ref().unwrap().account_selected, 2);
    }

    #[test]
    fn shift_j_and_k_reorder_accounts_and_persist_the_order() {
        use super::super::testkit::TempDir;

        let dir = TempDir::new();
        let mut app = app_with_settings(&["a", "b", "c"]);
        app.accounts = vec!["a".into(), "b".into(), "c".into()];
        app.config_path = dir.path.join("config.toml");

        feed(&mut app, key(KeyCode::Char('J')));
        let state = app.settings.as_ref().unwrap();
        let names: Vec<&str> = state
            .accounts
            .iter()
            .map(|account| account.name.as_str())
            .collect();
        assert_eq!(names, ["b", "a", "c"]);
        assert_eq!(state.account_selected, 1);
        assert_eq!(app.accounts, ["b", "a", "c"]);
        let text = std::fs::read_to_string(&app.config_path).unwrap();
        assert!(text.contains("[accounts]"), "{text}");
        assert!(
            text.contains("order = [\"b\", \"a\", \"c\"]"),
            "{text}"
        );

        app.settings.as_mut().unwrap().account_selected = 0;
        feed(&mut app, key(KeyCode::Char('K')));
        assert_eq!(
            app.settings.as_ref().unwrap().account_selected,
            0,
            "moving up from the top clamps"
        );
        assert_eq!(app.accounts, ["b", "a", "c"]);
    }

    #[test]
    fn a_opens_a_blank_form_and_e_opens_it_prefilled() {
        use super::super::testkit::TempDir;

        let dir = TempDir::new();
        let accounts_dir = dir.path.join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();
        std::fs::write(
            accounts_dir.join("work.toml"),
            "[account]\nname = \"work\"\n\n\
             [imap]\nhost = \"imap.example.com\"\nuser = \"u\"\n\n\
             [[identity]]\naddress = \"u@example.com\"\n",
        )
        .unwrap();

        let mut app = app_with_settings(&["work"]);
        app.dirs.config = dir.path.clone();

        assert_eq!(
            feed(&mut app, key(KeyCode::Char('a'))),
            SettingsOutcome::Stay
        );
        assert!(app.account_form.is_some());
        app.account_form = None;

        feed(&mut app, key(KeyCode::Char('e')));
        let form =
            app.account_form.as_ref().expect("edit opens a form");
        assert_eq!(form.editing.as_deref(), Some("work"));
    }

    #[test]
    fn esc_cancels_a_running_sign_in_instead_of_closing() {
        let mut app = app_with_settings(&["work"]);
        app.oauth_flow =
            Some(super::super::oauthflow::test_flow("work"));
        assert_eq!(
            feed(&mut app, key(KeyCode::Esc)),
            SettingsOutcome::Stay
        );
        assert!(app.oauth_flow.is_none());
        assert!(app.settings.is_some(), "settings stay open");
        assert_eq!(
            feed(&mut app, key(KeyCode::Esc)),
            SettingsOutcome::Close,
            "the next esc closes as usual"
        );
    }

    #[test]
    fn d_asks_for_confirmation_before_removing() {
        let mut app = app_with_settings(&["work"]);
        feed(&mut app, key(KeyCode::Char('d')));
        assert_eq!(
            app.settings.as_ref().unwrap().pending_delete.as_deref(),
            Some("work")
        );
        feed(&mut app, key(KeyCode::Char('n')));
        assert!(
            app.settings.as_ref().unwrap().pending_delete.is_none()
        );
        assert_eq!(
            app.settings.as_ref().unwrap().accounts.len(),
            1,
            "declining leaves the account alone"
        );
    }

    #[test]
    fn confirming_removal_deletes_only_the_config_file() {
        use super::super::testkit::TempDir;

        let dir = TempDir::new();
        let accounts_dir = dir.path.join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();
        std::fs::write(
            accounts_dir.join("work.toml"),
            "[account]\nname = \"work\"\n\n\
             [imap]\nhost = \"h\"\nuser = \"u\"\n",
        )
        .unwrap();
        let maildir = dir.path.join("maildir/work/cur/1.eml");
        std::fs::create_dir_all(maildir.parent().unwrap()).unwrap();
        std::fs::write(&maildir, "kept").unwrap();

        let mut app = app_with_settings(&["work"]);
        app.dirs.config = dir.path.clone();
        feed(&mut app, key(KeyCode::Char('d')));
        feed(&mut app, key(KeyCode::Char('y')));

        assert!(!accounts_dir.join("work.toml").exists());
        assert!(maildir.exists(), "the mail store is never touched");
        assert!(app.settings.as_ref().unwrap().accounts.is_empty());
    }
}
