use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use antiphon_config::{
    Dirs, Loaded, NamedAccount, OauthProvider, load,
};
use antiphon_oauth::{
    GOOGLE_MAIL_SCOPES, Grant, MICROSOFT_GRAPH_SEND_SCOPES,
    MICROSOFT_IMAP_SCOPES, OauthError, Provider, TokenSet, TokenStore,
    device_code_flow, graph_grant, imap_grant, pkce_loopback_flow,
};
use antiphon_store::StoreLayout;

use crate::autostart;

const SECONDS_PER_MINUTE: u64 = 60;

pub fn login(account: &str) -> ExitCode {
    report(run_login(account))
}

pub fn status(account: &str) -> ExitCode {
    report(run_status(account))
}

fn report(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

/// The environment overrides the account file, so a private
/// client_id can be swapped in without editing shared config;
/// with neither set the account keeps bring-your-own.
fn resolve_client_id(
    oauth: &antiphon_config::Oauth,
    name: &str,
    env: impl Fn(&str) -> Option<String>,
) -> Result<String, String> {
    let var = match oauth.provider {
        OauthProvider::Microsoft => "ANTIPHON_MS_CLIENT_ID",
        OauthProvider::Google => "ANTIPHON_GOOGLE_CLIENT_ID",
    };
    if let Some(id) = env(var).filter(|id| !id.is_empty()) {
        return Ok(id);
    }
    oauth.client_id.clone().ok_or(format!(
        "account {name}: set client_id in [oauth] (or {var}); \
         register your own app with the provider first"
    ))
}

fn run_login(name: &str) -> Result<(), String> {
    let (dirs, loaded) = load_config()?;
    let entry = find_account(&loaded, name)?;
    let oauth = entry.account.oauth.as_ref().ok_or(format!(
        "account {name} has no [oauth] table; add one with \
         provider and client_id"
    ))?;
    let client_id =
        resolve_client_id(oauth, name, |var| std::env::var(var).ok())?;
    let store = open_store(&dirs)?;
    match oauth.provider {
        OauthProvider::Microsoft => {
            let graph_send = entry
                .account
                .graph
                .as_ref()
                .is_some_and(|graph| graph.send);
            login_microsoft(name, &client_id, graph_send, &store)
        }
        OauthProvider::Google => login_google(name, &client_id, &store),
    }
}

fn login_microsoft(
    account: &str,
    client_id: &str,
    graph_send: bool,
    store: &TokenStore,
) -> Result<(), String> {
    let grants: &[(&str, &str, String, bool)] = &[
        ("IMAP", MICROSOFT_IMAP_SCOPES, imap_grant(account), true),
        (
            "Graph send",
            MICROSOFT_GRAPH_SEND_SCOPES,
            graph_grant(account),
            graph_send,
        ),
    ];
    for (audience, scopes, grant_name, wanted) in grants {
        if !wanted {
            continue;
        }
        println!("authorising the {audience} grant for {account}");
        let tokens = device_code_flow(
            &Grant {
                provider: Provider::Microsoft,
                scopes: (*scopes).to_string(),
                client_id: client_id.to_string(),
            },
            &|prompt| {
                println!(
                    "open {} and enter code {}",
                    prompt.verification_url, prompt.user_code
                );
            },
        )
        .map_err(|error| error.to_string())?;
        save(store, grant_name, &tokens)?;
        println!("stored the {audience} grant for {account}");
    }
    Ok(())
}

fn login_google(
    account: &str,
    client_id: &str,
    store: &TokenStore,
) -> Result<(), String> {
    let tokens = pkce_loopback_flow(
        &Grant {
            provider: Provider::Google,
            scopes: GOOGLE_MAIL_SCOPES.to_string(),
            client_id: client_id.to_string(),
        },
        &|prompt| {
            println!(
                "open this URL in your browser to authorise \
                 {account}:"
            );
            println!("{}", prompt.consent_url);
            println!("waiting for the sign-in to complete...");
        },
    )
    .map_err(|error| error.to_string())?;
    save(store, &imap_grant(account), &tokens)?;
    println!("stored the mail grant for {account}");
    Ok(())
}

fn run_status(name: &str) -> Result<(), String> {
    let (dirs, loaded) = load_config()?;
    find_account(&loaded, name)?;
    let store = open_store(&dirs)?;
    let now = now_unix();
    let mut found = false;
    for (label, grant_name) in
        [("imap", imap_grant(name)), ("graph", graph_grant(name))]
    {
        match store.load(&grant_name) {
            Ok(tokens) => {
                found = true;
                println!("{}", describe(label, &tokens, now));
            }
            Err(OauthError::NoStoredToken(_)) => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    if !found {
        return Err(format!(
            "no grants stored for {name}; run \
             `antiphon oauth login {name}`"
        ));
    }
    Ok(())
}

fn describe(label: &str, tokens: &TokenSet, now: u64) -> String {
    format!(
        "{label}: {} via {}, {}",
        tokens.scope,
        tokens.provider,
        expiry(tokens, now)
    )
}

fn expiry(tokens: &TokenSet, now: u64) -> String {
    if tokens.expires_at_unix <= now {
        return String::from(
            "access token expired (refreshes on the next sync)",
        );
    }
    let minutes = (tokens.expires_at_unix - now) / SECONDS_PER_MINUTE;
    format!("access token valid for {minutes} min")
}

fn load_config() -> Result<(Dirs, Loaded), String> {
    let dirs = Dirs::from_process()
        .ok_or("cannot resolve the home directory")?;
    let loaded = load(&dirs).map_err(|error| error.to_string())?;
    Ok((dirs, loaded))
}

fn find_account<'a>(
    loaded: &'a Loaded,
    name: &str,
) -> Result<&'a NamedAccount, String> {
    loaded
        .accounts
        .iter()
        .find(|entry| entry.account.account.name == name)
        .ok_or(format!("no account named {name} is configured"))
}

/// The token store lives inside the vault, so the daemon must
/// be up (and the vault mounted) before it can be opened; this
/// is the same autostart path sendmail takes.
fn open_store(dirs: &Dirs) -> Result<TokenStore, String> {
    autostart::ensure_daemon(true, dirs)?;
    let layout = StoreLayout::new(dirs.store_root());
    if !layout.exists() {
        return Err(String::from(
            "the store is unavailable (vault sealed?)",
        ));
    }
    TokenStore::open(layout.tokens_dir())
        .map_err(|error| error.to_string())
}

fn save(
    store: &TokenStore,
    grant_name: &str,
    tokens: &TokenSet,
) -> Result<(), String> {
    store
        .save(grant_name, tokens)
        .map_err(|error| error.to_string())
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;

    use super::*;

    fn tokens_expiring_at(expires_at_unix: u64) -> TokenSet {
        TokenSet {
            access_token: SecretString::from("at"),
            refresh_token: SecretString::from("rt"),
            expires_at_unix,
            scope: "https://mail.google.com/".to_string(),
            client_id: "client-app".to_string(),
            provider: Provider::Google,
        }
    }

    #[test]
    fn expiry_reports_minutes_left_or_expired() {
        let now = 1_000_000;
        let live = tokens_expiring_at(now + 45 * 60);
        assert_eq!(expiry(&live, now), "access token valid for 45 min");
        let dead = tokens_expiring_at(now);
        assert!(expiry(&dead, now).contains("expired"));
    }

    fn oauth_config(client_id: Option<&str>) -> antiphon_config::Oauth {
        antiphon_config::Oauth {
            provider: OauthProvider::Microsoft,
            client_id: client_id.map(str::to_string),
        }
    }

    #[test]
    fn the_environment_overrides_the_account_client_id() {
        let oauth = oauth_config(Some("from-config"));
        let id = resolve_client_id(&oauth, "work", |var| {
            (var == "ANTIPHON_MS_CLIENT_ID")
                .then(|| "from-env".to_string())
        })
        .unwrap();
        assert_eq!(id, "from-env");
    }

    #[test]
    fn without_an_override_the_account_client_id_stands() {
        let oauth = oauth_config(Some("from-config"));
        let id = resolve_client_id(&oauth, "work", |_| None).unwrap();
        assert_eq!(id, "from-config");
    }

    #[test]
    fn neither_source_set_names_the_variable_in_the_error() {
        let oauth = oauth_config(None);
        let error =
            resolve_client_id(&oauth, "work", |_| None).unwrap_err();
        assert!(error.contains("ANTIPHON_MS_CLIENT_ID"));
    }

    #[test]
    fn an_empty_override_is_ignored() {
        let oauth = oauth_config(Some("from-config"));
        let id =
            resolve_client_id(&oauth, "work", |_| Some(String::new()))
                .unwrap();
        assert_eq!(id, "from-config");
    }

    #[test]
    fn describe_names_the_grant_scope_and_provider() {
        let now = 1_000_000;
        let tokens = tokens_expiring_at(now + 60 * 60);
        let line = describe("imap", &tokens, now);
        assert!(line.starts_with("imap: https://mail.google.com/"));
        assert!(line.contains("via google"));
        assert!(line.contains("60 min"));
    }
}
