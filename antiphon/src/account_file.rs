//! Writes an account's TOML file from wizard or form answers:
//! a full template for a brand-new account, surgical per-key
//! edits for an existing one so hand-written content survives.

use std::path::Path;

use antiphon_config::Dirs;

use crate::account_wizard::AccountAnswers;
use crate::tui::configedit::{
    array_key_value, with_array_key, with_key,
};

/// A brand new account: fails rather than overwriting if the
/// name is already taken, the same guarantee `antiphon setup`
/// gives its own fresh account.
pub(crate) fn write_account(
    dirs: &Dirs,
    answers: &AccountAnswers,
) -> Result<(), String> {
    let accounts_dir = dirs.config.join("accounts");
    std::fs::create_dir_all(&accounts_dir)
        .map_err(|error| error.to_string())?;
    let text = account_toml(answers);
    let path = accounts_dir.join(format!("{}.toml", answers.name));
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    std::fs::write(&path, text)
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// The overwrite-friendly sibling of `write_account`: used by
/// the settings view's edit, where a name already on disk is
/// expected rather than an error. An existing file is edited
/// key by key so hand-written content ([[rules]], [oauth],
/// folder lists, comments) survives; the full template is
/// written only when there is nothing to start from.
pub(crate) fn write_account_file(
    dirs: &Dirs,
    answers: &AccountAnswers,
    previous_name: Option<&str>,
) -> Result<(), String> {
    let accounts_dir = dirs.config.join("accounts");
    std::fs::create_dir_all(&accounts_dir)
        .map_err(|error| error.to_string())?;
    let text = match existing_text(
        &accounts_dir,
        previous_name,
        &answers.name,
    )? {
        Some(existing) => edited_account_toml(&existing, answers),
        None => account_toml(answers),
    };
    let path = accounts_dir.join(format!("{}.toml", answers.name));
    std::fs::write(&path, text)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    remove_renamed(&accounts_dir, previous_name, &answers.name)
}

/// The file an edit starts from: the previous name's when the
/// edit renames the account, else the target's own.
fn existing_text(
    dir: &Path,
    previous_name: Option<&str>,
    new_name: &str,
) -> Result<Option<String>, String> {
    let stem = previous_name.unwrap_or(new_name);
    let path = dir.join(format!("{stem}.toml"));
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

/// Rewrites only the keys the edit form carries, one surgical
/// edit each, so every line the form does not know about is
/// left exactly as the user wrote it.
fn edited_account_toml(
    existing: &str,
    answers: &AccountAnswers,
) -> String {
    let edits: [(&str, &str, &str); 5] = [
        ("account", "name", &answers.name),
        ("imap", "host", &answers.imap_host),
        ("imap", "user", &answers.imap_user),
        ("imap", "password_cmd", &answers.password_cmd),
        ("smtp", "host", &answers.smtp_host),
    ];
    let mut text = existing.to_string();
    for (table, key, value) in edits {
        text = with_key(&text, table, key, &quoted(value));
    }
    with_identity_address(&text, &answers.address)
}

/// The first identity follows the form's address; its `match`
/// list is rewritten only while it still has the template's
/// shape (exactly the old address), so a hand-tuned list
/// survives an address change.
fn with_identity_address(text: &str, address: &str) -> String {
    let old_address = array_key_value(text, "identity", "address");
    let matches = array_key_value(text, "identity", "match");
    let edited =
        with_array_key(text, "identity", "address", &quoted(address));
    let template = old_address.map(|old| format!("[{old}]"));
    let rewrite = match (&matches, &template) {
        (None, None) => true,
        (Some(current), Some(shape)) => current == shape,
        _ => false,
    };
    if !rewrite {
        return edited;
    }
    let list = format!("[{}]", quoted(address));
    with_array_key(&edited, "identity", "match", &list)
}

fn quoted(value: &str) -> String {
    format!("\"{value}\"")
}

/// An edit that changed the account's name leaves its old file
/// behind unless removed once the new one is safely written.
fn remove_renamed(
    dir: &Path,
    previous_name: Option<&str>,
    new_name: &str,
) -> Result<(), String> {
    let Some(previous) = previous_name else {
        return Ok(());
    };
    if previous == new_name {
        return Ok(());
    }
    let old_path = dir.join(format!("{previous}.toml"));
    std::fs::remove_file(&old_path)
        .map_err(|error| format!("{}: {error}", old_path.display()))
}

fn account_toml(answers: &AccountAnswers) -> String {
    let AccountAnswers {
        name,
        address,
        imap_host,
        imap_user,
        smtp_host,
        password_cmd,
    } = answers;
    format!(
        "[account]\n\
         name = \"{name}\"\n\n\
         [imap]\n\
         host = \"{imap_host}\"\n\
         user = \"{imap_user}\"\n\
         password_cmd = \"{password_cmd}\"\n\n\
         [smtp]\n\
         host = \"{smtp_host}\"\n\n\
         [[identity]]\n\
         address = \"{address}\"\n\
         match = [\"{address}\"]\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs_at(root: &Path) -> Dirs {
        Dirs {
            config: root.to_path_buf(),
            state: root.join("state"),
            cache: root.join("cache"),
            data: root.join("data"),
        }
    }

    fn answers() -> AccountAnswers {
        AccountAnswers {
            name: "work".to_string(),
            address: "quin@example.com".to_string(),
            imap_host: "imap.example.com".to_string(),
            imap_user: "quin@example.com".to_string(),
            smtp_host: "smtp.example.com".to_string(),
            password_cmd: "pass show mail/work".to_string(),
        }
    }

    #[test]
    fn account_toml_carries_every_field() {
        let text = account_toml(&answers());
        assert!(text.contains("name = \"work\""));
        assert!(text.contains("host = \"imap.example.com\""));
        assert!(text.contains("host = \"smtp.example.com\""));
        assert!(text.contains("address = \"quin@example.com\""));
        assert!(
            text.contains("password_cmd = \"pass show mail/work\"")
        );
    }

    #[test]
    fn write_account_only_succeeds_once() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs_at(root.path());
        write_account(&dirs, &answers()).expect("first write");
        assert!(
            write_account(&dirs, &answers()).is_err(),
            "a second write must not overwrite silently"
        );
    }

    #[test]
    fn write_account_file_creates_and_then_overwrites() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs_at(root.path());
        write_account_file(&dirs, &answers(), None)
            .expect("first write");
        let path = dirs.config.join("accounts").join("work.toml");
        assert!(path.exists());

        let mut changed = answers();
        changed.imap_host = "imap2.example.com".to_string();
        write_account_file(&dirs, &changed, Some("work"))
            .expect("overwrite");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("imap2.example.com"));
    }

    fn hand_written_toml() -> &'static str {
        "folder_order = [\"INBOX\", \"lists\"]\n\
         folders_hidden = [\"Junk\"]\n\
         \n\
         [account]\n\
         name = \"work\"\n\
         \n\
         [imap]\n\
         host = \"imap.example.com\"\n\
         user = \"quin@example.com\"\n\
         password_cmd = \"pass show mail/work\"  # rotate soon\n\
         \n\
         [smtp]\n\
         host = \"smtp.example.com\"\n\
         \n\
         [[identity]]\n\
         address = \"quin@example.com\"\n\
         match = [\"quin@example.com\"]\n\
         \n\
         [[rules]]\n\
         from = \"ci@example.com\"\n\
         move_to = \"ci\"\n\
         \n\
         [oauth]\n\
         provider = \"microsoft\"\n"
    }

    #[test]
    fn an_edit_preserves_hand_written_config() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs_at(root.path());
        let accounts = dirs.config.join("accounts");
        std::fs::create_dir_all(&accounts).unwrap();
        let path = accounts.join("work.toml");
        std::fs::write(&path, hand_written_toml()).unwrap();

        let mut changed = answers();
        changed.imap_host = "imap2.example.com".to_string();
        write_account_file(&dirs, &changed, Some("work"))
            .expect("edit in place");

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("host = \"imap2.example.com\""));
        assert!(
            text.contains("folder_order = [\"INBOX\", \"lists\"]"),
            "folder_order survives: {text}"
        );
        assert!(text.contains("folders_hidden = [\"Junk\"]"));
        assert!(text.contains("[[rules]]"), "rules survive: {text}");
        assert!(text.contains("from = \"ci@example.com\""));
        assert!(text.contains("move_to = \"ci\""));
        assert!(text.contains("[oauth]"), "oauth survives: {text}");
        assert!(text.contains("provider = \"microsoft\""));
        assert!(
            text.contains(
                "password_cmd = \"pass show mail/work\"  # rotate soon"
            ),
            "comments survive: {text}"
        );
    }

    #[test]
    fn an_address_edit_follows_a_template_shaped_match_list() {
        let mut changed = answers();
        changed.address = "new@example.com".to_string();
        let text = edited_account_toml(hand_written_toml(), &changed);
        assert!(text.contains("address = \"new@example.com\""));
        assert!(text.contains("match = [\"new@example.com\"]"));
        assert!(!text.contains("address = \"quin@example.com\""));
        assert!(!text.contains("match = [\"quin@example.com\"]"));
    }

    #[test]
    fn an_address_edit_leaves_a_hand_tuned_match_list_alone() {
        let hand_tuned = hand_written_toml().replace(
            "match = [\"quin@example.com\"]",
            "match = [\"quin@example.com\", \"old@example.com\"]",
        );
        let mut changed = answers();
        changed.address = "new@example.com".to_string();
        let text = edited_account_toml(&hand_tuned, &changed);
        assert!(text.contains("address = \"new@example.com\""));
        assert!(text.contains(
            "match = [\"quin@example.com\", \"old@example.com\"]"
        ));
    }

    #[test]
    fn a_rename_carries_the_hand_written_content_across() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs_at(root.path());
        let accounts = dirs.config.join("accounts");
        std::fs::create_dir_all(&accounts).unwrap();
        std::fs::write(accounts.join("work.toml"), hand_written_toml())
            .unwrap();

        let mut renamed = answers();
        renamed.name = "personal".to_string();
        write_account_file(&dirs, &renamed, Some("work"))
            .expect("rename");

        assert!(!accounts.join("work.toml").exists());
        let text =
            std::fs::read_to_string(accounts.join("personal.toml"))
                .unwrap();
        assert!(text.contains("name = \"personal\""));
        assert!(text.contains("[[rules]]"));
        assert!(text.contains("folder_order = [\"INBOX\", \"lists\"]"));
    }

    #[test]
    fn write_account_file_renames_when_the_name_changes() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs_at(root.path());
        write_account_file(&dirs, &answers(), None)
            .expect("first write");

        let mut renamed = answers();
        renamed.name = "personal".to_string();
        write_account_file(&dirs, &renamed, Some("work"))
            .expect("rename");

        let accounts = dirs.config.join("accounts");
        assert!(!accounts.join("work.toml").exists());
        assert!(accounts.join("personal.toml").exists());
    }

    #[test]
    fn remove_renamed_only_acts_when_the_name_actually_changed() {
        let root = tempfile::tempdir().unwrap();
        let accounts = root.path().join("accounts");
        std::fs::create_dir_all(&accounts).unwrap();
        std::fs::write(accounts.join("work.toml"), "x").unwrap();

        remove_renamed(&accounts, Some("work"), "work")
            .expect("unchanged name is a no-op");
        assert!(accounts.join("work.toml").exists());

        remove_renamed(&accounts, Some("work"), "personal")
            .expect("a changed name removes the old file");
        assert!(!accounts.join("work.toml").exists());

        remove_renamed(&accounts, None, "personal")
            .expect("no previous name is a no-op");
    }
}
