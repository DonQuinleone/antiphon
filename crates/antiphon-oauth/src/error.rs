use std::fmt;

use oauth2::basic::BasicErrorResponseType;
use oauth2::{
    DeviceCodeErrorResponse, DeviceCodeErrorResponseType,
    ErrorResponse, HttpClientError, RequestTokenError,
    StandardErrorResponse,
};

type HttpError = HttpClientError<reqwest::Error>;
type BasicErrorResponse = StandardErrorResponse<BasicErrorResponseType>;

#[derive(Debug)]
pub enum OauthError {
    Declined(String),
    ExpiredDeviceCode,
    InvalidClient(String),
    InvalidGrant(String),
    Network(String),
    Protocol(String),
    NoRefreshToken,
    StateMismatch,
    Loopback(String),
    UnsupportedFlow(String),
    BadGrantName(String),
    NoStoredToken(String),
    Store(String),
}

impl fmt::Display for OauthError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OauthError::Declined(detail) => {
                write!(
                    out,
                    "the user declined authorisation \
                     (access_denied): {detail}"
                )
            }
            OauthError::ExpiredDeviceCode => {
                write!(
                    out,
                    "the device code expired before the sign-in \
                     was completed (expired_token)"
                )
            }
            OauthError::InvalidClient(detail) => {
                write!(
                    out,
                    "the provider rejected the client id \
                     (invalid_client): {detail}"
                )
            }
            OauthError::InvalidGrant(detail) => {
                write!(
                    out,
                    "the grant was rejected, so a fresh sign-in \
                     is needed (invalid_grant): {detail}"
                )
            }
            OauthError::Network(detail) => {
                write!(
                    out,
                    "the token endpoint could not be reached: \
                     {detail}"
                )
            }
            OauthError::Protocol(detail) => {
                write!(
                    out,
                    "the provider sent an unexpected response: \
                     {detail}"
                )
            }
            OauthError::NoRefreshToken => {
                write!(
                    out,
                    "the provider returned no refresh token; \
                     check that the scopes request offline access"
                )
            }
            OauthError::StateMismatch => {
                write!(
                    out,
                    "the loopback callback carried the wrong \
                     state parameter; the response was discarded"
                )
            }
            OauthError::Loopback(detail) => {
                write!(out, "the loopback listener failed: {detail}")
            }
            OauthError::UnsupportedFlow(detail) => {
                write!(out, "{detail}")
            }
            OauthError::BadGrantName(name) => {
                write!(
                    out,
                    "grant name {name:?} is not a plain file \
                     name"
                )
            }
            OauthError::NoStoredToken(name) => {
                write!(out, "no stored token for grant {name}")
            }
            OauthError::Store(detail) => {
                write!(out, "the token store failed: {detail}")
            }
        }
    }
}

impl std::error::Error for OauthError {}

pub(crate) fn map_token_error(
    error: RequestTokenError<HttpError, BasicErrorResponse>,
) -> OauthError {
    map_request_error(error, |response| {
        map_basic(response.error(), describe(&response))
    })
}

pub(crate) fn map_device_error(
    error: RequestTokenError<HttpError, DeviceCodeErrorResponse>,
) -> OauthError {
    map_request_error(error, map_device_response)
}

fn map_request_error<R: ErrorResponse + 'static>(
    error: RequestTokenError<HttpError, R>,
    map_response: impl Fn(R) -> OauthError,
) -> OauthError {
    match error {
        RequestTokenError::ServerResponse(response) => {
            map_response(response)
        }
        RequestTokenError::Request(cause) => {
            OauthError::Network(cause.to_string())
        }
        RequestTokenError::Parse(cause, _) => {
            OauthError::Protocol(cause.to_string())
        }
        RequestTokenError::Other(cause) => OauthError::Protocol(cause),
    }
}

fn map_device_response(
    response: DeviceCodeErrorResponse,
) -> OauthError {
    let detail = describe(&response);
    match response.error() {
        DeviceCodeErrorResponseType::AccessDenied => {
            OauthError::Declined(detail)
        }
        DeviceCodeErrorResponseType::ExpiredToken => {
            OauthError::ExpiredDeviceCode
        }
        DeviceCodeErrorResponseType::AuthorizationPending
        | DeviceCodeErrorResponseType::SlowDown => {
            OauthError::Protocol(format!(
                "polling ended while still pending: {detail}"
            ))
        }
        DeviceCodeErrorResponseType::Basic(basic) => {
            map_basic(basic, detail)
        }
    }
}

fn map_basic(
    kind: &BasicErrorResponseType,
    detail: String,
) -> OauthError {
    match kind {
        BasicErrorResponseType::InvalidClient => {
            OauthError::InvalidClient(detail)
        }
        BasicErrorResponseType::InvalidGrant => {
            OauthError::InvalidGrant(detail)
        }
        other => OauthError::Protocol(format!("{other}: {detail}")),
    }
}

fn describe<T>(response: &StandardErrorResponse<T>) -> String
where
    T: oauth2::ErrorResponseType + 'static,
{
    response.error_description().cloned().unwrap_or_default()
}
