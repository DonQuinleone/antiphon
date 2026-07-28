use std::io::Write;
use std::net::TcpStream;
use std::sync::Mutex;
use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};

use crate::stub::{Stub, bad_request, ok};
use crate::{
    GOOGLE_MAIL_SCOPES, Grant, MICROSOFT_IMAP_SCOPES, OauthError,
    Provider, TokenSet, device, pkce, query, refresh, token,
};

const EXPIRY_TOLERANCE_SECS: u64 = 5;

fn no_sleep(_: Duration) {}

fn microsoft_grant() -> Grant {
    Grant {
        provider: Provider::Microsoft,
        scopes: MICROSOFT_IMAP_SCOPES.to_string(),
        client_id: "client-app".to_string(),
        tenant: None,
    }
}

fn google_grant() -> Grant {
    Grant {
        provider: Provider::Google,
        scopes: GOOGLE_MAIL_SCOPES.to_string(),
        client_id: "client-app".to_string(),
        tenant: None,
    }
}

fn device_auth_body() -> &'static str {
    r#"{"device_code":"dev-123",
        "user_code":"ABCD-1234",
        "verification_uri":"https://example.com/device",
        "expires_in":300,
        "interval":1}"#
}

fn token_body(refresh_token: Option<&str>) -> String {
    let rotation = match refresh_token {
        Some(value) => {
            format!(r#","refresh_token":"{value}""#)
        }
        None => String::new(),
    };
    format!(
        r#"{{"access_token":"at-1",
            "token_type":"Bearer",
            "expires_in":3600{rotation}}}"#
    )
}

fn stored_set(refresh_token: &str) -> TokenSet {
    TokenSet {
        access_token: SecretString::from("at-old"),
        refresh_token: SecretString::from(refresh_token.to_string()),
        expires_at_unix: 0,
        scope: MICROSOFT_IMAP_SCOPES.to_string(),
        client_id: "client-app".to_string(),
        provider: Provider::Microsoft,
        tenant: None,
    }
}

#[test]
fn device_flow_polls_until_success() {
    let stub = Stub::serve(vec![
        ok(device_auth_body()),
        bad_request(r#"{"error":"authorization_pending"}"#),
        ok(&token_body(Some("rt-1"))),
    ]);
    let prompt: Mutex<Option<(String, String)>> = Mutex::new(None);
    let grant = microsoft_grant();
    let before = token::now_unix();
    let tokens = device::flow_at(
        &format!("{}/devicecode", stub.base_url),
        &format!("{}/token", stub.base_url),
        &grant,
        &|seen| {
            *prompt.lock().expect("prompt lock") = Some((
                seen.verification_url.clone(),
                seen.user_code.clone(),
            ));
        },
        no_sleep,
    )
    .expect("device flow");

    let (url, code) = prompt
        .lock()
        .expect("prompt lock")
        .clone()
        .expect("prompt shown");
    assert_eq!(url, "https://example.com/device");
    assert_eq!(code, "ABCD-1234");
    assert_eq!(tokens.access_token.expose_secret(), "at-1");
    assert_eq!(tokens.refresh_token.expose_secret(), "rt-1");
    assert_eq!(tokens.client_id, "client-app");
    assert_eq!(tokens.provider, Provider::Microsoft);

    let lower = before + 3600;
    let upper = token::now_unix() + 3600 + EXPIRY_TOLERANCE_SECS;
    assert!(tokens.expires_at_unix >= lower);
    assert!(tokens.expires_at_unix <= upper);

    let requests = stub.finish();
    assert_eq!(requests.len(), 3);
    assert!(requests[0].contains("client_id=client-app"));
    assert!(requests[0].contains("offline_access"));
    assert!(requests[1].contains("device_code"));
    assert!(requests[2].contains("dev-123"));
}

#[test]
fn device_flow_reports_a_decline() {
    let stub = Stub::serve(vec![
        ok(device_auth_body()),
        bad_request(r#"{"error":"access_denied"}"#),
    ]);
    let error = device::flow_at(
        &format!("{}/devicecode", stub.base_url),
        &format!("{}/token", stub.base_url),
        &microsoft_grant(),
        &|_| {},
        no_sleep,
    )
    .expect_err("declined");
    assert!(matches!(error, OauthError::Declined(_)));
    stub.finish();
}

#[test]
fn device_flow_needs_microsoft() {
    let error = crate::device_code_flow(&google_grant(), &|_| {})
        .expect_err("wrong provider");
    assert!(matches!(error, OauthError::UnsupportedFlow(_)));
}

#[test]
fn refresh_carries_the_rotated_token() {
    let stub = Stub::serve(vec![ok(&token_body(Some("rt-new")))]);
    let old = stored_set("rt-old");
    let renewed = refresh::refresh_at(
        &format!("{}/token", stub.base_url),
        &old,
        &microsoft_grant(),
    )
    .expect("refresh");

    assert_eq!(renewed.refresh_token.expose_secret(), "rt-new");
    assert_eq!(renewed.access_token.expose_secret(), "at-1");

    let requests = stub.finish();
    assert!(requests[0].contains("grant_type=refresh_token"));
    assert!(requests[0].contains("refresh_token=rt-old"));
}

#[test]
fn refresh_keeps_the_old_token_when_omitted() {
    let stub = Stub::serve(vec![ok(&token_body(None))]);
    let old = stored_set("rt-old");
    let renewed = refresh::refresh_at(
        &format!("{}/token", stub.base_url),
        &old,
        &microsoft_grant(),
    )
    .expect("refresh");
    assert_eq!(renewed.refresh_token.expose_secret(), "rt-old");
    stub.finish();
}

#[test]
fn pkce_flow_exchanges_the_loopback_code() {
    let stub = Stub::serve(vec![ok(&token_body(Some("rt-g")))]);
    let consent_seen: Mutex<Option<String>> = Mutex::new(None);
    let tokens = pkce::flow_at(
        "https://auth.example.com/consent",
        &format!("{}/token", stub.base_url),
        &[("access_type", "offline"), ("prompt", "consent")],
        &google_grant(),
        &|prompt| {
            *consent_seen.lock().expect("consent lock") =
                Some(prompt.consent_url.clone());
            complete_consent(&prompt.consent_url);
        },
    )
    .expect("pkce flow");

    assert_eq!(tokens.access_token.expose_secret(), "at-1");
    assert_eq!(tokens.refresh_token.expose_secret(), "rt-g");
    assert_eq!(tokens.provider, Provider::Google);

    let consent = consent_seen
        .lock()
        .expect("consent lock")
        .clone()
        .expect("consent url shown");
    assert!(consent.contains("code_challenge_method=S256"));
    assert!(consent.contains("access_type=offline"));
    assert!(consent.contains("prompt=consent"));

    let requests = stub.finish();
    assert!(requests[0].contains("grant_type=authorization_code"));
    assert!(requests[0].contains("code=pkce-code"));
    assert!(requests[0].contains("code_verifier="));
}

#[test]
fn microsoft_pkce_consent_skips_the_google_params() {
    let stub = Stub::serve(vec![ok(&token_body(Some("rt-m")))]);
    let consent_seen: Mutex<Option<String>> = Mutex::new(None);
    let tokens = pkce::flow_at(
        "https://auth.example.com/consent",
        &format!("{}/token", stub.base_url),
        &[],
        &microsoft_grant(),
        &|prompt| {
            *consent_seen.lock().expect("consent lock") =
                Some(prompt.consent_url.clone());
            complete_consent(&prompt.consent_url);
        },
    )
    .expect("pkce flow");
    assert_eq!(tokens.provider, Provider::Microsoft);
    assert_eq!(tokens.refresh_token.expose_secret(), "rt-m");

    let consent = consent_seen
        .lock()
        .expect("consent lock")
        .clone()
        .expect("consent url shown");
    assert!(consent.contains("code_challenge_method=S256"));
    assert!(!consent.contains("access_type"));
    stub.finish();
}

#[test]
fn app_only_exchanges_client_credentials() {
    let stub = Stub::serve(vec![ok(&token_body(None))]);
    let secret = SecretString::from("s3cret");
    let tokens = crate::app_only::app_only_token_at(
        &format!("{}/token", stub.base_url),
        Some("11111111-2222-3333-4444-555555555555"),
        "client-app",
        &secret,
    )
    .expect("app-only token");
    assert_eq!(tokens.access_token.expose_secret(), "at-1");
    assert!(tokens.refresh_token.expose_secret().is_empty());
    assert_eq!(tokens.provider, Provider::Microsoft);

    let requests = stub.finish();
    assert!(requests[0].contains("grant_type=client_credentials"));
    assert!(requests[0].contains("client_secret=s3cret"));
    assert!(requests[0].contains(".default"));
}

#[test]
fn the_tenant_token_url_names_the_tenant() {
    let url = crate::microsoft_token_url(Some(
        "11111111-2222-3333-4444-555555555555",
    ));
    assert_eq!(
        url,
        "https://login.microsoftonline.com/\
         11111111-2222-3333-4444-555555555555/oauth2/v2.0/token"
    );
}

#[test]
fn stale_tokens_are_detected_with_a_margin() {
    let mut tokens = stored_set("rt");
    tokens.expires_at_unix = 1_000;
    assert!(!tokens.is_stale(500, 100));
    assert!(tokens.is_stale(900, 100));
    assert!(tokens.is_stale(1_000, 0));
    assert!(tokens.is_stale(2_000, 0));
}

fn complete_consent(consent_url: &str) {
    let (_, raw_query) =
        consent_url.split_once('?').expect("consent query");
    let pairs = query::parse(raw_query);
    let state = query::get(&pairs, "state").expect("state param");
    let redirect =
        query::get(&pairs, "redirect_uri").expect("redirect param");
    assert!(query::get(&pairs, "code_challenge").is_some());

    let authority = redirect
        .strip_prefix("http://")
        .expect("loopback redirect")
        .trim_end_matches('/');
    let mut stream = TcpStream::connect(authority).expect("connect");
    let request =
        format!("GET /?code=pkce-code&state={state} HTTP/1.1\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .expect("write callback");
}
