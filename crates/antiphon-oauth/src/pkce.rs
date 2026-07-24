use std::net::{Ipv4Addr, TcpListener};

use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, CsrfToken, PkceCodeChallenge,
    RedirectUrl, TokenUrl,
};

use crate::client::{bad_endpoint, http_client, scope_list};
use crate::error::map_token_error;
use crate::{
    BrowserPrompt, GOOGLE_AUTH_URL, GOOGLE_TOKEN_URL, Grant,
    OauthError, Provider, TokenSet, loopback, token,
};

pub fn pkce_loopback_flow(
    grant: &Grant,
    on_prompt: &dyn Fn(&BrowserPrompt),
) -> Result<TokenSet, OauthError> {
    if grant.provider != Provider::Google {
        return Err(OauthError::UnsupportedFlow(format!(
            "{} has no PKCE loopback flow; use \
             device_code_flow",
            grant.provider
        )));
    }
    flow_at(GOOGLE_AUTH_URL, GOOGLE_TOKEN_URL, grant, on_prompt)
}

pub(crate) fn flow_at(
    auth_url: &str,
    token_url: &str,
    grant: &Grant,
    on_prompt: &dyn Fn(&BrowserPrompt),
) -> Result<TokenSet, OauthError> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .map_err(|error| OauthError::Loopback(error.to_string()))?;
    let port = listener
        .local_addr()
        .map_err(|error| OauthError::Loopback(error.to_string()))?
        .port();
    let client =
        BasicClient::new(ClientId::new(grant.client_id.clone()))
            .set_auth_uri(
                AuthUrl::new(auth_url.to_string())
                    .map_err(bad_endpoint)?,
            )
            .set_token_uri(
                TokenUrl::new(token_url.to_string())
                    .map_err(bad_endpoint)?,
            )
            .set_redirect_uri(
                RedirectUrl::new(format!("http://127.0.0.1:{port}/"))
                    .map_err(bad_endpoint)?,
            );
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let (consent_url, state) = client
        .authorize_url(CsrfToken::new_random)
        .add_scopes(scope_list(&grant.scopes))
        .add_extra_param("access_type", "offline")
        .add_extra_param("prompt", "consent")
        .set_pkce_challenge(challenge)
        .url();
    on_prompt(&BrowserPrompt {
        consent_url: consent_url.to_string(),
    });
    let code = loopback::wait_for_code(&listener, state.secret())?;
    let http = http_client()?;
    let response = client
        .exchange_code(AuthorizationCode::new(code))
        .set_pkce_verifier(verifier)
        .request(&http)
        .map_err(map_token_error)?;
    token::from_response(grant, &response, None)
}
