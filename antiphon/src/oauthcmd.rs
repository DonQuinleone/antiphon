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
        OauthProvider::Microsoft => login_microsoft(
            name,
            &client_id,
            entry.account.graph.as_ref(),
            &store,
        ),
        OauthProvider::Google => login_google(name, &client_id, &store),
    }
}

struct MicrosoftGrant {
    audience: &'static str,
    grant_name: String,
    grant: Grant,
    wanted: bool,
}

/// The IMAP grant always uses the [oauth] registration on the
/// common endpoint; a delegated Graph grant may carry its own
/// registration and tenant, and app-only Graph needs no
/// interactive grant at all (client credentials at send time).
fn microsoft_grants(
    account: &str,
    client_id: &str,
    graph: Option<&antiphon_config::Graph>,
) -> Vec<MicrosoftGrant> {
    let sending = graph.filter(|graph| graph.send);
    let delegated = sending.filter(|graph| {
        graph.auth == antiphon_config::GraphAuth::Delegated
    });
    vec![
        MicrosoftGrant {
            audience: "IMAP",
            grant_name: imap_grant(account),
            grant: Grant {
                provider: Provider::Microsoft,
                scopes: MICROSOFT_IMAP_SCOPES.to_string(),
                client_id: client_id.to_string(),
                tenant: None,
            },
            wanted: true,
        },
        MicrosoftGrant {
            audience: "Graph send",
            grant_name: graph_grant(account),
            grant: Grant {
                provider: Provider::Microsoft,
                scopes: MICROSOFT_GRAPH_SEND_SCOPES.to_string(),
                client_id: delegated
                    .and_then(|graph| graph.client_id.as_deref())
                    .unwrap_or(client_id)
                    .to_string(),
                tenant: delegated
                    .and_then(|graph| graph.tenant.clone()),
            },
            wanted: delegated.is_some(),
        },
    ]
}

fn login_microsoft(
    account: &str,
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
    for spec in microsoft_grants(account, client_id, graph) {
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
    client_id: &str,
    store: &TokenStore,
) -> Result<(), String> {
    let tokens = pkce_loopback_flow(
        &Grant {
            provider: Provider::Google,
            scopes: GOOGLE_MAIL_SCOPES.to_string(),
            client_id: client_id.to_string(),
            tenant: None,
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

    fn graph_config(
        auth: antiphon_config::GraphAuth,
        tenant: Option<&str>,
        client_id: Option<&str>,
    ) -> antiphon_config::Graph {
        antiphon_config::Graph {
            send: true,
            tenant: tenant.map(str::to_string),
            client_id: client_id.map(str::to_string),
            auth,
            secret_cmd: None,
        }
    }

    #[test]
    fn a_delegated_graph_grant_carries_tenant_and_client_id() {
        let graph = graph_config(
            antiphon_config::GraphAuth::Delegated,
            Some("tenant-1"),
            Some("graph-app"),
        );
        let grants = microsoft_grants("work", "imap-app", Some(&graph));
        let send = &grants[1];
        assert!(send.wanted);
        assert_eq!(send.grant.client_id, "graph-app");
        assert_eq!(send.grant.tenant.as_deref(), Some("tenant-1"));
        assert_eq!(grants[0].grant.client_id, "imap-app");
        assert_eq!(grants[0].grant.tenant, None);
    }

    #[test]
    fn a_delegated_grant_falls_back_to_the_imap_client_id() {
        let graph = graph_config(
            antiphon_config::GraphAuth::Delegated,
            None,
            None,
        );
        let grants = microsoft_grants("work", "imap-app", Some(&graph));
        assert_eq!(grants[1].grant.client_id, "imap-app");
    }

    #[test]
    fn app_only_graph_wants_no_interactive_grant() {
        let graph = graph_config(
            antiphon_config::GraphAuth::AppOnly,
            Some("tenant-1"),
            Some("graph-app"),
        );
        let grants = microsoft_grants("work", "imap-app", Some(&graph));
        assert!(!grants[1].wanted);
        assert!(grants[0].wanted);
    }

    #[test]
    fn no_graph_table_wants_only_the_imap_grant() {
        let grants = microsoft_grants("work", "imap-app", None);
        assert!(grants[0].wanted);
        assert!(!grants[1].wanted);
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
