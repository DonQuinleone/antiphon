mod accounts;
mod cmd;
mod cmd_rows;
mod draw;

pub(super) use draw::{draw_alias_modal, draw_settings};

use antiphon_core::Action;
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use crate::tui::app::{App, View};

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

/// What the row shows in the server column. OAuth providers
/// name themselves; an IMAP account shows its host. The
/// auto-filled provider hosts are long and read-together with
/// the address, so a short provider label stands in their place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ServerKind {
    Imap,
    Microsoft,
    Google,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AccountSummary {
    pub(super) name: String,
    /// The `[account] name`, which keys the stored grants;
    /// usually the file stem, but not necessarily.
    pub(super) account_name: String,
    pub(super) address: String,
    pub(super) host: String,
    pub(super) kind: ServerKind,
    pub(super) oauth: Option<crate::tui::oauth_status::OauthInfo>,
}

impl AccountSummary {
    pub(super) fn server_label(&self) -> &str {
        match self.kind {
            ServerKind::Microsoft => "MS365",
            ServerKind::Google => "Google",
            ServerKind::Imap => &self.host,
        }
    }
}

pub(super) struct SettingsState {
    pub(super) tab: SettingsTab,
    pub(super) accounts: Vec<AccountSummary>,
    pub(super) account_selected: usize,
    pub(super) pending_delete: Option<String>,
    pub(super) pending_revoke: Option<String>,
    pub(super) essentials_selected: usize,
    pub(super) daemon_hint: Option<String>,
    pub(super) folders: Vec<crate::tui::folders::FolderRow>,
    pub(super) folder_selected: usize,
}

impl App {
    pub(super) fn open_settings(&mut self) {
        // One status poll per settings open keeps the daemon's
        // auth-failure report current without chatter.
        crate::tui::oauth_status::refresh_auth_failures(self);
        let folders = self.folder_rows();
        self.settings = Some(SettingsState {
            tab: SettingsTab::Accounts,
            accounts: accounts::account_summaries(
                &self.dirs,
                &self.auth_failures,
            ),
            account_selected: 0,
            pending_delete: None,
            pending_revoke: None,
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
        // The scope/tab list is the ordered account names; keep it
        // in step so a just-added account is switchable at once
        // rather than reported missing until the next launch.
        if let Ok(loaded) = antiphon_config::load(&self.dirs) {
            self.accounts = super::actions::account_names(&loaded);
            // A just-added account must join the sidebar's account
            // list, or the periodic folder discovery (which only
            // walks these entries) never finds it and it stays
            // invisible, and so unsyncable, until a restart. Keep
            // the folders already discovered for existing accounts.
            self.account_entries = super::prefs::account_seeds(&loaded)
                .into_iter()
                .map(|seed| super::sidebar::AccountEntry {
                    folders: self
                        .account_entries
                        .iter()
                        .find(|entry| entry.name == seed.name)
                        .map(|entry| entry.folders.clone())
                        .unwrap_or_default(),
                    ..seed
                })
                .collect();
        }
        let accounts = accounts::account_summaries(
            &self.dirs,
            &self.auth_failures,
        );
        let Some(state) = self.settings.as_mut() else {
            return;
        };
        let last = accounts.len().saturating_sub(1);
        state.accounts = accounts;
        state.account_selected = state.account_selected.min(last);
    }
}

/// Raw-key handling that precedes the keymap: cancel a running
/// sign-in on esc, and the delete/revoke y/n confirmations.
/// Returns true when it consumed the key.
pub(super) fn feed_modal(app: &mut App, key: KeyEvent) -> bool {
    if app.oauth_flow.is_some() && key.code == KeyCode::Esc {
        crate::tui::oauthflow::cancel(app);
        return true;
    }
    let pending_delete = app
        .settings
        .as_ref()
        .and_then(|state| state.pending_delete.clone());
    if let Some(name) = pending_delete {
        accounts::feed_confirm_delete(app, key, &name);
        return true;
    }
    let pending_revoke = app
        .settings
        .as_ref()
        .and_then(|state| state.pending_revoke.clone());
    if let Some(name) = pending_revoke {
        crate::tui::oauth_status::feed_confirm_revoke(app, key, &name);
        return true;
    }
    false
}

/// A resolved settings action: switch tabs, close the view, or
/// hand off to whichever tab is active.
pub(super) fn dispatch(app: &mut App, action: Action) {
    match action {
        Action::NextTab => switch_tab(app, SettingsTab::next),
        Action::PrevTab => switch_tab(app, SettingsTab::previous),
        Action::SettingsClose => {
            app.settings = None;
            app.view = View::List;
        }
        _ => {
            let Some(tab) =
                app.settings.as_ref().map(|state| state.tab)
            else {
                return;
            };
            match tab {
                SettingsTab::Accounts => accounts::apply(app, action),
                SettingsTab::Essentials => cmd::apply(app, action),
                SettingsTab::Folders => {
                    crate::tui::folders::apply(app, action)
                }
            }
        }
    }
}

fn switch_tab(app: &mut App, step: fn(SettingsTab) -> SettingsTab) {
    if let Some(state) = app.settings.as_mut() {
        state.tab = step(state.tab);
    }
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

#[cfg(test)]
mod tests {
    use antiphon_core::Action;

    use crate::tui::settings::*;
    use crate::tui::testkit::app_with_settings;

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

    fn write_account(config: &std::path::Path, name: &str) {
        let accounts = config.join("accounts");
        std::fs::create_dir_all(&accounts).unwrap();
        std::fs::write(
            accounts.join(format!("{name}.toml")),
            format!(
                "[account]\nname = \"{name}\"\n\n\
                 [imap]\nhost = \"h\"\nuser = \"u\"\n\n\
                 [smtp]\nhost = \"s\"\n"
            ),
        )
        .unwrap();
    }

    #[test]
    fn a_newly_added_account_joins_the_sidebar_entries() {
        use crate::tui::sidebar::AccountEntry;
        use crate::tui::testkit::{TempDir, app_with_messages};

        let dir = TempDir::new();
        write_account(&dir.path, "work");
        write_account(&dir.path, "home");
        let mut app = app_with_messages(1);
        app.dirs.config = dir.path.clone();
        app.account_entries = vec![AccountEntry {
            name: "work".to_string(),
            folders: vec!["archive".to_string()],
            order: Vec::new(),
            hidden: Vec::new(),
            unsynced: Vec::new(),
        }];
        app.refresh_settings_accounts();
        let names: Vec<&str> = app
            .account_entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert!(
            names.contains(&"home"),
            "the added account joins the sidebar: {names:?}"
        );
        let work = app
            .account_entries
            .iter()
            .find(|entry| entry.name == "work")
            .expect("work stays");
        assert_eq!(
            work.folders,
            vec!["archive".to_string()],
            "an existing account keeps its discovered folders"
        );
    }

    #[test]
    fn tab_switches_and_closes() {
        let mut app = app_with_settings(&["work"]);
        dispatch(&mut app, Action::NextTab);
        assert_eq!(
            app.settings.as_ref().unwrap().tab,
            SettingsTab::Essentials
        );
        dispatch(&mut app, Action::SettingsClose);
        assert!(app.settings.is_none());
    }

    #[test]
    fn esc_cancels_a_running_sign_in() {
        let mut app = app_with_settings(&["work"]);
        app.oauth_flow = Some(crate::tui::oauthflow::test_flow("work"));
        assert!(feed_modal(&mut app, key(KeyCode::Esc)));
        assert!(app.oauth_flow.is_none());
        assert!(app.settings.is_some(), "settings stay open");
    }
}
