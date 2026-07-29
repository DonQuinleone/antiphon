//! The background sign-in worker: the driver spawns it on its
//! own thread, and it runs each wanted grant's flow (the PKCE
//! loopback first, Microsoft's device code as a fallback),
//! stores the tokens, and reports progress back through the
//! channel the driver polls.

use std::sync::mpsc::Sender;

use antiphon_oauth::{
    BrowserPrompt, Grant, OauthError, Provider, TokenSet, TokenStore,
    VerificationPrompt, device_code_flow, pkce_loopback_flow,
};

use super::oauthflow::FlowUpdate;
use crate::oauthgrants::{GrantSpec, open_store};

pub(super) fn worker(
    dirs: &antiphon_config::Dirs,
    account: &str,
    specs: &[GrantSpec],
    tx: &Sender<FlowUpdate>,
) {
    let store = match open_store(dirs) {
        Ok(store) => store,
        Err(error) => {
            let _ = tx.send(FlowUpdate::Done(Err(error)));
            return;
        }
    };
    run_flows(
        account,
        specs,
        &store,
        tx,
        pkce_loopback_flow,
        device_code_flow,
        launch_browser,
    );
}

/// Every wanted grant in turn: the PKCE loopback flow first
/// for either provider, falling back to Microsoft's
/// device-code flow when the loopback listener cannot bind.
/// A dead channel means the user cancelled, so the loop stops
/// before storing anything further.
fn run_flows(
    account: &str,
    specs: &[GrantSpec],
    store: &TokenStore,
    tx: &Sender<FlowUpdate>,
    pkce: impl Fn(
        &Grant,
        &dyn Fn(&BrowserPrompt),
    ) -> Result<TokenSet, OauthError>,
    device: impl Fn(
        &Grant,
        &dyn Fn(&VerificationPrompt),
    ) -> Result<TokenSet, OauthError>,
    launch: impl Fn(&str),
) {
    for spec in specs.iter().filter(|spec| spec.wanted) {
        let tokens =
            match obtain(account, spec, tx, &pkce, &device, &launch) {
                Ok(tokens) => tokens,
                Err(error) => {
                    let _ = tx.send(FlowUpdate::Done(Err(format!(
                        "{} grant: {error}",
                        spec.audience
                    ))));
                    return;
                }
            };
        let cancelled = tx
            .send(FlowUpdate::Status(format!(
                "storing the {} grant for {account}...",
                spec.audience
            )))
            .is_err();
        if cancelled {
            return;
        }
        if let Err(error) = store.save(&spec.grant_name, &tokens) {
            let _ = tx.send(FlowUpdate::Done(Err(error.to_string())));
            return;
        }
    }
    let _ = tx.send(FlowUpdate::Done(Ok(format!(
        "signed in: grants stored for {account}"
    ))));
}

fn obtain(
    account: &str,
    spec: &GrantSpec,
    tx: &Sender<FlowUpdate>,
    pkce: impl Fn(
        &Grant,
        &dyn Fn(&BrowserPrompt),
    ) -> Result<TokenSet, OauthError>,
    device: impl Fn(
        &Grant,
        &dyn Fn(&VerificationPrompt),
    ) -> Result<TokenSet, OauthError>,
    launch: impl Fn(&str),
) -> Result<TokenSet, OauthError> {
    let attempt = pkce(&spec.grant, &|prompt| {
        launch(&prompt.consent_url);
        let _ = tx.send(FlowUpdate::Status(format!(
            "waiting for the browser sign-in for {account} \
             ({})... esc cancels",
            spec.audience
        )));
    });
    match attempt {
        Err(OauthError::Loopback(_))
            if spec.grant.provider == Provider::Microsoft =>
        {
            device(&spec.grant, &|prompt| {
                let _ = tx.send(FlowUpdate::Status(format!(
                    "open {} and enter code {} for {account}... \
                     esc cancels",
                    prompt.verification_url, prompt.user_code
                )));
            })
        }
        other => other,
    }
}

#[cfg(target_os = "macos")]
const BROWSER_OPENER: &str = "open";
#[cfg(not(target_os = "macos"))]
const BROWSER_OPENER: &str = "xdg-open";

fn launch_browser(url: &str) {
    let _ = std::process::Command::new(BROWSER_OPENER)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::mpsc::channel;

    use antiphon_config::{Oauth, OauthProvider};
    use secrecy::SecretString;

    use super::super::testkit::TempDir;
    use super::*;
    use crate::oauthgrants::account_grants;

    fn token_set(provider: Provider) -> TokenSet {
        TokenSet {
            access_token: SecretString::from("at"),
            refresh_token: SecretString::from("rt"),
            expires_at_unix: 2_000_000_000,
            scope: "scope".to_string(),
            client_id: "app".to_string(),
            provider,
            tenant: None,
        }
    }

    fn google_specs() -> Vec<GrantSpec> {
        account_grants(
            "work",
            &Oauth {
                provider: OauthProvider::Google,
                client_id: Some("app".to_string()),
                tenant: None,
            },
            "app",
            None,
        )
    }

    fn microsoft_specs() -> Vec<GrantSpec> {
        account_grants(
            "work",
            &Oauth {
                provider: OauthProvider::Microsoft,
                client_id: Some("app".to_string()),
                tenant: None,
            },
            "app",
            None,
        )
    }

    fn no_device(
        _: &Grant,
        _: &dyn Fn(&VerificationPrompt),
    ) -> Result<TokenSet, OauthError> {
        panic!("the device flow must not run");
    }

    #[test]
    fn a_successful_pkce_flow_stores_the_grant() {
        let dir = TempDir::new();
        let store = TokenStore::open(&dir.path).expect("store");
        let (tx, rx) = channel();
        let opened: Mutex<Vec<String>> = Mutex::new(Vec::new());
        run_flows(
            "work",
            &google_specs(),
            &store,
            &tx,
            |grant, on_prompt| {
                on_prompt(&BrowserPrompt {
                    consent_url: "https://consent.example".into(),
                });
                Ok(token_set(grant.provider))
            },
            no_device,
            |url| opened.lock().unwrap().push(url.to_string()),
        );
        assert!(store.load("work-imap").is_ok());
        assert_eq!(
            opened.lock().unwrap().as_slice(),
            ["https://consent.example"]
        );
        let updates: Vec<FlowUpdate> = rx.try_iter().collect();
        let Some(FlowUpdate::Done(Ok(notice))) = updates.last() else {
            panic!("expected a successful Done update");
        };
        assert!(notice.contains("work"));
        assert!(updates.iter().any(|update| matches!(
            update,
            FlowUpdate::Status(text) if text.contains("browser")
        )));
    }

    #[test]
    fn microsoft_falls_back_to_the_device_code_flow() {
        let dir = TempDir::new();
        let store = TokenStore::open(&dir.path).expect("store");
        let (tx, rx) = channel();
        run_flows(
            "work",
            &microsoft_specs(),
            &store,
            &tx,
            |_, _| Err(OauthError::Loopback("cannot bind".to_string())),
            |grant, on_prompt| {
                on_prompt(&VerificationPrompt {
                    verification_url: "https://device.example"
                        .to_string(),
                    user_code: "ABCD-1234".to_string(),
                });
                Ok(token_set(grant.provider))
            },
            |_| {},
        );
        assert!(store.load("work-imap").is_ok());
        let updates: Vec<FlowUpdate> = rx.try_iter().collect();
        assert!(matches!(
            updates.last(),
            Some(FlowUpdate::Done(Ok(_)))
        ));
        assert!(updates.iter().any(|update| matches!(
            update,
            FlowUpdate::Status(text) if text.contains("ABCD-1234")
        )));
    }

    #[test]
    fn a_declined_sign_in_reports_the_failure() {
        let dir = TempDir::new();
        let store = TokenStore::open(&dir.path).expect("store");
        let (tx, rx) = channel();
        run_flows(
            "work",
            &google_specs(),
            &store,
            &tx,
            |_, _| Err(OauthError::Declined("denied".to_string())),
            no_device,
            |_| {},
        );
        assert!(store.load("work-imap").is_err());
        let updates: Vec<FlowUpdate> = rx.try_iter().collect();
        assert!(matches!(
            updates.last(),
            Some(FlowUpdate::Done(Err(_)))
        ));
    }

    #[test]
    fn a_cancelled_flow_stores_nothing() {
        let dir = TempDir::new();
        let store = TokenStore::open(&dir.path).expect("store");
        let (tx, rx) = channel();
        drop(rx);
        run_flows(
            "work",
            &google_specs(),
            &store,
            &tx,
            |grant, _| Ok(token_set(grant.provider)),
            no_device,
            |_| {},
        );
        assert!(
            store.load("work-imap").is_err(),
            "no grant lands after a cancel"
        );
    }
}
