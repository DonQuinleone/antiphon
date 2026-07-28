use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::app::{App, View};
use super::settings_accounts;
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
            accounts: settings_accounts::account_summaries(&self.dirs),
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
        let accounts = settings_accounts::account_summaries(&self.dirs);
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
        return settings_accounts::feed_confirm_delete(app, key, &name);
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
            SettingsTab::Accounts => {
                settings_accounts::feed_accounts(app, key)
            }
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
}
