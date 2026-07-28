//! Saving the account form: validation, the base file write
//! through `account_file` (surgical for edits), and the
//! [oauth]/[graph] keys the form owns patched on afterwards.

use std::path::Path;

use antiphon_config::{Dirs, GraphAuth, OauthProvider};

use super::account_form::{
    AccountFormState, graph_auth_toml, provider_name,
};
use super::app::App;
use super::configedit;
use crate::account_file;
use crate::account_wizard::{self, AccountAnswers};
use crate::setup::validate_address;

pub(super) fn save(app: &mut App) {
    let Some(form) = app.account_form.as_ref() else {
        return;
    };
    match build_and_write(&app.dirs, form) {
        Ok(name) => {
            app.notice = Some(match super::request_reload() {
                None => format!("account {name} saved; syncing"),
                Some(notice) => {
                    format!("account {name} saved ({notice})")
                }
            });
            app.account_form = None;
            app.refresh_settings_accounts();
        }
        Err(error) => {
            if let Some(form) = app.account_form.as_mut() {
                form.error = Some(error);
            }
        }
    }
}

fn build_and_write(
    dirs: &Dirs,
    form: &AccountFormState,
) -> Result<String, String> {
    validate(form)?;
    let answers = AccountAnswers {
        name: form.name.trim().to_string(),
        address: form.address.trim().to_string(),
        imap_host: form.imap_host.trim().to_string(),
        imap_user: form.imap_user.trim().to_string(),
        smtp_host: form.smtp_host.trim().to_string(),
        password_cmd: resolve_password_cmd(form)?,
    };
    let adding = form.editing.is_none();
    if adding && account_path(dirs, &answers.name).exists() {
        return Err(format!("{} already exists", answers.name));
    }
    account_file::write_account_file(
        dirs,
        &answers,
        form.editing.as_deref(),
    )?;
    patch_oauth(&account_path(dirs, &answers.name), form)?;
    Ok(answers.name)
}

fn account_path(dirs: &Dirs, name: &str) -> std::path::PathBuf {
    dirs.config.join("accounts").join(format!("{name}.toml"))
}

fn validate(form: &AccountFormState) -> Result<(), String> {
    if form.name.trim().is_empty() {
        return Err("account name is required".to_string());
    }
    validate_address(form.address.trim())?;
    if form.imap_host.trim().is_empty() {
        return Err("imap host is required".to_string());
    }
    if form.imap_user.trim().is_empty() {
        return Err("imap user is required".to_string());
    }
    if form.smtp_host.trim().is_empty() {
        return Err("smtp host is required".to_string());
    }
    Ok(())
}

/// An OAuth account signs in with a grant, so no password is
/// asked for or written. Otherwise a typed password command
/// wins outright; failing that, on macOS, the masked field's
/// secret is stored in the Keychain and its lookup command
/// takes the empty field's place.
fn resolve_password_cmd(
    form: &AccountFormState,
) -> Result<String, String> {
    if form.provider().is_some() {
        return Ok(String::new());
    }
    let typed = form.password_cmd.trim();
    if !typed.is_empty() {
        return Ok(typed.to_string());
    }
    if !cfg!(target_os = "macos") {
        return Err(
            "give a password command, e.g. pass show mail/name"
                .to_string(),
        );
    }
    let secret = form.keychain_secret.trim();
    if secret.is_empty() {
        return Err("type the password into the Keychain field, \
                    or give a password command above"
            .to_string());
    }
    account_wizard::store_supplied_secret(
        form.name.trim(),
        form.address.trim(),
        secret,
    )
}

fn patch_oauth(
    path: &Path,
    form: &AccountFormState,
) -> Result<(), String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let patched = with_oauth(&text, form);
    if patched == text {
        return Ok(());
    }
    std::fs::write(path, patched)
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// Only the keys the form owns are touched: [oauth] provider
/// and client_id, [graph] send, auth, tenant and secret_cmd.
/// A non-OAuth type drops the whole [oauth] table but leaves
/// [graph] alone (its body goes with the account's own edits,
/// not this patch). Every other line survives untouched.
fn with_oauth(text: &str, form: &AccountFormState) -> String {
    let Some(provider) = form.provider() else {
        return configedit::without_table(text, "oauth");
    };
    let name = provider_name(Some(provider));
    let mut text =
        configedit::with_key(text, "oauth", "provider", &quoted(name));
    text = optional_key(
        &text,
        "oauth",
        "client_id",
        form.client_id.trim(),
    );
    if provider != OauthProvider::Microsoft {
        return text;
    }
    with_graph(&text, form)
}

/// The [graph] table for a Microsoft account: send off leaves
/// the table as it stands (only flipping `send` if it exists),
/// send on writes the auth flow, tenant and, for app-only, the
/// secret command; delegated drops any lingering secret_cmd.
fn with_graph(text: &str, form: &AccountFormState) -> String {
    if !form.graph_send {
        if configedit::has_table(text, "graph") {
            return configedit::with_key(
                text, "graph", "send", "false",
            );
        }
        return text.to_string();
    }
    let auth = graph_auth_toml(form.graph_auth);
    let mut text = configedit::with_key(text, "graph", "send", "true");
    text = configedit::with_key(&text, "graph", "auth", &quoted(auth));
    text = optional_key(&text, "graph", "tenant", form.tenant.trim());
    if form.graph_auth == GraphAuth::AppOnly {
        return optional_key(
            &text,
            "graph",
            "secret_cmd",
            form.graph_secret_cmd.trim(),
        );
    }
    configedit::without_key(&text, "graph", "secret_cmd")
}

/// A filled value is written, an emptied one removed, so the
/// file mirrors the form without leaving stale keys behind.
fn optional_key(
    text: &str,
    table: &str,
    key: &str,
    value: &str,
) -> String {
    if value.is_empty() {
        configedit::without_key(text, table, key)
    } else {
        configedit::with_key(text, table, key, &quoted(value))
    }
}

fn quoted(value: &str) -> String {
    format!("\"{value}\"")
}

#[cfg(test)]
mod tests {
    use super::super::account_form::AccountType;
    use super::super::account_form::tests::{
        filled_answers, filled_form,
    };
    use super::super::testkit::TempDir;
    use super::*;

    fn dirs_at(root: &Path) -> Dirs {
        Dirs {
            config: root.to_path_buf(),
            state: root.join("state"),
            cache: root.join("cache"),
            data: root.join("data"),
        }
    }

    #[test]
    fn a_blank_password_command_fails_validation_off_macos() {
        if cfg!(target_os = "macos") {
            return;
        }
        let mut form = filled_form();
        form.password_cmd = String::new();
        assert!(resolve_password_cmd(&form).is_err());
    }

    #[test]
    fn an_oauth_account_needs_no_password_at_all() {
        let mut form = filled_form();
        form.password_cmd = String::new();
        form.account_type = AccountType::Google;
        assert_eq!(resolve_password_cmd(&form), Ok(String::new()));
    }

    #[test]
    fn saving_an_edit_overwrites_only_the_one_file() {
        let root = TempDir::new();
        let dirs = dirs_at(&root.path);
        account_file::write_account_file(
            &dirs,
            &filled_answers(),
            None,
        )
        .expect("seed the account file");

        let mut form = filled_form();
        form.editing = Some("work".to_string());
        form.imap_host = "imap2.example.com".to_string();
        let name = build_and_write(&dirs, &form).expect("save");
        assert_eq!(name, "work");

        let text = std::fs::read_to_string(
            dirs.config.join("accounts/work.toml"),
        )
        .unwrap();
        assert!(text.contains("imap2.example.com"));
    }

    #[test]
    fn renaming_on_save_removes_the_old_file() {
        let root = TempDir::new();
        let dirs = dirs_at(&root.path);
        account_file::write_account_file(
            &dirs,
            &filled_answers(),
            None,
        )
        .expect("seed the account file");

        let mut form = filled_form();
        form.editing = Some("work".to_string());
        form.name = "personal".to_string();
        build_and_write(&dirs, &form).expect("save");

        let accounts_dir = dirs.config.join("accounts");
        assert!(!accounts_dir.join("work.toml").exists());
        assert!(accounts_dir.join("personal.toml").exists());
    }

    #[test]
    fn adding_over_an_existing_name_is_refused() {
        let root = TempDir::new();
        let dirs = dirs_at(&root.path);
        account_file::write_account_file(
            &dirs,
            &filled_answers(),
            None,
        )
        .expect("seed the account file");

        let form = filled_form();
        assert!(build_and_write(&dirs, &form).is_err());
    }

    #[test]
    fn a_google_add_writes_a_parseable_oauth_table() {
        let root = TempDir::new();
        let dirs = dirs_at(&root.path);
        let mut form = filled_form();
        form.password_cmd = String::new();
        form.account_type = AccountType::Google;
        form.client_id = "app-1".to_string();
        build_and_write(&dirs, &form).expect("save");

        let text = std::fs::read_to_string(
            dirs.config.join("accounts/work.toml"),
        )
        .unwrap();
        assert!(text.contains("[oauth]"), "{text}");
        assert!(text.contains("provider = \"google\""));
        assert!(text.contains("client_id = \"app-1\""));
        assert!(!text.contains("password_cmd"), "{text}");

        let loaded = antiphon_config::load(&dirs).expect("parse");
        let oauth = loaded.accounts[0]
            .account
            .oauth
            .as_ref()
            .expect("oauth table");
        assert_eq!(oauth.provider, OauthProvider::Google);
        assert_eq!(oauth.client_id.as_deref(), Some("app-1"));
    }

    #[test]
    fn microsoft_graph_send_writes_the_graph_table() {
        let root = TempDir::new();
        let dirs = dirs_at(&root.path);
        let mut form = filled_form();
        form.account_type = AccountType::Microsoft;
        form.graph_send = true;
        form.tenant = "tenant-1".to_string();
        build_and_write(&dirs, &form).expect("save");

        let loaded = antiphon_config::load(&dirs).expect("parse");
        let account = &loaded.accounts[0].account;
        let graph = account.graph.as_ref().expect("graph table");
        assert!(graph.send);
        assert_eq!(graph.tenant.as_deref(), Some("tenant-1"));
        assert_eq!(graph.auth, GraphAuth::Delegated);
        assert!(account.oauth.is_some());
    }

    fn oauth_toml() -> &'static str {
        "[account]\n\
         name = \"work\"\n\
         \n\
         [imap]\n\
         host = \"imap.example.com\"\n\
         user = \"quin@example.com\"\n\
         \n\
         [smtp]\n\
         host = \"smtp.example.com\"\n\
         \n\
         [[identity]]\n\
         address = \"quin@example.com\"\n\
         \n\
         [oauth]\n\
         provider = \"microsoft\"\n\
         client_id = \"app-1\"\n\
         \n\
         [graph]\n\
         send = true\n\
         tenant = \"tenant-1\"\n\
         auth = \"app_only\"\n\
         secret_cmd = \"pass show graph\"  # rotate\n"
    }

    fn seeded(root: &TempDir) -> Dirs {
        let dirs = dirs_at(&root.path);
        let accounts = dirs.config.join("accounts");
        std::fs::create_dir_all(&accounts).unwrap();
        std::fs::write(accounts.join("work.toml"), oauth_toml())
            .unwrap();
        dirs
    }

    #[test]
    fn choosing_none_drops_oauth_but_keeps_graph() {
        let root = TempDir::new();
        let dirs = seeded(&root);
        let mut form = filled_form();
        form.editing = Some("work".to_string());
        build_and_write(&dirs, &form).expect("save");

        let text = std::fs::read_to_string(
            dirs.config.join("accounts/work.toml"),
        )
        .unwrap();
        assert!(!text.contains("[oauth]"), "{text}");
        assert!(!text.contains("provider ="), "{text}");
        assert!(text.contains("[graph]"), "graph survives: {text}");
        assert!(text.contains("secret_cmd = \"pass show graph\""));
    }

    /// An edit that re-affirms the inferred graph settings (as
    /// opening the form does) keeps the app-only flow and the
    /// secret command's hand-written comment.
    #[test]
    fn an_edit_keeps_the_graph_secret_command_and_comment() {
        let root = TempDir::new();
        let dirs = seeded(&root);
        let mut form = filled_form();
        form.editing = Some("work".to_string());
        form.password_cmd = String::new();
        form.account_type = AccountType::Microsoft;
        form.client_id = "app-2".to_string();
        form.graph_send = true;
        form.graph_auth = GraphAuth::AppOnly;
        form.graph_secret_cmd = "pass show graph".to_string();
        form.tenant = String::new();
        build_and_write(&dirs, &form).expect("save");

        let text = std::fs::read_to_string(
            dirs.config.join("accounts/work.toml"),
        )
        .unwrap();
        assert!(text.contains("client_id = \"app-2\""));
        assert!(
            !text.contains("tenant ="),
            "an emptied tenant is removed: {text}"
        );
        assert!(text.contains("auth = \"app_only\""));
        assert!(
            text.contains("secret_cmd = \"pass show graph\"  # rotate"),
            "the command and its comment survive: {text}"
        );
    }

    /// Switching an app-only account to delegated drops the
    /// now-unused secret command.
    #[test]
    fn delegated_drops_the_graph_secret_command() {
        let root = TempDir::new();
        let dirs = seeded(&root);
        let mut form = filled_form();
        form.editing = Some("work".to_string());
        form.password_cmd = String::new();
        form.account_type = AccountType::Microsoft;
        form.graph_send = true;
        form.graph_auth = GraphAuth::Delegated;
        build_and_write(&dirs, &form).expect("save");

        let text = std::fs::read_to_string(
            dirs.config.join("accounts/work.toml"),
        )
        .unwrap();
        assert!(text.contains("auth = \"delegated\""), "{text}");
        assert!(!text.contains("secret_cmd"), "{text}");
    }

    #[test]
    fn turning_graph_send_off_keeps_the_table() {
        let root = TempDir::new();
        let dirs = seeded(&root);
        let mut form = filled_form();
        form.editing = Some("work".to_string());
        form.account_type = AccountType::Microsoft;
        form.graph_send = false;
        build_and_write(&dirs, &form).expect("save");

        let text = std::fs::read_to_string(
            dirs.config.join("accounts/work.toml"),
        )
        .unwrap();
        assert!(text.contains("send = false"), "{text}");
        assert!(text.contains("secret_cmd = \"pass show graph\""));
    }

    fn rich_imap_toml() -> &'static str {
        "folder_order = [\"INBOX\", \"lists\"]\n\
         folders_hidden = [\"Junk\"]\n\
         folders_unsynced = [\"Archive\"]\n\
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
         match_sender = \"ci@example.com\"\n\
         move_to = \"ci\"\n"
    }

    /// Changing an IMAP account to Microsoft 365 adds the
    /// [oauth]/[graph] tables while every hand-written line
    /// (rules, folder lists, comments) is left untouched.
    #[test]
    fn changing_type_preserves_hand_written_content() {
        let root = TempDir::new();
        let dirs = dirs_at(&root.path);
        let accounts = dirs.config.join("accounts");
        std::fs::create_dir_all(&accounts).unwrap();
        std::fs::write(accounts.join("work.toml"), rich_imap_toml())
            .unwrap();

        let mut form = filled_form();
        form.editing = Some("work".to_string());
        form.password_cmd = String::new();
        form.account_type = AccountType::Microsoft;
        form.client_id = "app-9".to_string();
        form.graph_send = true;
        form.graph_auth = GraphAuth::AppOnly;
        form.tenant = "tenant-9".to_string();
        form.graph_secret_cmd = "pass show graph".to_string();
        build_and_write(&dirs, &form).expect("save");

        let text = std::fs::read_to_string(accounts.join("work.toml"))
            .unwrap();
        assert!(text.contains("[[rules]]"), "rules survive: {text}");
        assert!(text.contains("match_sender = \"ci@example.com\""));
        assert!(
            text.contains("folder_order = [\"INBOX\", \"lists\"]"),
            "{text}"
        );
        assert!(text.contains("folders_hidden = [\"Junk\"]"));
        assert!(text.contains("folders_unsynced = [\"Archive\"]"));
        assert!(
            text.contains(
                "password_cmd = \"pass show mail/work\"  # rotate soon"
            ),
            "comments survive: {text}"
        );
        assert!(text.contains("provider = \"microsoft\""));
        assert!(text.contains("client_id = \"app-9\""));
        assert!(text.contains("auth = \"app_only\""));
        assert!(text.contains("secret_cmd = \"pass show graph\""));

        let loaded = antiphon_config::load(&dirs).expect("parse");
        let account = &loaded.accounts[0].account;
        assert_eq!(
            account.oauth.as_ref().map(|oauth| oauth.provider),
            Some(OauthProvider::Microsoft)
        );
        assert_eq!(account.rules.len(), 1);
        assert_eq!(
            account.folders_unsynced,
            vec!["Archive".to_string()]
        );
    }
}
