use std::path::Path;

use antiphon_config::{AccountFile, Dirs};

use crate::setup::{mail_secret, prompt, required, validate_address};

/// One account's worth of setup answers: gathered by
/// `prompt_account`, then handed to `write_account_file`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AccountAnswers {
    pub(crate) name: String,
    pub(crate) address: String,
    pub(crate) imap_host: String,
    pub(crate) imap_user: String,
    pub(crate) smtp_host: String,
    pub(crate) password_cmd: String,
}

impl AccountAnswers {
    pub(crate) fn from_existing(
        account: &AccountFile,
    ) -> AccountAnswers {
        AccountAnswers {
            name: account.account.name.clone(),
            address: account
                .identities
                .first()
                .map(|identity| identity.address.clone())
                .unwrap_or_default(),
            imap_host: account.imap.host.clone(),
            imap_user: account.imap.user.clone(),
            smtp_host: account
                .smtp
                .as_ref()
                .map(|smtp| smtp.host.clone())
                .unwrap_or_default(),
            password_cmd: account
                .imap
                .password_cmd
                .clone()
                .unwrap_or_default(),
        }
    }
}

/// The wizard's account Q&A, reused for both a fresh `antiphon
/// setup` and the settings view's add/edit: with `defaults`,
/// Enter keeps the existing answer instead of demanding a
/// fresh one.
pub(crate) fn prompt_account(
    defaults: Option<&AccountAnswers>,
) -> Result<AccountAnswers, String> {
    let address = prompt_address(defaults.map(|d| d.address.as_str()))?;
    let domain = address.split('@').next_back().unwrap_or_default();
    let name = prompt(
        "account name",
        defaults.map_or("personal", |d| d.name.as_str()),
    )?;
    let imap_host = prompt(
        "imap host",
        &defaults.map_or_else(
            || format!("imap.{domain}"),
            |d| d.imap_host.clone(),
        ),
    )?;
    let imap_user = prompt(
        "imap user",
        defaults.map_or(address.as_str(), |d| d.imap_user.as_str()),
    )?;
    let smtp_host = prompt(
        "smtp host",
        &defaults.map_or_else(
            || format!("smtp.{domain}"),
            |d| d.smtp_host.clone(),
        ),
    )?;
    let password_cmd = mail_secret(&name, &address)?;
    Ok(AccountAnswers {
        name,
        address,
        imap_host,
        imap_user,
        smtp_host,
        password_cmd,
    })
}

/// The settings form's masked Keychain field, already holding
/// the secret in memory: stored the same way `mail_secret`
/// stores its interactively-typed one, just without the
/// terminal prompt in between.
#[cfg(target_os = "macos")]
pub(crate) fn store_supplied_secret(
    name: &str,
    address: &str,
    secret: &str,
) -> Result<String, String> {
    crate::setup::store_keychain_secret(name, address, secret)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn store_supplied_secret(
    _name: &str,
    _address: &str,
    _secret: &str,
) -> Result<String, String> {
    Err("the Keychain is only available on macOS".to_string())
}

fn prompt_address(default: Option<&str>) -> Result<String, String> {
    let Some(default) = default else {
        return required("e-mail address", validate_address);
    };
    loop {
        let answer = prompt("e-mail address", default)?;
        match validate_address(&answer) {
            Ok(()) => return Ok(answer),
            Err(error) => println!("{error}"),
        }
    }
}

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
/// expected rather than an error.
pub(crate) fn write_account_file(
    dirs: &Dirs,
    answers: &AccountAnswers,
    previous_name: Option<&str>,
) -> Result<(), String> {
    let accounts_dir = dirs.config.join("accounts");
    std::fs::create_dir_all(&accounts_dir)
        .map_err(|error| error.to_string())?;
    let text = account_toml(answers);
    let path = accounts_dir.join(format!("{}.toml", answers.name));
    std::fs::write(&path, text)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    remove_renamed(&accounts_dir, previous_name, &answers.name)
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
    use antiphon_config::{Account, Identity, Imap, Smtp};

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
    fn from_existing_reads_the_first_identity_and_smtp_host() {
        let account = AccountFile {
            account: Account {
                name: "work".to_string(),
                maildir: None,
                archive: None,
                trash: None,
            },
            imap: Imap {
                host: "imap.example.com".to_string(),
                port: None,
                user: "quin".to_string(),
                password_cmd: Some("pass show mail/work".to_string()),
            },
            smtp: Some(Smtp {
                host: "smtp.example.com".to_string(),
                port: None,
                user: None,
                password_cmd: None,
            }),
            identities: vec![Identity {
                address: "quin@example.com".to_string(),
                name: None,
                signature: None,
                matches: Vec::new(),
                pgp_sign: false,
                pgp_key: None,
            }],
            rules: Vec::new(),
            oauth: None,
            graph: None,
            folder_names: Default::default(),
        };
        let defaults = AccountAnswers::from_existing(&account);
        assert_eq!(defaults.address, "quin@example.com");
        assert_eq!(defaults.imap_user, "quin");
        assert_eq!(defaults.smtp_host, "smtp.example.com");
        assert_eq!(defaults.password_cmd, "pass show mail/work");
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
