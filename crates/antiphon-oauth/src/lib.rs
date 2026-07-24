mod client;
mod device;
mod error;
mod loopback;
mod pkce;
mod query;
mod refresh;
mod store;
mod token;

#[cfg(test)]
mod flow_tests;
#[cfg(test)]
mod stub;

pub use device::device_code_flow;
pub use error::OauthError;
pub use pkce::pkce_loopback_flow;
pub use refresh::refresh;
pub use store::TokenStore;
pub use token::TokenSet;

use std::fmt;

use serde::{Deserialize, Serialize};

pub const MICROSOFT_DEVICE_AUTH_URL: &str =
    "https://login.microsoftonline.com/common/oauth2/v2.0/devicecode";
pub const MICROSOFT_TOKEN_URL: &str =
    "https://login.microsoftonline.com/common/oauth2/v2.0/token";
pub const GOOGLE_AUTH_URL: &str =
    "https://accounts.google.com/o/oauth2/v2/auth";
pub const GOOGLE_TOKEN_URL: &str =
    "https://oauth2.googleapis.com/token";

pub const MICROSOFT_IMAP_SCOPES: &str = "offline_access \
     https://outlook.office.com/IMAP.AccessAsUser.All";
pub const MICROSOFT_GRAPH_SEND_SCOPES: &str = "offline_access \
     https://graph.microsoft.com/Mail.Send";
pub const GOOGLE_MAIL_SCOPES: &str = "https://mail.google.com/";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Microsoft,
    Google,
}

impl Provider {
    pub const fn token_url(self) -> &'static str {
        match self {
            Provider::Microsoft => MICROSOFT_TOKEN_URL,
            Provider::Google => GOOGLE_TOKEN_URL,
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Provider::Microsoft => write!(out, "microsoft"),
            Provider::Google => write!(out, "google"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Grant {
    pub provider: Provider,
    pub scopes: String,
    pub client_id: String,
}

pub struct VerificationPrompt {
    pub verification_url: String,
    pub user_code: String,
}

pub struct BrowserPrompt {
    pub consent_url: String,
}
