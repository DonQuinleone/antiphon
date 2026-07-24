use std::time::{SystemTime, UNIX_EPOCH};

use oauth2::TokenResponse;
use oauth2::basic::BasicTokenResponse;
use secrecy::SecretString;
use serde::Deserialize;

use crate::{Grant, OauthError, Provider};

#[derive(Clone, Debug, Deserialize)]
pub struct TokenSet {
    pub access_token: SecretString,
    pub refresh_token: SecretString,
    pub expires_at_unix: u64,
    pub scope: String,
    pub client_id: String,
    pub provider: Provider,
}

impl TokenSet {
    pub fn is_stale(&self, now_unix: u64, margin_secs: u64) -> bool {
        now_unix.saturating_add(margin_secs) >= self.expires_at_unix
    }
}

pub(crate) fn from_response(
    grant: &Grant,
    response: &BasicTokenResponse,
    previous_refresh: Option<&SecretString>,
) -> Result<TokenSet, OauthError> {
    let refresh_token =
        match (response.refresh_token(), previous_refresh) {
            (Some(fresh), _) => {
                SecretString::from(fresh.secret().clone())
            }
            (None, Some(kept)) => kept.clone(),
            (None, None) => {
                return Err(OauthError::NoRefreshToken);
            }
        };
    let scope = match response.scopes() {
        Some(scopes) => join_scopes(scopes),
        None => grant.scopes.clone(),
    };
    let lifetime = response
        .expires_in()
        .map(|left| left.as_secs())
        .unwrap_or(0);
    Ok(TokenSet {
        access_token: SecretString::from(
            response.access_token().secret().clone(),
        ),
        refresh_token,
        expires_at_unix: now_unix().saturating_add(lifetime),
        scope,
        client_id: grant.client_id.clone(),
        provider: grant.provider,
    })
}

pub(crate) fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

fn join_scopes(scopes: &[oauth2::Scope]) -> String {
    scopes
        .iter()
        .map(|scope| scope.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}
