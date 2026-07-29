use antiphon_oauth::{
    Grant, OauthError, TokenSet, TokenStore, refresh,
};
use antiphon_store::StoreLayout;
use secrecy::ExposeSecret;

use crate::accounts::OauthAccount;
use crate::mailflow::now_unix;

/// An access token this close to expiry is refreshed up front
/// rather than risked against the server's clock.
pub(crate) const REFRESH_MARGIN_SECS: u64 = 5 * 60;

pub(crate) type Refresh<'a> =
    &'a dyn Fn(&TokenSet, &Grant) -> Result<TokenSet, OauthError>;

/// Resolves a live IMAP access token for an OAuth account,
/// shared by the sync pass and the IDLE watchers.
pub(crate) fn imap_access_token(
    layout: &StoreLayout,
    spec: &OauthAccount,
    force_refresh: bool,
) -> Result<String, String> {
    let store =
        TokenStore::open(layout.tokens_dir()).map_err(|error| {
            format!("{}: token store: {error}", spec.name)
        })?;
    if force_refresh {
        return refreshed_token(
            &store,
            &spec.grant_name(),
            &spec.name,
            Some(spec.user.as_str()),
            &refresh,
        );
    }
    access_token(
        &store,
        &spec.grant_name(),
        &spec.name,
        Some(spec.user.as_str()),
        now_unix(),
        &refresh,
    )
}

pub(crate) fn access_token(
    store: &TokenStore,
    grant_name: &str,
    account: &str,
    login_hint: Option<&str>,
    now_unix: u64,
    refresh: Refresh,
) -> Result<String, String> {
    let stored = load(store, grant_name, account)?;
    if !stored.is_stale(now_unix, REFRESH_MARGIN_SECS) {
        return Ok(stored.access_token.expose_secret().to_string());
    }
    renew(store, grant_name, account, login_hint, &stored, refresh)
}

pub(crate) fn refreshed_token(
    store: &TokenStore,
    grant_name: &str,
    account: &str,
    login_hint: Option<&str>,
    refresh: Refresh,
) -> Result<String, String> {
    let stored = load(store, grant_name, account)?;
    renew(store, grant_name, account, login_hint, &stored, refresh)
}

fn load(
    store: &TokenStore,
    grant_name: &str,
    account: &str,
) -> Result<TokenSet, String> {
    match store.load(grant_name) {
        Ok(stored) => Ok(stored),
        Err(OauthError::NoStoredToken(_)) => Err(format!(
            "{account}: no OAuth token stored; run \
             `antiphon oauth login {account}`"
        )),
        Err(error) => Err(format!("{account}: {error}")),
    }
}

/// The provider may rotate the refresh token on redemption, so
/// the renewed set is persisted before the access token is
/// handed out for use.
fn renew(
    store: &TokenStore,
    grant_name: &str,
    account: &str,
    login_hint: Option<&str>,
    stored: &TokenSet,
    refresh: Refresh,
) -> Result<String, String> {
    let grant = Grant {
        provider: stored.provider,
        scopes: stored.scope.clone(),
        client_id: stored.client_id.clone(),
        tenant: stored.tenant.clone(),
        login_hint: login_hint.map(str::to_string),
    };
    let renewed = refresh(stored, &grant).map_err(|error| {
        format!("{account}: refreshing the OAuth token: {error}")
    })?;
    store.save(grant_name, &renewed).map_err(|error| {
        format!("{account}: persisting the refreshed token: {error}")
    })?;
    Ok(renewed.access_token.expose_secret().to_string())
}

#[cfg(test)]
mod tests {
    use antiphon_oauth::stub::{Stub, bad_request, ok};
    use antiphon_oauth::{Provider, TokenSet, refresh_at};
    use secrecy::{ExposeSecret, SecretString};

    use super::*;

    const NOW: u64 = 1_000_000;
    const FAR_FUTURE: u64 = NOW + 24 * 60 * 60;

    fn stored_set(expires_at_unix: u64) -> TokenSet {
        TokenSet {
            access_token: SecretString::from("at-old"),
            refresh_token: SecretString::from("rt-old"),
            expires_at_unix,
            scope: "https://mail.google.com/".to_string(),
            client_id: "client-app".to_string(),
            provider: Provider::Google,
            tenant: None,
        }
    }

    fn store_with(
        dir: &tempfile::TempDir,
        tokens: &TokenSet,
    ) -> TokenStore {
        let store = TokenStore::open(dir.path()).expect("open");
        store.save("work-imap", tokens).expect("seed");
        store
    }

    fn no_refresh(
        _: &TokenSet,
        _: &Grant,
    ) -> Result<TokenSet, OauthError> {
        panic!("a fresh token must be used as-is");
    }

    fn rotated_token_body() -> &'static str {
        r#"{"access_token":"at-new",
            "token_type":"Bearer",
            "expires_in":3600,
            "refresh_token":"rt-new"}"#
    }

    #[test]
    fn a_fresh_token_is_used_without_refreshing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store_with(&dir, &stored_set(FAR_FUTURE));
        let token = access_token(
            &store,
            "work-imap",
            "work",
            None,
            NOW,
            &no_refresh,
        )
        .expect("fresh token");
        assert_eq!(token, "at-old");
    }

    #[test]
    fn a_token_inside_the_margin_counts_as_stale() {
        let dir = tempfile::tempdir().expect("tempdir");
        let expiring = stored_set(NOW + REFRESH_MARGIN_SECS - 1);
        let store = store_with(&dir, &expiring);
        let stub = Stub::serve(vec![ok(rotated_token_body())]);
        let refresh = |tokens: &TokenSet, grant: &Grant| {
            refresh_at(
                &format!("{}/token", stub.base_url),
                tokens,
                grant,
            )
        };
        let token = access_token(
            &store,
            "work-imap",
            "work",
            None,
            NOW,
            &refresh,
        )
        .expect("refreshed token");
        assert_eq!(token, "at-new");
        stub.finish();
    }

    #[test]
    fn a_stale_token_is_refreshed_and_the_rotation_persisted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store_with(&dir, &stored_set(NOW - 1));
        let stub = Stub::serve(vec![ok(rotated_token_body())]);
        let refresh = |tokens: &TokenSet, grant: &Grant| {
            refresh_at(
                &format!("{}/token", stub.base_url),
                tokens,
                grant,
            )
        };
        let token = access_token(
            &store,
            "work-imap",
            "work",
            None,
            NOW,
            &refresh,
        )
        .expect("refreshed token");
        assert_eq!(token, "at-new");

        let persisted = store.load("work-imap").expect("persisted");
        assert_eq!(persisted.refresh_token.expose_secret(), "rt-new");
        assert_eq!(persisted.access_token.expose_secret(), "at-new");

        let requests = stub.finish();
        assert!(requests[0].contains("grant_type=refresh_token"));
        assert!(requests[0].contains("refresh_token=rt-old"));
    }

    #[test]
    fn a_failed_refresh_leaves_the_stored_set_untouched() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store_with(&dir, &stored_set(NOW - 1));
        let stub = Stub::serve(vec![bad_request(
            r#"{"error":"invalid_grant"}"#,
        )]);
        let refresh = |tokens: &TokenSet, grant: &Grant| {
            refresh_at(
                &format!("{}/token", stub.base_url),
                tokens,
                grant,
            )
        };
        let error = access_token(
            &store,
            "work-imap",
            "work",
            None,
            NOW,
            &refresh,
        )
        .expect_err("refresh fails");
        assert!(error.contains("work"));
        assert!(error.contains("invalid_grant"));

        let kept = store.load("work-imap").expect("kept");
        assert_eq!(kept.refresh_token.expose_secret(), "rt-old");
        stub.finish();
    }

    #[test]
    fn refreshed_token_renews_even_a_fresh_set() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store_with(&dir, &stored_set(FAR_FUTURE));
        let stub = Stub::serve(vec![ok(rotated_token_body())]);
        let refresh = |tokens: &TokenSet, grant: &Grant| {
            refresh_at(
                &format!("{}/token", stub.base_url),
                tokens,
                grant,
            )
        };
        let token = refreshed_token(
            &store,
            "work-imap",
            "work",
            None,
            &refresh,
        )
        .expect("forced refresh");
        assert_eq!(token, "at-new");
        stub.finish();
    }

    #[test]
    fn a_missing_token_names_the_login_command() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = TokenStore::open(dir.path()).expect("open");
        let error = access_token(
            &store,
            "work-imap",
            "work",
            None,
            NOW,
            &no_refresh,
        )
        .expect_err("missing token");
        assert!(error.contains("antiphon oauth login work"));
    }
}
