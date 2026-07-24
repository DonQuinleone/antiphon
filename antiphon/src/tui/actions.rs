use antiphon_config::{Loaded, ReadingPane};
use antiphon_core::Action;

use super::app::App;
use super::commands::PromptKind;
use super::scope::{self, ViewScope};
use super::sidebar::{self, SidebarEntry};

const HALF_PAGE_ROWS: usize = 10;

const UNREAD_TAG: &str = "unread";
const FLAGGED_TAG: &str = "flagged";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpIntent {
    Flag {
        account: String,
        message_id: String,
        add: Vec<String>,
        remove: Vec<String>,
    },
    Delete {
        account: String,
        message_id: String,
    },
}

pub fn account_of(path: &std::path::Path) -> String {
    let mut components = path.components();
    for component in components.by_ref() {
        if component.as_os_str() == "maildir" {
            break;
        }
    }
    components
        .next()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn account_names(loaded: &Loaded) -> Vec<String> {
    loaded
        .accounts
        .iter()
        .map(|entry| entry.account.account.name.clone())
        .collect()
}

impl App {
    pub(super) fn apply_in_list(&mut self, action: Action) {
        match action {
            Action::MoveDown => self.select_forward(1),
            Action::MoveUp => self.select_back(1),
            Action::HalfPageDown => self.select_forward(HALF_PAGE_ROWS),
            Action::HalfPageUp => self.select_back(HALF_PAGE_ROWS),
            Action::Top => self.selected = 0,
            Action::Bottom => self.selected = self.last_index(),
            Action::ToggleSidebar => self.sidebar = !self.sidebar,
            Action::NextAccount => self.shift_scope(scope::next_scope),
            Action::PreviousAccount => {
                self.shift_scope(scope::previous_scope)
            }
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
            Action::MarkRead => self.set_unread(false),
            Action::MarkUnread => self.set_unread(true),
            Action::ToggleFlagged => self.toggle_flagged(),
            Action::DeleteMessage => self.delete_selected(),
            Action::Quit => self.quit = true,
            _ => self.not_built_notice(),
        }
    }

    fn shift_scope(
        &mut self,
        step: fn(&ViewScope, &[String]) -> ViewScope,
    ) {
        self.scope = step(&self.scope, &self.accounts);
        self.requery = true;
    }

    fn sidebar_open(&mut self) {
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
            } => {
                self.scope = ViewScope::Account(account);
                self.current_query = query;
                self.active_search = Some(name);
            }
            SidebarEntry::Saved { name, query } => {
                self.current_query = query;
                self.active_search = Some(name);
            }
        }
        self.requery = true;
    }

    pub(super) fn set_unread(&mut self, unread: bool) {
        let Some(message) = self.messages.get_mut(self.selected) else {
            return;
        };
        if message.unread == unread {
            return;
        }
        message.unread = unread;
        let tag = UNREAD_TAG.to_string();
        let (add, remove) = if unread {
            message.tags.push(tag.clone());
            (vec![tag], Vec::new())
        } else {
            message.tags.retain(|t| t != UNREAD_TAG);
            (Vec::new(), vec![tag])
        };
        self.pending_ops.push(OpIntent::Flag {
            account: account_of(&message.path),
            message_id: message.id.clone(),
            add,
            remove,
        });
    }

    fn toggle_flagged(&mut self) {
        let Some(message) = self.messages.get_mut(self.selected) else {
            return;
        };
        let tag = FLAGGED_TAG.to_string();
        let flagged = message.tags.iter().any(|t| t == FLAGGED_TAG);
        let (add, remove) = if flagged {
            message.tags.retain(|t| t != FLAGGED_TAG);
            (Vec::new(), vec![tag])
        } else {
            message.tags.push(tag.clone());
            (vec![tag], Vec::new())
        };
        self.pending_ops.push(OpIntent::Flag {
            account: account_of(&message.path),
            message_id: message.id.clone(),
            add,
            remove,
        });
    }

    fn delete_selected(&mut self) {
        if self.selected >= self.messages.len() {
            return;
        }
        let message = self.messages.remove(self.selected);
        self.total_messages = self.total_messages.saturating_sub(1);
        self.pending_ops.push(OpIntent::Delete {
            account: account_of(&message.path),
            message_id: message.id,
        });
        self.selected = self.selected.min(self.last_index());
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
    use super::super::app::{
        DEFAULT_QUERY, app_with_accounts, app_with_folders,
        app_with_messages,
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
        app.apply(Action::Sync);
        assert!(app.notice.is_some());
        app.apply(Action::Quit);
        assert!(app.quit);
    }

    #[test]
    fn marking_read_flips_state_and_queues_one_op() {
        let mut app = app_with_messages(2);
        assert!(app.messages[0].unread);
        app.apply(Action::MarkRead);
        app.apply(Action::MarkRead);
        assert!(!app.messages[0].unread);
        assert_eq!(app.pending_ops.len(), 1);
        let OpIntent::Flag { remove, add, .. } = &app.pending_ops[0]
        else {
            panic!("expected a flag op");
        };
        assert_eq!(remove, &vec!["unread".to_string()]);
        assert!(add.is_empty());
    }

    #[test]
    fn flag_toggle_round_trips_through_tags() {
        let mut app = app_with_messages(1);
        app.apply(Action::ToggleFlagged);
        assert!(app.messages[0].tags.contains(&"flagged".into()));
        app.apply(Action::ToggleFlagged);
        assert!(!app.messages[0].tags.contains(&"flagged".into()));
        assert_eq!(app.pending_ops.len(), 2);
    }

    #[test]
    fn accounts_derive_from_maildir_paths() {
        let cases = [
            ("/store/maildir/work/cur/1.host:2,S", "work"),
            ("/store/maildir/personal/new/2.host", "personal"),
            ("/elsewhere/3.host", ""),
        ];
        for (path, expected) in cases {
            assert_eq!(
                account_of(std::path::Path::new(path)),
                expected,
                "{path}"
            );
        }
    }

    #[test]
    fn delete_removes_the_row_and_clamps_selection() {
        let mut app = app_with_messages(2);
        app.apply(Action::Bottom);
        app.apply(Action::DeleteMessage);
        assert_eq!(app.messages.len(), 1);
        assert_eq!(app.selected, 0);
        assert_eq!(app.total_messages, 1);
        assert!(matches!(app.pending_ops[0], OpIntent::Delete { .. }));
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
