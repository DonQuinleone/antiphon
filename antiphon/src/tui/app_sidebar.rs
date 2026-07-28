//! The App's sidebar state: how the entry list is built for
//! the current accounts-bar mode and refreshed in place when
//! settings or syncs change it.

use antiphon_config::{AccountsBar, Loaded};

use super::app::App;
use super::scope::ViewScope;
use super::sidebar::{self, SidebarEntry};

/// Tabs mode starts on the primary account's tab (the first
/// account in order); sidebar mode keeps the unified start.
pub(super) fn initial_scope(
    loaded: &Loaded,
    accounts: &[String],
) -> ViewScope {
    match (loaded.config.ui.accounts_bar, accounts.first()) {
        (AccountsBar::Tabs, Some(first)) => {
            ViewScope::Account(first.clone())
        }
        _ => ViewScope::Unified,
    }
}

impl App {
    /// Rebuilds the sidebar from the in-memory account entries
    /// (after a settings edit, say), carrying the unread counts
    /// over so the rebuild does not blank them until the next
    /// periodic refresh.
    pub(super) fn rebuild_sidebar(&mut self) {
        let mut entries = self.build_sidebar_entries();
        let counts: Vec<(String, u32)> = self
            .sidebar_entries
            .iter()
            .filter_map(|entry| match entry {
                SidebarEntry::Folder { query, unread, .. } => {
                    Some((query.clone(), *unread))
                }
                _ => None,
            })
            .collect();
        sidebar::fill_unread(&mut entries, |query| {
            counts
                .iter()
                .find(|(known, _)| known == query)
                .map(|(_, unread)| *unread)
        });
        self.update_sidebar(entries);
    }

    /// The sidebar entry list for the current mode: the full
    /// account tree in sidebar mode, only the active account
    /// (plus searches) in tabs mode.
    pub(super) fn build_sidebar_entries(&self) -> Vec<SidebarEntry> {
        match self.accounts_bar {
            AccountsBar::Sidebar => sidebar::entries(
                &self.account_entries,
                &self.saved_searches,
            ),
            AccountsBar::Tabs => sidebar::tab_entries(
                &self.account_entries,
                &self.saved_searches,
                match &self.scope {
                    ViewScope::Unified => None,
                    ViewScope::Account(account) => Some(account),
                },
            ),
        }
    }

    /// Folders come and go as syncs land; the highlight is
    /// clamped rather than reset so it never dangles.
    pub fn update_sidebar(&mut self, entries: Vec<SidebarEntry>) {
        if entries == self.sidebar_entries {
            return;
        }
        let last = entries.len().saturating_sub(1);
        self.sidebar_entries = entries;
        self.sidebar_selected = self.sidebar_selected.min(last);
    }
}
