//! Writes an account's TOML file from wizard or form answers:
//! a full template for a brand-new account, surgical per-key
//! edits for an existing one so hand-written content survives.

use std::path::Path;

use antiphon_config::{Dirs, Identity};

use crate::account_wizard::AccountAnswers;
use crate::tui::configedit::with_key;

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

/// Rewrites only the account, imap and smtp keys the edit form
/// carries, one surgical edit each, so every line the form does
/// not know about is left exactly as the user wrote it. The
/// `[[identity]]` blocks are rewritten separately by
/// `write_account_identities`.
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
        // An OAuth account carries no password command; an
        // existing one is left as the user wrote it.
        if value.is_empty() {
            continue;
        }
        text = with_key(&text, table, key, &quoted(value));
    }
    text
}

/// Rewrites the account file at `path` so its `[[identity]]`
/// blocks match `identities` exactly: any dropped, any added,
/// the rest regenerated, while every other table and
/// hand-written line survives. The form owns every identity key,
/// so a block is rewritten wholesale rather than patched.
pub(crate) fn write_account_identities(
    path: &Path,
    identities: &[Identity],
) -> Result<(), String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let blocks: Vec<Vec<String>> =
        identities.iter().map(render_identity).collect();
    let rewritten = crate::tui::configedit::set_array_tables(
        &text, "identity", &blocks,
    );
    if rewritten == text {
        return Ok(());
    }
    std::fs::write(path, rewritten)
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn render_identity(identity: &Identity) -> Vec<String> {
    let mut block = vec![
        "[[identity]]".to_string(),
        format!("address = {}", quoted(&identity.address)),
    ];
    push_optional(&mut block, "name", identity.name.as_deref());
    push_optional(
        &mut block,
        "signature",
        identity.signature.as_deref(),
    );
    push_optional(&mut block, "pgp_key", identity.pgp_key.as_deref());
    if identity.pgp_sign {
        block.push("pgp_sign = true".to_string());
    }
    if !identity.matches.is_empty() {
        block.push(format!(
            "match = {}",
            quoted_array(&identity.matches)
        ));
    }
    block
}

fn push_optional(
    block: &mut Vec<String>,
    key: &str,
    value: Option<&str>,
) {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return;
    };
    block.push(format!("{key} = {}", quoted(value)));
}

fn quoted_array(values: &[String]) -> String {
    let items: Vec<String> =
        values.iter().map(|value| quoted(value)).collect();
    format!("[{}]", items.join(", "))
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
        from_name,
        imap_host,
        imap_user,
        smtp_host,
        password_cmd,
    } = answers;
    let password_line = match password_cmd.is_empty() {
        true => String::new(),
        false => format!("password_cmd = \"{password_cmd}\"\n"),
    };
    let name_line = match from_name.is_empty() {
        true => String::new(),
        false => format!("name = \"{from_name}\"\n"),
    };
    format!(
        "[account]\n\
         name = \"{name}\"\n\n\
         [imap]\n\
         host = \"{imap_host}\"\n\
         user = \"{imap_user}\"\n\
         {password_line}\n\
         [smtp]\n\
         host = \"{smtp_host}\"\n\n\
         [[identity]]\n\
         address = \"{address}\"\n\
         {name_line}\
         match = [\"{address}\"]\n"
    )
}
#[cfg(test)]
#[path = "account_file_tests.rs"]
mod tests;
