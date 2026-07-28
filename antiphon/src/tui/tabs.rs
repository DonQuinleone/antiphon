//! The tabbed accounts mode (`ui.accounts_bar = "tabs"`): the
//! sidebar shows only the active account's folders and the
//! status bar names it, driven by the g1..g9/gu jumps that also
//! work in sidebar mode.

use antiphon_config::AccountsBar;

use super::app::App;
use super::scope::ViewScope;

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

    /// The one path every scope change takes: the current
    /// query re-runs under the new scope, and in tabs mode
    /// the sidebar follows the active tab.
    pub(super) fn switch_scope(&mut self, scope: ViewScope) {
        self.scope = scope;
        self.thread_return = None;
        self.requery = true;
        self.sync_tab_sidebar();
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
