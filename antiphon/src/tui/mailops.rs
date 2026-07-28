//! The message operations the list view queues for the
//! daemon: read and flag toggles, deletes, archives and
//! moves, each recorded as a durable `OpIntent`.

use super::app::App;

const UNREAD_TAG: &str = "unread";
const FLAGGED_TAG: &str = "flagged";

// Lowercase because the store's local folder names are the
// server names lowercased by the sync engine.
const DEFAULT_ARCHIVE_FOLDER: &str = "archive";
// Typing "inbox" moves to the account root, whose folder path
// is empty.
const ROOT_FOLDER_INPUT: &str = "inbox";
// Lowercase like every local folder name.
const DEFAULT_TRASH_FOLDER: &str = "trash";

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

impl App {
    pub(super) fn toggle_read(&mut self) {
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

    pub(super) fn toggle_flagged(&mut self) {
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
}

#[cfg(test)]
mod tests {
    use antiphon_core::Action;

    use super::super::testkit::app_with_messages;
    use super::*;

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
}
