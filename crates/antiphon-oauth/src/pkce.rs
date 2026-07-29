use std::net::{Ipv4Addr, TcpListener};

use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, CsrfToken, PkceCodeChallenge,
    RedirectUrl, TokenUrl,
};

use crate::client::{bad_endpoint, http_client, scope_list};
use crate::error::map_token_error;
use crate::{
    BrowserPrompt, GOOGLE_AUTH_URL, Grant, OauthError, Provider,
    TokenSet, loopback, microsoft_auth_url, token,
};

struct Consent {
    auth_url: String,
    /// Google only issues a refresh token when asked for
    /// offline access with forced consent; Microsoft carries
    /// that in the offline_access scope instead.
    extra_params: &'static [(&'static str, &'static str)],
}

fn consent_for(grant: &Grant) -> Consent {
    match grant.provider {
        Provider::Google => Consent {
            auth_url: GOOGLE_AUTH_URL.to_string(),
            extra_params: &[
                ("access_type", "offline"),
                ("prompt", "consent"),
            ],
        },
        Provider::Microsoft => Consent {
            auth_url: microsoft_auth_url(grant.tenant.as_deref()),
            extra_params: &[],
        },
    }
}

/// Microsoft app registrations need a Mobile and desktop
/// platform redirect of http://localhost for this flow; Entra
/// treats a localhost loopback as matching on any port, whereas
/// 127.0.0.1 must match the port exactly. The listener still
/// binds the loopback address, which localhost resolves to. The
/// device-code flow remains available where the redirect is not
/// set.
pub fn pkce_loopback_flow(
    grant: &Grant,
    on_prompt: &dyn Fn(&BrowserPrompt),
) -> Result<TokenSet, OauthError> {
    let consent = consent_for(grant);
    flow_at(
        &consent.auth_url,
        &grant.token_url(),
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
                RedirectUrl::new(format!("http://localhost:{port}/"))
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
