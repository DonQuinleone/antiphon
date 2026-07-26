use std::io;

use antiphon_config::Dirs;
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::app::App;
use super::headers::byte_index;
use super::settings::{self, SettingsOutcome};
use super::sidebar::SidebarEntry;
use super::themecmd::{persist_key, remove_key};

const FOLDER_NAMES_TABLE: &str = "folder_names";

/// One row of the Folders settings tab: a discovered folder,
/// its account, and whatever alias is currently configured
/// for it (empty when none is).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FolderRow {
    pub(super) account: String,
    pub(super) folder: String,
    pub(super) alias: String,
}

/// The alias text being typed for the selected row: kept on
/// `App` rather than inside `SettingsState` so `input.rs` can
/// intercept keys the same way it does for the account form.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct AliasEdit {
    pub(super) text: String,
    pub(super) cursor: usize,
    pub(super) account: String,
    pub(super) folder: String,
}

/// Every discovered folder of every account, alias joined in;
/// pure over the sidebar's own discovery and the loaded
/// aliases, so it needs neither `App` nor the disk to test.
pub(super) fn rows(
    sidebar_entries: &[SidebarEntry],
    aliases: &[(String, String, String)],
) -> Vec<FolderRow> {
    sidebar_entries
        .iter()
        .filter_map(|entry| folder_row(entry, aliases))
        .collect()
}

fn folder_row(
    entry: &SidebarEntry,
    aliases: &[(String, String, String)],
) -> Option<FolderRow> {
    let SidebarEntry::Folder { account, name, .. } = entry else {
        return None;
    };
    let alias = aliases
        .iter()
        .find(|(acct, real, _)| acct == account && real == name)
        .map(|(_, _, alias)| alias.clone())
        .unwrap_or_default();
    Some(FolderRow {
        account: account.clone(),
        folder: name.clone(),
        alias,
    })
}

impl App {
    pub(super) fn folder_rows(&self) -> Vec<FolderRow> {
        rows(&self.sidebar_entries, &self.folder_aliases)
    }

    /// Re-derives the Folders tab's rows from the current
    /// sidebar and aliases; called after a save, so a rename
    /// or removal is reflected without re-opening settings.
    pub(super) fn refresh_settings_folders(&mut self) {
        let folders = self.folder_rows();
        let Some(state) = self.settings.as_mut() else {
            return;
        };
        let last = folders.len().saturating_sub(1);
        state.folders = folders;
        state.folder_selected = state.folder_selected.min(last);
    }
}

/// Keys on the Folders tab outside of an active alias edit:
/// j/k select a row, enter begins editing its alias.
pub(super) fn feed(app: &mut App, key: KeyEvent) -> SettingsOutcome {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            move_selection(app, 1);
            SettingsOutcome::Stay
        }
        KeyCode::Char('k') | KeyCode::Up => {
            move_selection(app, -1);
            SettingsOutcome::Stay
        }
        KeyCode::Enter => {
            begin_edit(app);
            SettingsOutcome::Stay
        }
        _ => SettingsOutcome::Stay,
    }
}

fn move_selection(app: &mut App, step: i32) {
    let Some(state) = app.settings.as_mut() else {
        return;
    };
    if state.folders.is_empty() {
        return;
    }
    state.folder_selected = settings::wrapped(
        state.folder_selected,
        state.folders.len(),
        step,
    );
}

fn selected_row(app: &App) -> Option<FolderRow> {
    let state = app.settings.as_ref()?;
    state.folders.get(state.folder_selected).cloned()
}

fn begin_edit(app: &mut App) {
    let Some(row) = selected_row(app) else {
        return;
    };
    app.folder_alias_edit = Some(AliasEdit {
        cursor: row.alias.chars().count(),
        text: row.alias,
        account: row.account,
        folder: row.folder,
    });
}

/// Keys while an alias is being typed: esc cancels without
/// writing anything, enter saves, everything else edits the
/// buffer in place the way the account form's fields do.
pub(super) fn feed_edit(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.folder_alias_edit = None,
        KeyCode::Enter => save(app),
        KeyCode::Char(ch) => insert(app, ch),
        KeyCode::Backspace => backspace(app),
        KeyCode::Delete => delete(app),
        KeyCode::Left => move_cursor(app, -1),
        KeyCode::Right => move_cursor(app, 1),
        KeyCode::Home => set_cursor(app, 0),
        KeyCode::End => end(app),
        _ => {}
    }
}

fn insert(app: &mut App, ch: char) {
    let Some(edit) = app.folder_alias_edit.as_mut() else {
        return;
    };
    let at = byte_index(&edit.text, edit.cursor);
    edit.text.insert(at, ch);
    edit.cursor += 1;
}

fn backspace(app: &mut App) {
    let Some(edit) = app.folder_alias_edit.as_mut() else {
        return;
    };
    if edit.cursor == 0 {
        return;
    }
    edit.cursor -= 1;
    let at = byte_index(&edit.text, edit.cursor);
    edit.text.remove(at);
}

fn delete(app: &mut App) {
    let Some(edit) = app.folder_alias_edit.as_mut() else {
        return;
    };
    if edit.cursor >= edit.text.chars().count() {
        return;
    }
    let at = byte_index(&edit.text, edit.cursor);
    edit.text.remove(at);
}

fn move_cursor(app: &mut App, step: i32) {
    let Some(edit) = app.folder_alias_edit.as_mut() else {
        return;
    };
    let len = edit.text.chars().count() as i32;
    edit.cursor = (edit.cursor as i32 + step).clamp(0, len) as usize;
}

fn set_cursor(app: &mut App, at: usize) {
    if let Some(edit) = app.folder_alias_edit.as_mut() {
        edit.cursor = at;
    }
}

fn end(app: &mut App) {
    if let Some(edit) = app.folder_alias_edit.as_mut() {
        edit.cursor = edit.text.chars().count();
    }
}

fn save(app: &mut App) {
    let Some(edit) = app.folder_alias_edit.take() else {
        return;
    };
    let Some(row) = selected_row(app) else {
        return;
    };
    let alias = edit.text.trim().to_string();
    match persist_alias(&app.dirs, &row.account, &row.folder, &alias) {
        Ok(()) => {
            update_alias(app, &row.account, &row.folder, &alias);
            app.notice = Some(alias_notice(&row.folder, &alias));
            app.refresh_settings_folders();
        }
        Err(error) => {
            app.notice = Some(format!("{}: {error}", row.folder));
            app.folder_alias_edit = Some(edit);
        }
    }
}

fn alias_notice(folder: &str, alias: &str) -> String {
    if alias.is_empty() {
        format!("{folder}: alias removed")
    } else {
        format!("{folder}: alias set to {alias}")
    }
}

/// The sidebar reads `App.folder_aliases` fresh on every
/// frame, so patching it here is all a saved alias needs to
/// show up there without a restart.
fn update_alias(
    app: &mut App,
    account: &str,
    folder: &str,
    alias: &str,
) {
    app.folder_aliases
        .retain(|(acct, real, _)| !(acct == account && real == folder));
    if !alias.is_empty() {
        app.folder_aliases.push((
            account.to_string(),
            folder.to_string(),
            alias.to_string(),
        ));
    }
}

/// Writes (or, for an empty alias, removes) the one
/// `folder_names` entry through the same surgical config edit
/// the essentials rows use, so every other line in the
/// account file survives untouched.
fn persist_alias(
    dirs: &Dirs,
    account: &str,
    folder: &str,
    alias: &str,
) -> io::Result<()> {
    let path =
        dirs.config.join("accounts").join(format!("{account}.toml"));
    if alias.is_empty() {
        remove_key(&path, FOLDER_NAMES_TABLE, folder)
    } else {
        persist_key(&path, FOLDER_NAMES_TABLE, folder, &quoted(alias))
    }
}

fn quoted(value: &str) -> String {
    format!("\"{value}\"")
}

#[cfg(test)]
mod tests {
    use super::super::testkit::TempDir;
    use super::*;

    fn entry(account: &str, name: &str) -> SidebarEntry {
        SidebarEntry::Folder {
            account: account.to_string(),
            name: name.to_string(),
            query: String::new(),
            unread: 0,
        }
    }

    #[test]
    fn rows_join_the_alias_by_account_and_folder() {
        let entries = vec![
            SidebarEntry::Unified,
            entry("work", "archive"),
            entry("work", "lists/aerc"),
            entry("personal", "archive"),
        ];
        let aliases = vec![(
            "work".to_string(),
            "lists/aerc".to_string(),
            "aerc-list".to_string(),
        )];
        let built = rows(&entries, &aliases);
        assert_eq!(
            built,
            vec![
                FolderRow {
                    account: "work".to_string(),
                    folder: "archive".to_string(),
                    alias: String::new(),
                },
                FolderRow {
                    account: "work".to_string(),
                    folder: "lists/aerc".to_string(),
                    alias: "aerc-list".to_string(),
                },
                FolderRow {
                    account: "personal".to_string(),
                    folder: "archive".to_string(),
                    alias: String::new(),
                },
            ]
        );
    }

    #[test]
    fn an_alias_with_no_matching_folder_is_never_joined_in() {
        let entries = vec![entry("work", "archive")];
        let aliases = vec![(
            "work".to_string(),
            "gone".to_string(),
            "stale".to_string(),
        )];
        let built = rows(&entries, &aliases);
        assert_eq!(built[0].alias, "");
    }

    #[test]
    fn saving_a_blank_alias_removes_a_present_mapping() {
        let dir = TempDir::new();
        let dirs = Dirs {
            config: dir.path.clone(),
            state: dir.path.join("state"),
            cache: dir.path.join("cache"),
            data: dir.path.join("data"),
        };
        let path = dirs.config.join("accounts/work.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "[account]\nname = \"work\"\n\n\
             [folder_names]\n\"lists/aerc\" = \"aerc-list\"\n",
        )
        .unwrap();

        persist_alias(&dirs, "work", "lists/aerc", "")
            .expect("remove the mapping");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("aerc-list"));
        assert!(text.contains("[account]"), "the rest survives");

        persist_alias(&dirs, "work", "lists/aerc", "aerc")
            .expect("write a fresh mapping");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"lists/aerc\" = \"aerc\""));
    }

    #[test]
    fn update_alias_replaces_rather_than_duplicates() {
        let mut app = super::super::testkit::app_with_messages(1);
        app.folder_aliases = vec![(
            "work".to_string(),
            "archive".to_string(),
            "Old".to_string(),
        )];
        update_alias(&mut app, "work", "archive", "New");
        assert_eq!(
            app.folder_aliases,
            vec![(
                "work".to_string(),
                "archive".to_string(),
                "New".to_string()
            )]
        );
        update_alias(&mut app, "work", "archive", "");
        assert!(app.folder_aliases.is_empty());
    }
}
