//! The tabbed accounts mode (`ui.accounts_bar = "tabs"`): the
//! sidebar shows only the active account's folders and the
//! status bar names it, driven by the g1..g9/gu jumps that also
//! work in sidebar mode.

use antiphon_config::AccountsBar;

use super::app::App;
use super::scope::ViewScope;
use super::sidebar::{self, SidebarEntry};

impl App {
    /// g1..g9 jump straight to that account's scope; a number
    /// with no account says so instead of doing nothing.
    pub(super) fn open_account_tab(&mut self, tab: u8) {
        let index = usize::from(tab).saturating_sub(1);
        let Some(account) = self.accounts.get(index).cloned() else {
            self.notice = Some(format!("no account {tab}"));
            return;
        };
        self.switch_scope(ViewScope::Account(account));
    }

    pub(super) fn open_unified_tab(&mut self) {
        self.switch_scope(ViewScope::Unified);
    }

    /// The one path every scope change takes. Switching to an
    /// account opens that account's inbox, so a jump lands in the
    /// mail rather than re-running the previous query under the
    /// new scope; the unified tab keeps the current query.
    pub(super) fn switch_scope(&mut self, scope: ViewScope) {
        self.thread_return = None;
        self.scope = scope;
        self.sync_tab_sidebar();
        let account = match &self.scope {
            ViewScope::Account(account) => Some(account.clone()),
            ViewScope::Unified => None,
        };
        match account {
            Some(account) if self.select_account_inbox(&account) => {
                self.sidebar_open();
            }
            Some(account) => {
                // The account exists but has no synced folders yet,
                // so there is no inbox to open; say so plainly
                // instead of leaving a blank, misread view.
                self.notice = Some(format!(
                    "{account} has not synced yet; press s to sync"
                ));
                self.requery = true;
            }
            None => self.requery = true,
        }
    }

    /// Points the sidebar highlight at the account's inbox entry,
    /// so `sidebar_open` opens it. False when the account has no
    /// inbox row yet (never synced), leaving the caller to fall
    /// back to a plain requery.
    fn select_account_inbox(&mut self, account: &str) -> bool {
        let found = self.sidebar_entries.iter().position(|entry| {
            matches!(
                entry,
                SidebarEntry::Folder { account: acc, name, .. }
                    if acc.as_str() == account
                        && name.as_str() == sidebar::INBOX_LABEL
            )
        });
        match found {
            Some(index) => {
                self.sidebar_selected = index;
                true
            }
            None => false,
        }
    }

    /// In tabs mode the sidebar lists only the active
    /// account's folders, so a scope change rebuilds it; the
    /// full sidebar never changes with scope.
    pub(super) fn sync_tab_sidebar(&mut self) {
        if self.accounts_bar == AccountsBar::Tabs {
            self.rebuild_sidebar();
        }
    }
}

#[cfg(test)]
mod tests {
    use antiphon_core::Action;

    use super::super::sidebar::SidebarEntry;
    use super::super::testkit::app_with_folders;
    use super::*;

    fn tabbed_app() -> App {
        let mut app = app_with_folders(&[
            ("a", &["archive"][..]),
            ("b", &["lists"][..]),
        ]);
        app.accounts_bar = AccountsBar::Tabs;
        app
    }

    fn labels(app: &App) -> Vec<String> {
        app.sidebar_entries
            .iter()
            .map(|entry| entry.label().to_string())
            .collect()
    }

    #[test]
    fn switching_to_an_account_opens_its_inbox() {
        let mut app = tabbed_app();
        app.active_search = Some("unread".to_string());
        app.apply(Action::AccountTab(2));
        assert_eq!(app.scope, ViewScope::Account("b".into()));
        let selected = &app.sidebar_entries[app.sidebar_selected];
        assert!(
            matches!(
                selected,
                SidebarEntry::Folder { account, name, .. }
                    if account == "b" && name == super::sidebar::INBOX_LABEL
            ),
            "the switch lands the highlight on account b's inbox",
        );
        assert_eq!(
            app.active_search.as_deref(),
            Some("inbox"),
            "and opens it rather than keeping the prior search",
        );
        assert!(app.take_requery());
    }

    #[test]
    fn switching_to_an_unsynced_account_says_so() {
        let mut app = tabbed_app();
        app.accounts.push("c".to_string());
        app.apply(Action::AccountTab(3));
        assert_eq!(app.scope, ViewScope::Account("c".into()));
        assert_eq!(
            app.notice.as_deref(),
            Some("c has not synced yet; press s to sync"),
            "an unsynced account lands with an accurate notice",
        );
    }

    #[test]
    fn g_number_jumps_to_the_account_and_requeries() {
        let mut app = tabbed_app();
        app.apply(Action::AccountTab(2));
        assert_eq!(app.scope, ViewScope::Account("b".into()));
        assert!(app.take_requery());
        assert_eq!(
            labels(&app),
            ["inbox", "lists", "all", "inbox", "unread", "flagged"],
            "only the active account's folders, then searches"
        );
        assert!(
            !app.sidebar_entries
                .iter()
                .any(|entry| entry == &SidebarEntry::Unified),
        );

        app.apply(Action::AccountUnified);
        assert_eq!(app.scope, ViewScope::Unified);
        assert!(app.take_requery());
        assert_eq!(
            labels(&app),
            ["all", "inbox", "unread", "flagged"],
            "the unified tab keeps only the searches"
        );
    }

    #[test]
    fn a_number_with_no_account_notices_and_stays() {
        let mut app = tabbed_app();
        app.apply(Action::AccountTab(7));
        assert_eq!(app.scope, ViewScope::Unified);
        assert!(!app.take_requery());
        assert_eq!(app.notice.as_deref(), Some("no account 7"));
    }

    #[test]
    fn the_jumps_switch_scope_in_sidebar_mode_too() {
        let mut app = app_with_folders(&[("a", &[][..])]);
        app.apply(Action::AccountTab(1));
        assert_eq!(app.scope, ViewScope::Account("a".into()));
        assert!(app.take_requery());
        assert!(
            app.sidebar_entries
                .iter()
                .any(|entry| entry == &SidebarEntry::Unified),
            "sidebar mode keeps the full tree"
        );
    }

    #[test]
    fn cycling_follows_the_tabs_in_tabs_mode() {
        let mut app = tabbed_app();
        app.apply(Action::NextAccount);
        assert_eq!(app.scope, ViewScope::Account("a".into()));
        assert_eq!(
            labels(&app),
            ["inbox", "archive", "all", "inbox", "unread", "flagged"],
        );
    }
}
