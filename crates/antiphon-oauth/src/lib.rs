mod app_only;
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
#[cfg(any(test, feature = "stub"))]
pub mod stub;

#[cfg(any(test, feature = "stub"))]
pub use app_only::app_only_token_at;
pub use app_only::{MICROSOFT_GRAPH_APP_SCOPES, app_only_token};
pub use device::device_code_flow;
pub use error::OauthError;
pub use pkce::pkce_loopback_flow;
pub use refresh::refresh;
#[cfg(any(test, feature = "stub"))]
pub use refresh::refresh_at;
pub use store::TokenStore;
pub use token::TokenSet;

use std::fmt;

use serde::{Deserialize, Serialize};

const MICROSOFT_AUTHORITY: &str = "https://login.microsoftonline.com";
/// The multi-tenant endpoint; tenant-restricted app
/// registrations refuse it and need their tenant named.
const COMMON_TENANT: &str = "common";

pub fn microsoft_auth_url(tenant: Option<&str>) -> String {
    microsoft_endpoint(tenant, "authorize")
}

pub fn microsoft_token_url(tenant: Option<&str>) -> String {
    microsoft_endpoint(tenant, "token")
}

pub fn microsoft_device_auth_url(tenant: Option<&str>) -> String {
    microsoft_endpoint(tenant, "devicecode")
}

fn microsoft_endpoint(tenant: Option<&str>, leaf: &str) -> String {
    format!(
        "{MICROSOFT_AUTHORITY}/{}/oauth2/v2.0/{leaf}",
        tenant.unwrap_or(COMMON_TENANT)
    )
}

pub const GOOGLE_AUTH_URL: &str =
    "https://accounts.google.com/o/oauth2/v2/auth";
pub const GOOGLE_TOKEN_URL: &str =
    "https://oauth2.googleapis.com/token";

pub const MICROSOFT_IMAP_SCOPES: &str = "offline_access \
     https://outlook.office.com/IMAP.AccessAsUser.All \
     https://outlook.office.com/SMTP.Send";
pub const MICROSOFT_GRAPH_SEND_SCOPES: &str = "offline_access \
     https://graph.microsoft.com/Mail.Send";
pub const GOOGLE_MAIL_SCOPES: &str = "https://mail.google.com/";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Microsoft,
    Google,
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
    /// Microsoft only; `None` uses the multi-tenant /common/
    /// endpoints. Google ignores it.
    pub tenant: Option<String>,
}

impl Grant {
    pub(crate) fn token_url(&self) -> String {
        match self.provider {
            Provider::Microsoft => {
                microsoft_token_url(self.tenant.as_deref())
            }
            Provider::Google => GOOGLE_TOKEN_URL.to_string(),
        }
    }
}

pub struct VerificationPrompt {
    pub verification_url: String,
    pub user_code: String,
}

pub struct BrowserPrompt {
    pub consent_url: String,
}

pub fn imap_grant(account: &str) -> String {
    format!("{account}-imap")
}

pub fn graph_grant(account: &str) -> String {
    format!("{account}-graph")
}
