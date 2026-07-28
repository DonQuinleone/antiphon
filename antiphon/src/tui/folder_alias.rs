//! The Folders tab's alias editing: the modal buffer, its
//! keys, and the `folder_names` persistence behind it.

use std::io;

use antiphon_config::Dirs;
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::app::App;
use super::configedit::{persist_key, remove_key};
use super::folders::selected_row;
use super::headers::byte_index;

const FOLDER_NAMES_TABLE: &str = "folder_names";

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

pub(super) fn begin_edit(app: &mut App) {
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
