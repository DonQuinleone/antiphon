//! The grant set an account's sign-in must obtain, shared by
//! the `oauth login` command and the settings view's in-client
//! authorise so both always ask for exactly the same scopes.

use antiphon_config::{Dirs, Graph, GraphAuth, Oauth, OauthProvider};
use antiphon_oauth::{
    GOOGLE_MAIL_SCOPES, Grant, MICROSOFT_GRAPH_SEND_SCOPES,
    MICROSOFT_IMAP_SCOPES, Provider, TokenSet, TokenStore, graph_grant,
    imap_grant,
};
use antiphon_store::StoreLayout;

use crate::autostart;

pub(crate) struct GrantSpec {
    pub(crate) audience: &'static str,
    pub(crate) grant_name: String,
    pub(crate) grant: Grant,
    pub(crate) wanted: bool,
}

/// The environment overrides the account file, so a private
/// client_id can be swapped in without editing shared config;
/// with neither set the account keeps bring-your-own.
pub(crate) fn resolve_client_id(
    oauth: &Oauth,
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

/// Every grant the account's provider setup calls for; specs
/// not `wanted` are listed so callers can explain why they
/// are skipped (app-only Graph, say).
pub(crate) fn account_grants(
    account: &str,
    oauth: &Oauth,
    client_id: &str,
    graph: Option<&Graph>,
) -> Vec<GrantSpec> {
    match oauth.provider {
        OauthProvider::Google => vec![GrantSpec {
            audience: "mail",
            grant_name: imap_grant(account),
            grant: Grant {
                provider: Provider::Google,
                scopes: GOOGLE_MAIL_SCOPES.to_string(),
                client_id: client_id.to_string(),
                tenant: None,
            },
            wanted: true,
        }],
        OauthProvider::Microsoft => {
            microsoft_grants(account, client_id, graph)
        }
    }
}

/// The IMAP grant always uses the [oauth] registration on the
/// common endpoint; a delegated Graph grant may carry its own
/// registration and tenant, and app-only Graph needs no
/// interactive grant at all (client credentials at send time).
fn microsoft_grants(
    account: &str,
    client_id: &str,
    graph: Option<&Graph>,
) -> Vec<GrantSpec> {
    let sending = graph.filter(|graph| graph.send);
    let delegated =
        sending.filter(|graph| graph.auth == GraphAuth::Delegated);
    vec![
        GrantSpec {
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
        GrantSpec {
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

const SECONDS_PER_MINUTE: u64 = 60;

/// One grant's expiry, phrased for a status line.
pub(crate) fn expiry(tokens: &TokenSet, now: u64) -> String {
    if tokens.expires_at_unix <= now {
        return String::from(
            "access token expired (refreshes on the next sync)",
        );
    }
    let minutes = (tokens.expires_at_unix - now) / SECONDS_PER_MINUTE;
    format!("access token valid for {minutes} min")
}

/// The token store lives inside the vault, so the daemon must
/// be up (and the vault mounted) before it can be opened; this
/// is the same autostart path sendmail takes.
pub(crate) fn open_store(dirs: &Dirs) -> Result<TokenStore, String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn oauth_config(
        provider: OauthProvider,
        client_id: Option<&str>,
    ) -> Oauth {
        Oauth {
            provider,
            client_id: client_id.map(str::to_string),
        }
    }

    #[test]
    fn the_environment_overrides_the_account_client_id() {
        let oauth =
            oauth_config(OauthProvider::Microsoft, Some("from-config"));
        let id = resolve_client_id(&oauth, "work", |var| {
            (var == "ANTIPHON_MS_CLIENT_ID")
                .then(|| "from-env".to_string())
        })
        .unwrap();
        assert_eq!(id, "from-env");
    }

    #[test]
    fn without_an_override_the_account_client_id_stands() {
        let oauth =
            oauth_config(OauthProvider::Microsoft, Some("from-config"));
        let id = resolve_client_id(&oauth, "work", |_| None).unwrap();
        assert_eq!(id, "from-config");
    }

    #[test]
    fn neither_source_set_names_the_variable_in_the_error() {
        let oauth = oauth_config(OauthProvider::Microsoft, None);
        let error =
            resolve_client_id(&oauth, "work", |_| None).unwrap_err();
        assert!(error.contains("ANTIPHON_MS_CLIENT_ID"));
    }

    #[test]
    fn an_empty_override_is_ignored() {
        let oauth =
            oauth_config(OauthProvider::Microsoft, Some("from-config"));
        let id =
            resolve_client_id(&oauth, "work", |_| Some(String::new()))
                .unwrap();
        assert_eq!(id, "from-config");
    }

    fn graph_config(
        auth: GraphAuth,
        tenant: Option<&str>,
        client_id: Option<&str>,
    ) -> Graph {
        Graph {
            send: true,
            tenant: tenant.map(str::to_string),
            client_id: client_id.map(str::to_string),
            auth,
            secret_cmd: None,
        }
    }

    #[test]
    fn a_google_account_wants_one_mail_grant() {
        let oauth =
            oauth_config(OauthProvider::Google, Some("google-app"));
        let grants = account_grants("work", &oauth, "google-app", None);
        assert_eq!(grants.len(), 1);
        assert!(grants[0].wanted);
        assert_eq!(grants[0].grant_name, "work-imap");
        assert_eq!(grants[0].grant.provider, Provider::Google);
        assert_eq!(grants[0].grant.scopes, GOOGLE_MAIL_SCOPES);
    }

    fn microsoft_account_grants(
        graph: Option<&Graph>,
    ) -> Vec<GrantSpec> {
        let oauth =
            oauth_config(OauthProvider::Microsoft, Some("imap-app"));
        account_grants("work", &oauth, "imap-app", graph)
    }

    #[test]
    fn a_delegated_graph_grant_carries_tenant_and_client_id() {
        let graph = graph_config(
            GraphAuth::Delegated,
            Some("tenant-1"),
            Some("graph-app"),
        );
        let grants = microsoft_account_grants(Some(&graph));
        let send = &grants[1];
        assert!(send.wanted);
        assert_eq!(send.grant.client_id, "graph-app");
        assert_eq!(send.grant.tenant.as_deref(), Some("tenant-1"));
        assert_eq!(grants[0].grant.client_id, "imap-app");
        assert_eq!(grants[0].grant.tenant, None);
    }

    #[test]
    fn a_delegated_grant_falls_back_to_the_imap_client_id() {
        let graph = graph_config(GraphAuth::Delegated, None, None);
        let grants = microsoft_account_grants(Some(&graph));
        assert_eq!(grants[1].grant.client_id, "imap-app");
    }

    #[test]
    fn app_only_graph_wants_no_interactive_grant() {
        let graph = graph_config(
            GraphAuth::AppOnly,
            Some("tenant-1"),
            Some("graph-app"),
        );
        let grants = microsoft_account_grants(Some(&graph));
        assert!(!grants[1].wanted);
        assert!(grants[0].wanted);
    }

    #[test]
    fn no_graph_table_wants_only_the_imap_grant() {
        let grants = microsoft_account_grants(None);
        assert!(grants[0].wanted);
        assert!(!grants[1].wanted);
    }
}
