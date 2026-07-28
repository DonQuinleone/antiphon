use antiphon_config::AccountFile;

use crate::setup::{mail_secret, prompt, required, validate_address};

/// One account's worth of setup answers: gathered by
/// `prompt_account`, then handed to `write_account_file`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AccountAnswers {
    pub(crate) name: String,
    pub(crate) address: String,
    pub(crate) from_name: String,
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
            from_name: account
                .identities
                .first()
                .and_then(|identity| identity.name.clone())
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
    let from_name = prompt(
        "from name (optional)",
        defaults.map_or("", |d| d.from_name.as_str()),
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
        from_name,
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

#[cfg(test)]
mod tests {
    use antiphon_config::{Account, Identity, Imap, Smtp};

    use super::*;

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
                name: Some("Quin at Work".to_string()),
                signature: None,
                matches: Vec::new(),
                pgp_sign: false,
                pgp_key: None,
            }],
            rules: Vec::new(),
            oauth: None,
            graph: None,
            folder_names: Default::default(),
            folder_order: Vec::new(),
            folders_hidden: Vec::new(),
            folders_unsynced: Vec::new(),
        };
        let defaults = AccountAnswers::from_existing(&account);
        assert_eq!(defaults.address, "quin@example.com");
        assert_eq!(defaults.from_name, "Quin at Work");
        assert_eq!(defaults.imap_user, "quin");
        assert_eq!(defaults.smtp_host, "smtp.example.com");
        assert_eq!(defaults.password_cmd, "pass show mail/work");
    }
}
