use oauth2::basic::BasicClient;
use oauth2::{ClientId, ClientSecret, TokenResponse, TokenUrl};
use secrecy::SecretString;

use crate::client::{bad_endpoint, http_client, scope_list};
use crate::error::map_token_error;
use crate::{OauthError, Provider, TokenSet, token};

pub const MICROSOFT_GRAPH_APP_SCOPES: &str =
    "https://graph.microsoft.com/.default";

/// App-only (client_credentials) tokens require a concrete
/// tenant; the /common/ endpoint refuses the grant.
pub fn microsoft_tenant_token_url(tenant: &str) -> String {
    format!(
        "https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token"
    )
}

/// Fetches an app-only Graph token: no user, no consent screen
/// and no refresh token, so callers request a fresh one
/// whenever the last goes stale instead of refreshing.
pub fn app_only_token(
    tenant: &str,
    client_id: &str,
    client_secret: &SecretString,
) -> Result<TokenSet, OauthError> {
    app_only_token_at(
        &microsoft_tenant_token_url(tenant),
        client_id,
        client_secret,
    )
}

pub fn app_only_token_at(
    token_url: &str,
    client_id: &str,
    client_secret: &SecretString,
) -> Result<TokenSet, OauthError> {
    use secrecy::ExposeSecret;

    // Microsoft documents the secret in the request body, not
    // the HTTP Basic header this library defaults to.
    let client = BasicClient::new(ClientId::new(client_id.to_owned()))
        .set_client_secret(ClientSecret::new(
            client_secret.expose_secret().to_owned(),
        ))
        .set_auth_type(oauth2::AuthType::RequestBody)
        .set_token_uri(
            TokenUrl::new(token_url.to_owned())
                .map_err(bad_endpoint)?,
        );
    let http = http_client()?;
    let response = client
        .exchange_client_credentials()
        .add_scopes(scope_list(MICROSOFT_GRAPH_APP_SCOPES))
        .request(&http)
        .map_err(map_token_error)?;
    let lifetime = response
        .expires_in()
        .map(|left| left.as_secs())
        .unwrap_or(0);
    Ok(TokenSet {
        access_token: SecretString::from(
            response.access_token().secret().clone(),
        ),
        refresh_token: SecretString::from(String::new()),
        expires_at_unix: token::now_unix().saturating_add(lifetime),
        scope: MICROSOFT_GRAPH_APP_SCOPES.to_owned(),
        client_id: client_id.to_owned(),
        provider: Provider::Microsoft,
    })
}
