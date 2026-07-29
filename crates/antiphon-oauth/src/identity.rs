//! Guards against a multi-tenant sign-in silently returning a
//! different account's token. Microsoft's /common consent lets
//! the browser hand back whichever account already has a session,
//! so a sign-in meant for one mailbox can mint a valid token for
//! another. Office365 then rejects that token at IMAP with "User
//! is authenticated but not connected"; verifying the token's own
//! identity at sign-in turns that silent failure into a clear
//! error naming the account that was actually authorised.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;

use crate::{OauthError, Provider};

#[derive(Deserialize)]
struct IdentityClaims {
    upn: Option<String>,
    unique_name: Option<String>,
    preferred_username: Option<String>,
    email: Option<String>,
}

/// Rejects a freshly minted token whose signed-in identity does
/// not match the account it was requested for. Only Microsoft is
/// checked: Google issues opaque (non-JWT) access tokens and its
/// single-tenant consent cannot return another account. A token
/// that cannot be read is allowed through rather than blocking a
/// sign-in on a parsing gap.
pub(crate) fn verify(
    provider: Provider,
    expected: Option<&str>,
    access_token: &str,
) -> Result<(), OauthError> {
    if provider != Provider::Microsoft {
        return Ok(());
    }
    let Some(expected) = nonempty(expected) else {
        return Ok(());
    };
    let Some(signed_in) = token_identity(access_token) else {
        return Ok(());
    };
    if signed_in.eq_ignore_ascii_case(expected) {
        return Ok(());
    }
    Err(OauthError::IdentityMismatch {
        expected: expected.to_string(),
        signed_in,
    })
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|text| !text.is_empty())
}

/// The signed-in address from a JWT access token's payload, taken
/// from the first identity claim that carries one.
fn token_identity(access_token: &str) -> Option<String> {
    let payload = access_token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let claims: IdentityClaims =
        serde_json::from_slice(&bytes).ok()?;
    claims
        .upn
        .or(claims.unique_name)
        .or(claims.preferred_username)
        .or(claims.email)
        .map(|address| address.trim().to_string())
        .filter(|address| !address.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jwt(payload: &str) -> String {
        let body = URL_SAFE_NO_PAD.encode(payload.as_bytes());
        format!("header.{body}.signature")
    }

    #[test]
    fn matching_identity_passes() {
        let token = jwt(r#"{"upn":"quin@example.com"}"#);
        assert!(
            verify(
                Provider::Microsoft,
                Some("quin@example.com"),
                &token,
            )
            .is_ok()
        );
    }

    #[test]
    fn case_only_difference_still_matches() {
        let token = jwt(r#"{"upn":"Quin@Example.com"}"#);
        assert!(
            verify(
                Provider::Microsoft,
                Some("quin@example.com"),
                &token,
            )
            .is_ok()
        );
    }

    #[test]
    fn a_different_account_is_rejected() {
        let token = jwt(r#"{"upn":"other@elsewhere.org"}"#);
        let error = verify(
            Provider::Microsoft,
            Some("quin@example.com"),
            &token,
        )
        .expect_err("wrong account rejected");
        match error {
            OauthError::IdentityMismatch {
                expected,
                signed_in,
            } => {
                assert_eq!(expected, "quin@example.com");
                assert_eq!(signed_in, "other@elsewhere.org");
            }
            other => panic!("wrong error: {other:?}"),
        }
    }

    #[test]
    fn falls_back_through_the_identity_claims() {
        let token = jwt(r#"{"preferred_username":"quin@example.com"}"#);
        assert!(
            verify(
                Provider::Microsoft,
                Some("quin@example.com"),
                &token,
            )
            .is_ok()
        );
    }

    #[test]
    fn google_is_never_checked() {
        assert!(
            verify(Provider::Google, Some("quin@example.com"), "opaque")
                .is_ok()
        );
    }

    #[test]
    fn an_unreadable_token_does_not_block() {
        assert!(
            verify(
                Provider::Microsoft,
                Some("quin@example.com"),
                "not-a-jwt",
            )
            .is_ok()
        );
    }

    #[test]
    fn no_expected_identity_skips_the_check() {
        let token = jwt(r#"{"upn":"other@elsewhere.org"}"#);
        assert!(verify(Provider::Microsoft, None, &token).is_ok());
    }
}
