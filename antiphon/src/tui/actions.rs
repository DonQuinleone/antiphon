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
    Move {
        account: String,
        message_id: String,
        to_folder: String,
        from_folder: String,
    },
}

/// The folder a delivered message sits in, relative to its
/// account maildir: empty for the root inbox, "lists/rust"
/// for a nested folder.
pub fn folder_of(path: &std::path::Path) -> String {
    let account = account_of(path);
    let mut parts: Vec<&str> = Vec::new();
    let mut seen_account = false;
    for component in path.components() {
        let text = component.as_os_str().to_str().unwrap_or("");
        if seen_account {
            parts.push(text);
        }
        if !seen_account && text == account {
            seen_account = true;
        }
    }
    // The trailing cur|new/<file> pair never names a folder.
    let folder_parts = parts.len().saturating_sub(2).min(parts.len());
    parts[..folder_parts].join("/")
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

const THREAD_LABEL: &str = "thread";
// Lowercase because the store's local folder names are the
// server names lowercased by the sync engine.
const DEFAULT_ARCHIVE_FOLDER: &str = "archive";
// Typing "inbox" moves to the account root, whose folder path
// is empty.
const ROOT_FOLDER_INPUT: &str = "inbox";
// Lowercase like every local folder name.
const DEFAULT_TRASH_FOLDER: &str = "trash";

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
        self.scope = step(&self.scope, &self.accounts);
        self.thread_return = None;
        self.requery = true;
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
    }

    fn toggle_read(&mut self) {
        let Some(message) = self.messages.get(self.selected) else {
            return;
        };
        let unread = message.unread;
        self.set_unread(!unread);
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

    /// d is a move to the account's trash folder; only inside
    /// that folder does it become a real deletion, and then
    /// only after a y/n confirmation. The server's own trash
    /// expiry does the rest.
    pub(super) fn delete_selected(&mut self) {
        let Some(message) = self.selected_message() else {
            return;
        };
        let account = account_of(&message.path);
        let trash = self.trash_folder_of(&account);
        if folder_of(&message.path) == trash {
            self.open_prompt(
                super::commands::PromptKind::ConfirmDelete,
            );
            return;
        }
        self.move_selected_to(&trash);
    }

    /// The confirmed permanent deletion, from the trash tab
    /// of the prompt; everything else still goes through
    /// delete_selected's move.
    pub(super) fn delete_selected_forever(&mut self) {
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

    fn trash_folder_of(&self, account: &str) -> String {
        self.trash_folders
            .iter()
            .find(|(name, _)| name == account)
            .map(|(_, folder)| folder.clone())
            .unwrap_or_else(|| DEFAULT_TRASH_FOLDER.to_string())
    }

    /// a sends the message to the account's archive folder
    /// ("Archive" unless the account names another): a durable
    /// move op the daemon replays against the server.
    pub(super) fn archive_selected(&mut self) {
        let Some(message) = self.selected_message() else {
            return;
        };
        let account = account_of(&message.path);
        let to_folder = self.archive_folder_of(&account);
        self.move_selected_to(&to_folder);
    }

    /// The one row-to-op path for moves: the row leaves at
    /// once, the op records where the message came from so
    /// the server replay can find it there.
    pub(super) fn move_selected_to(&mut self, to_folder: &str) {
        if self.selected >= self.messages.len() {
            return;
        }
        let message = self.messages.remove(self.selected);
        self.total_messages = self.total_messages.saturating_sub(1);
        let account = account_of(&message.path);
        let to_folder = self.resolve_folder(&account, to_folder);
        self.pending_ops.push(OpIntent::Move {
            account,
            message_id: message.id,
            from_folder: folder_of(&message.path),
            to_folder,
        });
        self.selected = self.selected.min(self.last_index());
    }

    /// Folder names typed by the user accept an alias as well
    /// as the real path; display is the inverse mapping.
    pub(super) fn resolve_folder(
        &self,
        account: &str,
        input: &str,
    ) -> String {
        if input == ROOT_FOLDER_INPUT {
            return String::new();
        }
        self.folder_aliases
            .iter()
            .find(|(acct, _, alias)| acct == account && alias == input)
            .map(|(_, real, _)| real.clone())
            .unwrap_or_else(|| input.to_string())
    }

    pub(super) fn alias_for(
        &self,
        account: &str,
        folder: &str,
    ) -> Option<&str> {
        self.folder_aliases
            .iter()
            .find(|(acct, real, _)| acct == account && real == folder)
            .map(|(_, _, alias)| alias.as_str())
    }

    fn archive_folder_of(&self, account: &str) -> String {
        self.archive_folders
            .iter()
            .find(|(name, _)| name == account)
            .map(|(_, folder)| folder.clone())
            .unwrap_or_else(|| DEFAULT_ARCHIVE_FOLDER.to_string())
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
    fn folder_of_reads_the_subdir_between_account_and_cur() {
        let cases = [
            ("store/maildir/work/cur/a.eml", ""),
            ("store/maildir/work/lists/rust/new/a.eml", "lists/rust"),
            ("store/maildir/work/archive/cur/a.eml", "archive"),
        ];
        for (path, expected) in cases {
            assert_eq!(
                folder_of(std::path::Path::new(path)),
                expected,
                "{path}"
            );
        }
    }

    #[test]
    fn typed_folder_names_resolve_aliases_and_the_root() {
        let mut app = app_with_messages(1);
        app.folder_aliases = vec![(
            "work".to_string(),
            "inbox/accounts".to_string(),
            "accounts".to_string(),
        )];
        assert_eq!(
            app.resolve_folder("work", "accounts"),
            "inbox/accounts"
        );
        assert_eq!(
            app.resolve_folder("home", "accounts"),
            "accounts",
            "aliases are per account"
        );
        assert_eq!(app.resolve_folder("work", "inbox"), "");
        assert_eq!(
            app.alias_for("work", "inbox/accounts"),
            Some("accounts")
        );
    }

    #[test]
    fn delete_trashes_first_and_only_confirms_inside_trash() {
        let mut app = app_with_messages(2);
        app.messages[0].path =
            std::path::PathBuf::from("store/maildir/work/cur/one.eml");
        app.messages[1].path = std::path::PathBuf::from(
            "store/maildir/work/trash/cur/two.eml",
        );
        app.apply_in_list(Action::DeleteMessage);
        let Some(OpIntent::Move { to_folder, .. }) =
            app.pending_ops.last()
        else {
            panic!("expected a move to trash");
        };
        assert_eq!(to_folder, "trash");
        assert_eq!(app.messages.len(), 1);

        app.selected = 0;
        app.apply_in_list(Action::DeleteMessage);
        assert!(
            app.prompt.is_some(),
            "inside trash, d asks before deleting"
        );
        assert_eq!(app.messages.len(), 1, "nothing removed yet");
        app.prompt = None;
        app.delete_selected_forever();
        assert!(matches!(
            app.pending_ops.last(),
            Some(OpIntent::Delete { .. })
        ));
        assert!(app.messages.is_empty());
    }

    #[test]
    fn archiving_moves_to_the_account_folder_and_drops_the_row() {
        let mut app = app_with_messages(2);
        app.messages[1].path =
            std::path::PathBuf::from("store/maildir/work/cur/two.eml");
        app.archive_folders =
            vec![("work".to_string(), "Archief".to_string())];
        app.selected = 1;
        app.apply_in_list(Action::Archive);
        assert_eq!(app.messages.len(), 1);
        assert_eq!(app.total_messages, 1);
        let Some(OpIntent::Move {
            account, to_folder, ..
        }) = app.pending_ops.last()
        else {
            panic!("expected a move op");
        };
        assert_eq!(account, "work");
        assert_eq!(to_folder, "Archief");

        app.apply_in_list(Action::Archive);
        let Some(OpIntent::Move { to_folder, .. }) =
            app.pending_ops.last()
        else {
            panic!("expected a move op");
        };
        assert_eq!(to_folder, "archive", "default folder");
        assert!(app.messages.is_empty());
        app.apply_in_list(Action::Archive);
        assert_eq!(app.pending_ops.len(), 2, "empty list is safe");
    }

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
    fn m_toggles_read_state_and_queues_an_op_each_way() {
        let mut app = app_with_messages(2);
        assert!(app.messages[0].unread);
        app.apply(Action::ToggleRead);
        assert!(!app.messages[0].unread);
        let OpIntent::Flag { remove, add, .. } = &app.pending_ops[0]
        else {
            panic!("expected a flag op");
        };
        assert_eq!(remove, &vec!["unread".to_string()]);
        assert!(add.is_empty());

        app.apply(Action::ToggleRead);
        assert!(app.messages[0].unread);
        assert_eq!(app.pending_ops.len(), 2);
        let OpIntent::Flag { remove, add, .. } = &app.pending_ops[1]
        else {
            panic!("expected a flag op");
        };
        assert_eq!(add, &vec!["unread".to_string()]);
        assert!(remove.is_empty());
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
        assert!(
            matches!(app.pending_ops[0], OpIntent::Move { .. }),
            "delete is a move to trash outside the trash folder"
        );
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
