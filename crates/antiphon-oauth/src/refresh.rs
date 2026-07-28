use oauth2::basic::BasicClient;
use oauth2::{ClientId, RefreshToken, TokenUrl};
use secrecy::ExposeSecret;

use crate::client::{bad_endpoint, http_client, scope_list};
use crate::error::map_token_error;
use crate::{Grant, OauthError, TokenSet, token};

pub fn refresh(
    tokens: &TokenSet,
    grant: &Grant,
) -> Result<TokenSet, OauthError> {
    refresh_at(&grant.token_url(), tokens, grant)
}

pub fn refresh_at(
    token_url: &str,
    tokens: &TokenSet,
    grant: &Grant,
) -> Result<TokenSet, OauthError> {
    let client =
        BasicClient::new(ClientId::new(grant.client_id.clone()))
            .set_token_uri(
                TokenUrl::new(token_url.to_string())
                    .map_err(bad_endpoint)?,
            );
    let http = http_client()?;
    let current = RefreshToken::new(
        tokens.refresh_token.expose_secret().to_string(),
    );
    let response = client
        .exchange_refresh_token(&current)
        .add_scopes(scope_list(&grant.scopes))
        .request(&http)
        .map_err(map_token_error)?;
    token::from_response(grant, &response, Some(&tokens.refresh_token))
}
