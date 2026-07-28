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
    MICROSOFT_AUTH_URL, MICROSOFT_TOKEN_URL, OauthError, Provider,
    TokenSet, loopback, token,
};

struct Consent {
    auth_url: &'static str,
    token_url: &'static str,
    /// Google only issues a refresh token when asked for
    /// offline access with forced consent; Microsoft carries
    /// that in the offline_access scope instead.
    extra_params: &'static [(&'static str, &'static str)],
}

const CONSENTS: [(Provider, Consent); 2] = [
    (
        Provider::Google,
        Consent {
            auth_url: GOOGLE_AUTH_URL,
            token_url: GOOGLE_TOKEN_URL,
            extra_params: &[
                ("access_type", "offline"),
                ("prompt", "consent"),
            ],
        },
    ),
    (
        Provider::Microsoft,
        Consent {
            auth_url: MICROSOFT_AUTH_URL,
            token_url: MICROSOFT_TOKEN_URL,
            extra_params: &[],
        },
    ),
];

/// Microsoft app registrations need a Mobile and desktop
/// platform redirect of http://127.0.0.1 for this flow; the
/// device-code flow remains available where that is not set.
pub fn pkce_loopback_flow(
    grant: &Grant,
    on_prompt: &dyn Fn(&BrowserPrompt),
) -> Result<TokenSet, OauthError> {
    let consent = CONSENTS
        .iter()
        .find(|(provider, _)| *provider == grant.provider)
        .map(|(_, consent)| consent)
        .expect("every provider has a consent entry");
    flow_at(
        consent.auth_url,
        consent.token_url,
        consent.extra_params,
        grant,
        on_prompt,
    )
}

pub(crate) fn flow_at(
    auth_url: &str,
    token_url: &str,
    extra_params: &[(&str, &str)],
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
    let mut request = client
        .authorize_url(CsrfToken::new_random)
        .add_scopes(scope_list(&grant.scopes))
        .set_pkce_challenge(challenge);
    for (name, value) in extra_params {
        request = request.add_extra_param(*name, *value);
    }
    let (consent_url, state) = request.url();
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
