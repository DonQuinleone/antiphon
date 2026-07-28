use std::thread;
use std::time::Duration;

use oauth2::basic::BasicClient;
use oauth2::{
    ClientId, DeviceAuthorizationUrl,
    StandardDeviceAuthorizationResponse, TokenUrl,
};

use crate::client::{bad_endpoint, http_client, scope_list};
use crate::error::{map_device_error, map_token_error};
use crate::{
    Grant, OauthError, Provider, TokenSet, VerificationPrompt,
    microsoft_device_auth_url, token,
};

pub fn device_code_flow(
    grant: &Grant,
    on_prompt: &dyn Fn(&VerificationPrompt),
) -> Result<TokenSet, OauthError> {
    if grant.provider != Provider::Microsoft {
        return Err(OauthError::UnsupportedFlow(format!(
            "{} has no device-code flow; use \
             pkce_loopback_flow",
            grant.provider
        )));
    }
    flow_at(
        &microsoft_device_auth_url(grant.tenant.as_deref()),
        &grant.token_url(),
        grant,
        on_prompt,
        thread::sleep,
    )
}

pub(crate) fn flow_at(
    device_auth_url: &str,
    token_url: &str,
    grant: &Grant,
    on_prompt: &dyn Fn(&VerificationPrompt),
    sleep: fn(Duration),
) -> Result<TokenSet, OauthError> {
    let client =
        BasicClient::new(ClientId::new(grant.client_id.clone()))
            .set_device_authorization_url(
                DeviceAuthorizationUrl::new(
                    device_auth_url.to_string(),
                )
                .map_err(bad_endpoint)?,
            )
            .set_token_uri(
                TokenUrl::new(token_url.to_string())
                    .map_err(bad_endpoint)?,
            );
    let http = http_client()?;
    let authorization: StandardDeviceAuthorizationResponse = client
        .exchange_device_code()
        .add_scopes(scope_list(&grant.scopes))
        .request(&http)
        .map_err(map_token_error)?;
    on_prompt(&VerificationPrompt {
        verification_url: authorization.verification_uri().to_string(),
        user_code: authorization.user_code().secret().clone(),
    });
    let response = client
        .exchange_device_access_token(&authorization)
        .request(&http, sleep, None)
        .map_err(map_device_error)?;
    token::from_response(grant, &response, None)
}
