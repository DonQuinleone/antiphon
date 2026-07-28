use antiphon_config::{Loaded, ReadingPane};
use antiphon_core::Action;

pub(super) use super::mailops::{OpIntent, account_of, folder_of};

use super::app::App;
use super::commands::PromptKind;
use super::scope::{self, ViewScope};
use super::sidebar::{self, SidebarEntry};

const HALF_PAGE_ROWS: usize = 10;

pub fn account_names(loaded: &Loaded) -> Vec<String> {
    loaded
        .accounts
        .iter()
        .map(|entry| entry.account.account.name.clone())
        .collect()
}

const THREAD_LABEL: &str = "thread";
impl App {
    pub(super) fn apply_in_list(&mut self, action: Action) {
        match action {
            Action::MoveDown => self.select_forward(1),
            Action::MoveUp => self.select_back(1),
            Action::PaneScrollDown => {
                self.preview_scroll =
                    self.preview_scroll.saturating_add(1)
            }
            Action::PaneScrollUp => {
                self.preview_scroll =
                    self.preview_scroll.saturating_sub(1)
            }
            Action::HalfPageDown => self.select_forward(HALF_PAGE_ROWS),
            Action::HalfPageUp => self.select_back(HALF_PAGE_ROWS),
            Action::Top => self.selected = 0,
            Action::Bottom => self.selected = self.last_index(),
            Action::ToggleSidebar => self.sidebar = !self.sidebar,
            Action::ToggleHeaders => {
                self.headers_all = !self.headers_all
            }
            Action::OpenLink | Action::Attachments => {
                self.notice = Some("open a message first".to_string())
            }
            Action::NextAccount => self.shift_scope(scope::next_scope),
            Action::PreviousAccount => {
                self.shift_scope(scope::previous_scope)
            }
            Action::AccountTab(tab) => self.open_account_tab(tab),
            Action::AccountUnified => self.open_unified_tab(),
            Action::SidebarNext => {
                self.sidebar_selected = sidebar::next_index(
                    self.sidebar_selected,
                    self.sidebar_entries.len(),
                )
            }
            Action::SidebarPrevious => {
                self.sidebar_selected = sidebar::previous_index(
                    self.sidebar_selected,
                    self.sidebar_entries.len(),
                )
            }
            Action::SidebarOpen => self.sidebar_open(),
            Action::CycleReadingPane => self.cycle_reading_pane(),
            Action::Search => self.open_prompt(PromptKind::Search),
            Action::Command => self.open_prompt(PromptKind::Command),
            Action::ToggleRead => self.toggle_read(),
            Action::ToggleFlagged => self.toggle_flagged(),
            Action::DeleteMessage => self.delete_selected(),
            Action::Archive => self.archive_selected(),
            Action::MoveTo => self.open_folder_picker(),
            Action::ThreadView => self.open_thread(),
            Action::Back => self.close_thread(),
            Action::Quit => self.quit = true,
            _ => self.not_built_notice(),
        }
    }

    /// The list stays flat; T pivots it onto the selected
    /// message's whole thread, and back restores the listing
    /// it came from.
    fn open_thread(&mut self) {
        let Some(message) = self.selected_message() else {
            return;
        };
        let thread = message.thread_id.clone();
        if thread.is_empty() {
            self.notice = Some("no thread for this message".into());
            return;
        }
        if self.thread_return.is_none() {
            self.thread_return = Some((
                self.current_query.clone(),
                self.active_search.clone(),
            ));
        }
        self.current_query = format!("thread:{thread}");
        self.active_search = Some(THREAD_LABEL.to_string());
        self.requery = true;
    }

    fn close_thread(&mut self) {
        let Some((query, search)) = self.thread_return.take() else {
            return;
        };
        self.current_query = query;
        self.active_search = search;
        self.requery = true;
    }

    fn shift_scope(
        &mut self,
        step: fn(&ViewScope, &[String]) -> ViewScope,
    ) {
        self.switch_scope(step(&self.scope, &self.accounts));
    }

    fn sidebar_open(&mut self) {
        self.thread_return = None;
        let Some(entry) =
            self.sidebar_entries.get(self.sidebar_selected)
        else {
            return;
        };
        match entry.clone() {
            SidebarEntry::Unified => self.scope = ViewScope::Unified,
            SidebarEntry::Account(account) => {
                self.scope = ViewScope::Account(account)
            }
            SidebarEntry::Folder {
                account,
                name,
                query,
                ..
            } => {
                let label = self
                    .alias_for(&account, &name)
                    .unwrap_or(&name)
                    .to_string();
                self.scope = ViewScope::Account(account);
                self.current_query = query;
                self.active_search = Some(label);
            }
            SidebarEntry::Saved { name, query } => {
                self.current_query = query;
                self.active_search = Some(name);
            }
        }
        self.requery = true;
        self.sync_tab_sidebar();
    }

    fn select_forward(&mut self, rows: usize) {
        self.selected = (self.selected + rows).min(self.last_index());
    }

    fn select_back(&mut self, rows: usize) {
        self.selected = self.selected.saturating_sub(rows);
    }

    pub(super) fn last_index(&self) -> usize {
        self.messages.len().saturating_sub(1)
    }

    fn cycle_reading_pane(&mut self) {
        self.reading_pane = match self.reading_pane {
            ReadingPane::Below => ReadingPane::Right,
            ReadingPane::Right => ReadingPane::Off,
            ReadingPane::Off => ReadingPane::Below,
        };
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn t_pivots_to_the_thread_and_back_restores() {
        let mut app = app_with_messages(2);
        app.messages[0].thread_id = "th7".to_string();
        app.current_query = "tag:inbox".to_string();
        app.active_search = Some("inbox".to_string());

        app.apply_in_list(Action::ThreadView);
        assert_eq!(app.current_query, "thread:th7");
        assert_eq!(app.active_search.as_deref(), Some("thread"));
        assert!(app.take_requery());

        app.apply_in_list(Action::Back);
        assert_eq!(app.current_query, "tag:inbox");
        assert_eq!(app.active_search.as_deref(), Some("inbox"));
        assert!(app.take_requery());
        app.apply_in_list(Action::Back);
        assert!(!app.take_requery(), "back is idle with no thread");
    }

    #[test]
    fn a_threadless_message_never_pivots() {
        let mut app = app_with_messages(1);
        app.apply_in_list(Action::ThreadView);
        assert!(app.thread_return.is_none());
        assert!(!app.take_requery());
    }
    use super::super::app::DEFAULT_QUERY;
    use super::super::testkit::{
        app_with_accounts, app_with_folders, app_with_messages,
    };
    use super::*;

    #[test]
    fn movement_clamps_at_both_ends() {
        let mut app = app_with_messages(3);
        app.apply(Action::MoveUp);
        assert_eq!(app.selected, 0);
        app.apply(Action::Bottom);
        assert_eq!(app.selected, 2);
        app.apply(Action::MoveDown);
        assert_eq!(app.selected, 2);
        app.apply(Action::HalfPageUp);
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn half_page_moves_by_the_constant() {
        let mut app = app_with_messages(30);
        app.apply(Action::HalfPageDown);
        assert_eq!(app.selected, HALF_PAGE_ROWS);
    }

    #[test]
    fn reading_pane_cycles_through_all_three() {
        let mut app = app_with_messages(1);
        app.apply(Action::CycleReadingPane);
        assert_eq!(app.reading_pane, ReadingPane::Right);
        app.apply(Action::CycleReadingPane);
        assert_eq!(app.reading_pane, ReadingPane::Off);
        app.apply(Action::CycleReadingPane);
        assert_eq!(app.reading_pane, ReadingPane::Below);
    }

    #[test]
    fn unhandled_actions_leave_a_notice_and_quit_quits() {
        let mut app = app_with_messages(1);
        app.apply(Action::OpenLink);
        assert!(app.notice.is_some());
        app.apply(Action::Quit);
        assert!(app.quit);
    }

    #[test]
    fn gt_cycles_unified_through_accounts_and_back() {
        let mut app = app_with_accounts(&["a", "b"]);
        app.apply(Action::NextAccount);
        assert_eq!(app.scope, ViewScope::Account("a".into()));
        assert!(app.take_requery());
        app.apply(Action::NextAccount);
        assert_eq!(app.scope, ViewScope::Account("b".into()));
        app.apply(Action::NextAccount);
        assert_eq!(app.scope, ViewScope::Unified);
        app.apply(Action::PreviousAccount);
        assert_eq!(app.scope, ViewScope::Account("b".into()));
        assert!(app.take_requery());
        assert!(!app.take_requery());
    }

    #[test]
    fn sidebar_moves_in_entry_order_without_querying() {
        let mut app = app_with_accounts(&["a"]);
        app.apply(Action::SidebarNext);
        app.apply(Action::SidebarNext);
        assert_eq!(app.sidebar_selected, 2);
        assert!(!app.take_requery());
        app.apply(Action::SidebarPrevious);
        assert_eq!(app.sidebar_selected, 1);
    }

    #[test]
    fn opening_an_account_entry_sets_the_scope() {
        let mut app = app_with_accounts(&["a", "b"]);
        let position = app
            .sidebar_entries
            .iter()
            .position(|entry| {
                entry == &SidebarEntry::Account("b".into())
            })
            .expect("account b entry");
        app.sidebar_selected = position;
        app.apply(Action::SidebarOpen);
        assert_eq!(app.scope, ViewScope::Account("b".into()));
        assert!(app.take_requery());
        assert_eq!(app.current_query, DEFAULT_QUERY);
        assert!(app.active_search.is_none());
    }

    #[test]
    fn opening_a_folder_scopes_its_account_and_queries_it() {
        let mut app = app_with_folders(&[
            ("a", &[][..]),
            ("b", &["archive"][..]),
        ]);
        let cases = [
            ("b", "archive", "path:\"b/archive/**\""),
            ("a", "inbox", "path:\"a/cur\" or path:\"a/new\""),
        ];
        for (account, folder, query) in cases {
            let position = app
                .sidebar_entries
                .iter()
                .position(|entry| match entry {
                    SidebarEntry::Folder {
                        account: entry_account,
                        name,
                        ..
                    } => entry_account == account && name == folder,
                    _ => false,
                })
                .expect("folder entry");
            app.sidebar_selected = position;
            app.apply(Action::SidebarOpen);
            assert_eq!(
                app.scope,
                ViewScope::Account(account.into()),
                "{folder}"
            );
            assert_eq!(app.current_query, query, "{folder}");
            assert_eq!(
                app.active_search.as_deref(),
                Some(folder),
                "{folder}"
            );
            assert!(app.take_requery(), "{folder}");
            let scoped = app.scoped(&app.current_query).unwrap();
            assert!(scoped.contains(query), "{folder}: {scoped}");
        }
    }

    #[test]
    fn opening_a_saved_search_keeps_scope_and_names_it() {
        let mut app = app_with_accounts(&["a"]);
        app.scope = ViewScope::Account("a".into());
        let unread = app
            .sidebar_entries
            .iter()
            .position(|entry| entry.label() == "unread")
            .expect("built-in unread entry");
        app.sidebar_selected = unread;
        app.apply(Action::SidebarOpen);
        assert_eq!(app.current_query, "tag:unread");
        assert_eq!(app.active_search.as_deref(), Some("unread"));
        assert_eq!(app.scope, ViewScope::Account("a".into()));
        assert!(app.take_requery());
    }

    #[test]
    fn empty_list_never_panics() {
        let mut app = app_with_messages(0);
        app.apply(Action::Bottom);
        app.apply(Action::MoveDown);
        assert_eq!(app.selected, 0);
        assert!(app.selected_message().is_none());
    }
}
