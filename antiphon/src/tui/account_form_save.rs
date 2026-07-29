//! Saving the account form: validation, the base file write
//! through `account_file` (surgical for edits), and the
//! [oauth]/[graph] keys the form owns patched on afterwards.

use std::path::Path;

use antiphon_config::{Dirs, GraphAuth, OauthProvider};

use super::account_form::AccountFormState;
use super::account_form_fields::{graph_auth_toml, provider_name};
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
    let address = form.address.trim().to_string();
    let (imap_host, imap_user, smtp_host) = servers(form, &address);
    let answers = AccountAnswers {
        name: form.name.trim().to_string(),
        address: address.clone(),
        from_name: first_identity_name(form),
        imap_host,
        imap_user,
        smtp_host,
        password_cmd: resolve_password_cmd(form)?,
    };
    let adding = form.editing.is_none();
    if adding && account_path(dirs, &answers.name).exists() {
        return Err(format!("{} already exists", answers.name));
    }
    let path = account_path(dirs, &answers.name);
    account_file::write_account_file(
        dirs,
        &answers,
        form.editing.as_deref(),
    )?;
    write_identities(&path, form, &address)?;
    patch_oauth(&path, form)?;
    if adding {
        seed_microsoft_defaults(&path, form)?;
    }
    Ok(answers.name)
}

/// The Microsoft mailbox exposes calendars, contacts and other
/// non-mail folders over IMAP; syncing them wastes time and
/// clutters the sidebar, so a fresh Microsoft account starts with
/// them unsynced. They stay listed in the Folders tab for
/// re-including one. `calendar*` also covers calendar subfolders.
const MS365_DEFAULT_UNSYNCED: &[&str] = &[
    "calendar*",
    "contacts",
    "conversation history",
    "journal",
    "rss feeds",
    "outbox",
];

fn seed_microsoft_defaults(
    path: &Path,
    form: &AccountFormState,
) -> Result<(), String> {
    if form.provider() != Some(OauthProvider::Microsoft) {
        return Ok(());
    }
    let values: Vec<String> = MS365_DEFAULT_UNSYNCED
        .iter()
        .map(|name| (*name).to_string())
        .collect();
    configedit::persist_root_key(
        path,
        "folders_unsynced",
        &configedit::toml_string_array(&values),
    )
    .map_err(|error| format!("{}: {error}", path.display()))
}

fn first_identity_name(form: &AccountFormState) -> String {
    form.identities
        .first()
        .map(|identity| identity.from_name.trim().to_string())
        .unwrap_or_default()
}

/// Rewrites the file's `[[identity]]` blocks to match the form's
/// identity list, each from address falling back to the account
/// address. A form always carries at least one identity, so an
/// empty list is left untouched rather than clearing the file.
fn write_identities(
    path: &Path,
    form: &AccountFormState,
    account_address: &str,
) -> Result<(), String> {
    if form.identities.is_empty() {
        return Ok(());
    }
    let identities: Vec<antiphon_config::Identity> = form
        .identities
        .iter()
        .map(|identity| identity.to_config(account_address))
        .collect();
    account_file::write_account_identities(path, &identities)
}

fn account_path(dirs: &Dirs, name: &str) -> std::path::PathBuf {
    dirs.config.join("accounts").join(format!("{name}.toml"))
}

fn validate(form: &AccountFormState) -> Result<(), String> {
    if form.name.trim().is_empty() {
        return Err("account name is required".to_string());
    }
    validate_address(form.address.trim())?;
    if form.provider().is_some() {
        return Ok(());
    }
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

/// An OAuth account never asks for server details: each
/// provider's IMAP and SMTP hosts are fixed and the IMAP user
/// is the e-mail address. The standard 993/587 ports stay
/// implicit for the daemon to supply.
struct OauthHosts {
    imap: &'static str,
    smtp: &'static str,
}

fn oauth_hosts(provider: OauthProvider) -> OauthHosts {
    match provider {
        OauthProvider::Microsoft => OauthHosts {
            imap: "outlook.office365.com",
            smtp: "smtp.office365.com",
        },
        OauthProvider::Google => OauthHosts {
            imap: "imap.gmail.com",
            smtp: "smtp.gmail.com",
        },
    }
}

/// The IMAP host, IMAP user and SMTP host to write: the form's
/// own for an IMAP account, or the provider's fixed hosts (with
/// the address as the user) for an OAuth one.
fn servers(
    form: &AccountFormState,
    address: &str,
) -> (String, String, String) {
    let Some(provider) = form.provider() else {
        return (
            form.imap_host.trim().to_string(),
            form.imap_user.trim().to_string(),
            form.smtp_host.trim().to_string(),
        );
    };
    let hosts = oauth_hosts(provider);
    (
        hosts.imap.to_string(),
        address.to_string(),
        hosts.smtp.to_string(),
    )
}

/// An OAuth account signs in with a grant, so no password is
/// asked for or written. Otherwise the password-mode toggle
/// decides: command mode wants a lookup command, Keychain mode
/// (macOS) stores the typed secret and writes its lookup command
/// in its place.
fn resolve_password_cmd(
    form: &AccountFormState,
) -> Result<String, String> {
    if form.provider().is_some() {
        return Ok(String::new());
    }
    if !(cfg!(target_os = "macos") && form.keychain) {
        let typed = form.password_cmd.trim();
        if typed.is_empty() {
            return Err(
                "give a password command, e.g. pass show mail/name"
                    .to_string(),
            );
        }
        return Ok(typed.to_string());
    }
    let secret = form.keychain_secret.trim();
    if secret.is_empty() {
        return Err(
            "type the password to store in the Keychain".to_string()
        );
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
#[path = "account_form_save_tests.rs"]
mod tests;
