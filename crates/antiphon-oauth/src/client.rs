use oauth2::Scope;
use reqwest::blocking::Client;
use reqwest::redirect::Policy;

use crate::OauthError;

pub(crate) fn http_client() -> Result<Client, OauthError> {
    Client::builder()
        .redirect(Policy::none())
        .build()
        .map_err(|error| OauthError::Network(error.to_string()))
}

pub(crate) fn scope_list(scopes: &str) -> Vec<Scope> {
    scopes
        .split_whitespace()
        .map(|scope| Scope::new(scope.to_string()))
        .collect()
}

pub(crate) fn bad_endpoint(
    error: impl std::fmt::Display,
) -> OauthError {
    OauthError::Protocol(format!("bad endpoint URL: {error}"))
}
