use std::process::Command;

use antiphon_config::{Loaded, Rule};
use antiphon_oauth::imap_grant;
use antiphon_sync::{Auth, DeliveryRule, SmtpAccount, SyncAccount};

const IMAPS_PORT: u16 = 993;
const SUBMISSION_PORT: u16 = 587;

/// Everything the worker derives from configuration, swapped
/// wholesale on reload so a pass never sees a half-updated
/// account list.
#[derive(Clone)]
pub(crate) struct AccountSet {
    pub(crate) accounts: Vec<SyncAccount>,
    pub(crate) oauth: Vec<OauthAccount>,
    pub(crate) smtp: Vec<(String, SmtpAccount)>,
    pub(crate) rules: Vec<(String, Vec<DeliveryRule>)>,
    pub(crate) notify: bool,
}

impl AccountSet {
    pub(crate) fn from_loaded(loaded: &Loaded) -> AccountSet {
        AccountSet {
            accounts: sync_accounts(loaded),
            oauth: oauth_accounts(loaded),
            smtp: smtp_accounts(loaded),
            rules: delivery_rules(loaded),
            notify: loaded.config.notifications.enabled,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.accounts.len() + self.oauth.len()
    }

    pub(crate) fn graph_spec(
        &self,
        account: &str,
    ) -> Option<&OauthAccount> {
        self.oauth
            .iter()
            .find(|spec| spec.name == account && spec.graph_send)
    }

    pub(crate) fn rules_for(&self, account: &str) -> &[DeliveryRule] {
        self.rules
            .iter()
            .find(|(name, _)| name == account)
            .map(|(_, rules)| rules.as_slice())
            .unwrap_or(&[])
    }
}

#[derive(Clone, Debug)]
pub struct OauthAccount {
    pub name: String,
    pub user: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp: Option<SmtpEndpoint>,
    pub graph_send: bool,
}

#[derive(Clone, Debug)]
pub struct SmtpEndpoint {
    pub host: String,
    pub port: u16,
    pub user: String,
}

impl OauthAccount {
    pub fn grant_name(&self) -> String {
        imap_grant(&self.name)
    }

    pub fn sync_account(&self, access_token: String) -> SyncAccount {
        SyncAccount {
            name: self.name.clone(),
            host: self.imap_host.clone(),
            port: self.imap_port,
            user: self.user.clone(),
            auth: Auth::XOauth2 {
                user: self.user.clone(),
                access_token,
            },
        }
    }

    pub fn smtp_account(
        &self,
        access_token: String,
    ) -> Option<SmtpAccount> {
        let endpoint = self.smtp.as_ref()?;
        Some(SmtpAccount {
            host: endpoint.host.clone(),
            port: endpoint.port,
            user: endpoint.user.clone(),
            auth: Auth::XOauth2 {
                user: endpoint.user.clone(),
                access_token,
            },
        })
    }
}

pub fn sync_accounts(loaded: &Loaded) -> Vec<SyncAccount> {
    loaded
        .accounts
        .iter()
        .filter_map(|entry| {
            let account = &entry.account;
            if account.oauth.is_some() {
                return None;
            }
            let command = account.imap.password_cmd.as_deref()?;
            let password = resolve_password(command)?;
            Some(SyncAccount {
                name: account.account.name.clone(),
                host: account.imap.host.clone(),
                port: account.imap.port.unwrap_or(IMAPS_PORT),
                user: account.imap.user.clone(),
                auth: Auth::Password(password),
            })
        })
        .collect()
}

pub fn oauth_accounts(loaded: &Loaded) -> Vec<OauthAccount> {
    loaded
        .accounts
        .iter()
        .filter_map(|entry| {
            let account = &entry.account;
            account.oauth.as_ref()?;
            let user = account.imap.user.clone();
            Some(OauthAccount {
                name: account.account.name.clone(),
                user: user.clone(),
                imap_host: account.imap.host.clone(),
                imap_port: account.imap.port.unwrap_or(IMAPS_PORT),
                smtp: account.smtp.as_ref().map(|smtp| SmtpEndpoint {
                    host: smtp.host.clone(),
                    port: smtp.port.unwrap_or(SUBMISSION_PORT),
                    user: smtp
                        .user
                        .clone()
                        .unwrap_or_else(|| user.clone()),
                }),
                graph_send: account
                    .graph
                    .as_ref()
                    .is_some_and(|graph| graph.send),
            })
        })
        .collect()
}

pub fn smtp_accounts(loaded: &Loaded) -> Vec<(String, SmtpAccount)> {
    loaded
        .accounts
        .iter()
        .filter_map(|entry| {
            let account = &entry.account;
            if account.oauth.is_some() {
                return None;
            }
            let smtp = account.smtp.as_ref()?;
            let user = smtp
                .user
                .clone()
                .unwrap_or_else(|| account.imap.user.clone());
            let command = smtp
                .password_cmd
                .as_deref()
                .or(account.imap.password_cmd.as_deref())?;
            let password = resolve_password(command)?;
            Some((
                account.account.name.clone(),
                SmtpAccount {
                    host: smtp.host.clone(),
                    port: smtp.port.unwrap_or(SUBMISSION_PORT),
                    user,
                    auth: Auth::Password(password),
                },
            ))
        })
        .collect()
}

pub fn delivery_rules(
    loaded: &Loaded,
) -> Vec<(String, Vec<DeliveryRule>)> {
    loaded
        .accounts
        .iter()
        .filter(|entry| !entry.account.rules.is_empty())
        .map(|entry| {
            let rules = entry
                .account
                .rules
                .iter()
                .map(to_delivery_rule)
                .collect();
            (entry.account.account.name.clone(), rules)
        })
        .collect()
}

fn to_delivery_rule(rule: &Rule) -> DeliveryRule {
    DeliveryRule {
        match_list: rule.match_list.clone(),
        match_sender: rule.match_sender.clone(),
        move_to: rule.move_to.clone(),
        tag: rule.tag.clone(),
    }
}

fn resolve_password(command: &str) -> Option<String> {
    let output =
        Command::new("sh").args(["-c", command]).output().ok()?;
    if !output.status.success() {
        eprintln!("password_cmd failed: {command}");
        return None;
    }
    let password = String::from_utf8_lossy(&output.stdout)
        .trim_end_matches(['\r', '\n'])
        .to_string();
    if password.is_empty() {
        eprintln!("password_cmd produced nothing: {command}");
        return None;
    }
    Some(password)
}

#[cfg(test)]
mod tests {
    use antiphon_config::{
        Account, AccountFile, Config, Imap, NamedAccount, Oauth,
        OauthProvider, Smtp,
    };

    use super::*;

    fn oauth_entry(smtp: Option<Smtp>) -> NamedAccount {
        NamedAccount {
            file_stem: "work".to_string(),
            account: AccountFile {
                account: Account {
                    name: "work".to_string(),
                    maildir: None,
                    archive: None,
                    trash: None,
                },
                imap: Imap {
                    host: "imap.example.com".to_string(),
                    port: None,
                    user: "quin@example.com".to_string(),
                    password_cmd: Some("echo never-run".to_string()),
                },
                smtp,
                identities: Vec::new(),
                rules: Vec::new(),
                oauth: Some(Oauth {
                    provider: OauthProvider::Google,
                    client_id: Some("client-app".to_string()),
                }),
                graph: None,
                folder_names: Default::default(),
                folder_order: Vec::new(),
                folders_hidden: Vec::new(),
            },
        }
    }

    fn loaded_with(entry: NamedAccount) -> Loaded {
        Loaded {
            config: Config::default(),
            accounts: vec![entry],
        }
    }

    fn bare_smtp() -> Smtp {
        Smtp {
            host: "smtp.example.com".to_string(),
            port: None,
            user: None,
            password_cmd: None,
        }
    }

    #[test]
    fn oauth_accounts_never_reach_the_password_paths() {
        let loaded = loaded_with(oauth_entry(Some(bare_smtp())));
        assert!(sync_accounts(&loaded).is_empty());
        assert!(smtp_accounts(&loaded).is_empty());
    }

    #[test]
    fn oauth_specs_fill_ports_and_the_smtp_user_fallback() {
        let loaded = loaded_with(oauth_entry(Some(bare_smtp())));
        let specs = oauth_accounts(&loaded);
        assert_eq!(specs.len(), 1);
        let spec = &specs[0];
        assert_eq!(spec.name, "work");
        assert_eq!(spec.imap_port, IMAPS_PORT);
        assert_eq!(spec.grant_name(), "work-imap");
        let endpoint = spec.smtp.as_ref().expect("smtp endpoint");
        assert_eq!(endpoint.port, SUBMISSION_PORT);
        assert_eq!(endpoint.user, "quin@example.com");
    }

    #[test]
    fn oauth_specs_build_xoauth2_sync_and_smtp_accounts() {
        let loaded = loaded_with(oauth_entry(Some(bare_smtp())));
        let spec = &oauth_accounts(&loaded)[0];

        let sync = spec.sync_account("token-1".to_string());
        assert_eq!(sync.user, "quin@example.com");
        assert!(matches!(
            &sync.auth,
            Auth::XOauth2 { user, access_token }
                if user == "quin@example.com"
                    && access_token == "token-1"
        ));

        let smtp = spec
            .smtp_account("token-1".to_string())
            .expect("smtp account");
        assert!(matches!(
            &smtp.auth,
            Auth::XOauth2 { user, access_token }
                if user == "quin@example.com"
                    && access_token == "token-1"
        ));

        let no_smtp = oauth_entry(None);
        let spec = &oauth_accounts(&loaded_with(no_smtp))[0];
        assert!(spec.smtp_account("token-1".to_string()).is_none());
    }
}
