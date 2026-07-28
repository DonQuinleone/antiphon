//! OAuth account state for the settings view and status line:
//! stored-grant classification, the revoke confirmation, and
//! the daemon's auth-failure report.

use antiphon_config::{Dirs, GraphAuth, NamedAccount};
use antiphon_oauth::{TokenSet, TokenStore, graph_grant, imap_grant};
use antiphon_store::StoreLayout;
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::app::App;
use super::settings::SettingsOutcome;
use crate::oauthgrants::expiry;

/// A token this close to expiry reads as due for a refresh;
/// the daemon renews it on its next pass.
const REFRESH_DUE_MARGIN_SECS: u64 = 300;
const SECONDS_PER_MINUTE: u64 = 60;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum OauthState {
    Ok { minutes_left: u64 },
    RefreshDue,
    NeedsSignIn,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct OauthInfo {
    pub(super) state: OauthState,
    pub(super) app_only: bool,
    pub(super) detail: String,
}

impl OauthInfo {
    /// The account row's marker, e.g. "oauth: ok (42 min)".
    pub(super) fn label(&self) -> String {
        let state = match &self.state {
            OauthState::Ok { minutes_left } => {
                format!("ok ({minutes_left} min)")
            }
            OauthState::RefreshDue => "refresh due".to_string(),
            OauthState::NeedsSignIn => "needs sign-in".to_string(),
        };
        match self.app_only {
            true => format!("oauth: {state} \u{b7} app-only"),
            false => format!("oauth: {state}"),
        }
    }
}

/// The account's headline state follows its IMAP grant: the
/// one every sync depends on. A daemon-reported failure wins
/// outright, since a stored token the provider refuses is as
/// dead as a missing one.
pub(super) fn classify(
    tokens: Option<&TokenSet>,
    now: u64,
    auth_failed: bool,
) -> OauthState {
    if auth_failed {
        return OauthState::NeedsSignIn;
    }
    let Some(tokens) = tokens else {
        return OauthState::NeedsSignIn;
    };
    if tokens.is_stale(now, REFRESH_DUE_MARGIN_SECS) {
        return OauthState::RefreshDue;
    }
    OauthState::Ok {
        minutes_left: (tokens.expires_at_unix - now)
            / SECONDS_PER_MINUTE,
    }
}

pub(super) fn info_for(
    entry: &NamedAccount,
    store: Option<&TokenStore>,
    auth_failures: &[String],
    now: u64,
) -> Option<OauthInfo> {
    entry.account.oauth.as_ref()?;
    let name = entry.account.account.name.as_str();
    let app_only = entry.account.graph.as_ref().is_some_and(|graph| {
        graph.send && graph.auth == GraphAuth::AppOnly
    });
    let imap = load(store, &imap_grant(name));
    let graph = load(store, &graph_grant(name));
    let failed = auth_failures.iter().any(|failure| failure == name);
    Some(OauthInfo {
        state: classify(imap.as_ref(), now, failed),
        app_only,
        detail: detail_line(imap.as_ref(), graph.as_ref(), now),
    })
}

fn load(store: Option<&TokenStore>, grant: &str) -> Option<TokenSet> {
    store?.load(grant).ok()
}

/// The selected account's scopes and expiries, one segment per
/// stored grant; empty when nothing is stored.
fn detail_line(
    imap: Option<&TokenSet>,
    graph: Option<&TokenSet>,
    now: u64,
) -> String {
    let mut segments = Vec::new();
    for (label, tokens) in [("imap", imap), ("graph", graph)] {
        let Some(tokens) = tokens else {
            continue;
        };
        segments.push(format!(
            "{label}: {} \u{b7} {}",
            tokens.scope,
            expiry(tokens, now)
        ));
    }
    segments.join("  ")
}

/// The token store without starting anything: `None` while the
/// vault is sealed or the store absent, which reads as
/// "needs sign-in" rather than an error.
pub(super) fn open_store_if_present(dirs: &Dirs) -> Option<TokenStore> {
    let layout = StoreLayout::new(dirs.store_root());
    if !layout.exists() {
        return None;
    }
    TokenStore::open(layout.tokens_dir()).ok()
}

/// Asks the daemon which accounts need a fresh sign-in,
/// leaving the last answer standing when it cannot be asked.
pub(super) fn refresh_auth_failures(app: &mut App) {
    if let Some(failures) = super::daemon::auth_failures() {
        app.auth_failures = failures;
    }
}

pub(super) fn arm_revoke(app: &mut App) {
    let Some(state) = app.settings.as_ref() else {
        return;
    };
    let Some(summary) = state.accounts.get(state.account_selected)
    else {
        return;
    };
    if summary.oauth.is_none() {
        app.notice =
            Some(format!("{} has no oauth grants", summary.name));
        return;
    }
    let name = summary.account_name.clone();
    if let Some(state) = app.settings.as_mut() {
        state.pending_revoke = Some(name);
    }
}

pub(super) fn feed_confirm_revoke(
    app: &mut App,
    key: KeyEvent,
    name: &str,
) -> SettingsOutcome {
    if matches!(key.code, KeyCode::Char('y' | 'Y')) {
        perform_revoke(app, name);
    }
    if let Some(state) = app.settings.as_mut() {
        state.pending_revoke = None;
    }
    SettingsOutcome::Stay
}

fn perform_revoke(app: &mut App, name: &str) {
    app.notice = Some(match revoke_grants(&app.dirs, name) {
        Ok(()) => format!("revoked the sign-in for {name}"),
        Err(error) => format!("revoke {name}: {error}"),
    });
    app.refresh_settings_accounts();
}

fn revoke_grants(dirs: &Dirs, name: &str) -> Result<(), String> {
    let store = open_store_if_present(dirs)
        .ok_or("the store is unavailable (vault sealed?)")?;
    for grant in [imap_grant(name), graph_grant(name)] {
        store.remove(&grant).map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use antiphon_oauth::Provider;
    use secrecy::SecretString;

    use super::super::testkit::TempDir;
    use super::*;

    const NOW: u64 = 1_000_000;

    fn tokens_expiring_at(expires_at_unix: u64) -> TokenSet {
        TokenSet {
            access_token: SecretString::from("at"),
            refresh_token: SecretString::from("rt"),
            expires_at_unix,
            scope: "https://mail.google.com/".to_string(),
            client_id: "app".to_string(),
            provider: Provider::Google,
            tenant: None,
        }
    }

    #[test]
    fn a_live_token_reads_ok_with_minutes_left() {
        let tokens = tokens_expiring_at(NOW + 42 * 60);
        assert_eq!(
            classify(Some(&tokens), NOW, false),
            OauthState::Ok { minutes_left: 42 }
        );
    }

    #[test]
    fn a_token_near_or_past_expiry_reads_refresh_due() {
        let expired = tokens_expiring_at(NOW - 10);
        assert_eq!(
            classify(Some(&expired), NOW, false),
            OauthState::RefreshDue
        );
        let closing = tokens_expiring_at(NOW + 60);
        assert_eq!(
            classify(Some(&closing), NOW, false),
            OauthState::RefreshDue,
            "inside the refresh margin"
        );
    }

    #[test]
    fn a_missing_grant_or_daemon_failure_needs_sign_in() {
        assert_eq!(classify(None, NOW, false), OauthState::NeedsSignIn);
        let live = tokens_expiring_at(NOW + 3600);
        assert_eq!(
            classify(Some(&live), NOW, true),
            OauthState::NeedsSignIn,
            "the daemon's report beats a stored token"
        );
    }

    #[test]
    fn labels_read_as_the_row_shows_them() {
        let ok = OauthInfo {
            state: OauthState::Ok { minutes_left: 42 },
            app_only: false,
            detail: String::new(),
        };
        assert_eq!(ok.label(), "oauth: ok (42 min)");
        let app_only = OauthInfo {
            state: OauthState::NeedsSignIn,
            app_only: true,
            detail: String::new(),
        };
        assert_eq!(
            app_only.label(),
            "oauth: needs sign-in \u{b7} app-only"
        );
    }

    #[test]
    fn the_detail_line_names_each_stored_grant() {
        let imap = tokens_expiring_at(NOW + 3600);
        let detail = detail_line(Some(&imap), None, NOW);
        assert!(detail.contains("imap: https://mail.google.com/"));
        assert!(detail.contains("60 min"));
        assert!(!detail.contains("graph"));
        assert_eq!(detail_line(None, None, NOW), "");
    }

    #[test]
    fn revoking_removes_both_grants_but_keeps_others() {
        let dir = TempDir::new();
        let dirs = Dirs {
            config: dir.path.join("config"),
            state: dir.path.join("state"),
            cache: dir.path.join("cache"),
            data: dir.path.join("data"),
        };
        let layout = StoreLayout::new(dirs.store_root());
        layout.init().expect("store layout");
        let store =
            TokenStore::open(layout.tokens_dir()).expect("store");
        let tokens = tokens_expiring_at(NOW);
        store.save("work-imap", &tokens).expect("imap");
        store.save("work-graph", &tokens).expect("graph");
        store.save("personal-imap", &tokens).expect("other");

        revoke_grants(&dirs, "work").expect("revoke");
        assert!(store.load("work-imap").is_err());
        assert!(store.load("work-graph").is_err());
        assert!(store.load("personal-imap").is_ok());
    }

    #[test]
    fn revoking_without_a_store_reports_the_vault() {
        let dir = TempDir::new();
        let dirs = Dirs {
            config: dir.path.join("config"),
            state: dir.path.join("state"),
            cache: dir.path.join("cache"),
            data: dir.path.join("data"),
        };
        let error = revoke_grants(&dirs, "work").expect_err("no store");
        assert!(error.contains("vault"));
    }
}
