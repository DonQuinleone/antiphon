use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use antiphon_config::{
    Dirs, Loaded, NamedAccount, OauthProvider, load,
};
use antiphon_oauth::{
    OauthError, TokenSet, TokenStore, device_code_flow, graph_grant,
    imap_grant, pkce_loopback_flow,
};

use crate::oauthgrants::{
    account_grants, expiry, open_store, resolve_client_id,
};

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
    let address = entry.account.imap.user.as_str();
    match oauth.provider {
        OauthProvider::Microsoft => login_microsoft(
            name,
            address,
            oauth,
            &client_id,
            entry.account.graph.as_ref(),
            &store,
        ),
        OauthProvider::Google => {
            login_google(name, address, oauth, &client_id, &store)
        }
    }
}

fn login_microsoft(
    account: &str,
    address: &str,
    oauth: &antiphon_config::Oauth,
    client_id: &str,
    graph: Option<&antiphon_config::Graph>,
    store: &TokenStore,
) -> Result<(), String> {
    if graph.is_some_and(|graph| {
        graph.send && graph.auth == antiphon_config::GraphAuth::AppOnly
    }) {
        println!(
            "graph send is app-only: tokens come from \
             client_credentials at send time, no sign-in needed"
        );
    }
    for spec in
        account_grants(account, address, oauth, client_id, graph)
    {
        if !spec.wanted {
            continue;
        }
        println!(
            "authorising the {} grant for {account}",
            spec.audience
        );
        let tokens = device_code_flow(&spec.grant, &|prompt| {
            println!(
                "open {} and enter code {}",
                prompt.verification_url, prompt.user_code
            );
        })
        .map_err(|error| error.to_string())?;
        save(store, &spec.grant_name, &tokens)?;
        println!("stored the {} grant for {account}", spec.audience);
    }
    Ok(())
}

fn login_google(
    account: &str,
    address: &str,
    oauth: &antiphon_config::Oauth,
    client_id: &str,
    store: &TokenStore,
) -> Result<(), String> {
    let spec = account_grants(account, address, oauth, client_id, None)
        .into_iter()
        .next()
        .expect("google always wants its mail grant");
    let tokens = pkce_loopback_flow(&spec.grant, &|prompt| {
        println!(
            "open this URL in your browser to authorise \
                 {account}:"
        );
        println!("{}", prompt.consent_url);
        println!("waiting for the sign-in to complete...");
    })
    .map_err(|error| error.to_string())?;
    save(store, &spec.grant_name, &tokens)?;
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
    use antiphon_oauth::Provider;
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
            tenant: None,
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
