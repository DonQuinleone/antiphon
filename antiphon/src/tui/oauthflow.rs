//! The in-client OAuth sign-in: `o` on a settings account row
//! runs the provider flow on a background thread, keeps the
//! status line talking while the browser is open, and stores
//! the same grant set `antiphon oauth login` would.

use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use antiphon_oauth::{
    BrowserPrompt, Grant, OauthError, Provider, TokenSet, TokenStore,
    VerificationPrompt, device_code_flow, pkce_loopback_flow,
};

use super::app::App;
use crate::oauthgrants::{
    GrantSpec, account_grants, open_store, resolve_client_id,
};

pub(super) enum FlowUpdate {
    Status(String),
    Done(Result<String, String>),
}

/// A sign-in in progress: the worker thread reports through
/// `updates`, and dropping the handle (esc) cancels by cutting
/// the channel, which the worker notices before storing
/// anything.
pub(super) struct OauthFlow {
    pub(super) account: String,
    pub(super) status: String,
    updates: Receiver<FlowUpdate>,
}

/// Starts the sign-in for the selected settings account; every
/// precondition failure lands in the notice instead.
pub(super) fn authorise(app: &mut App, file_stem: &str) {
    if let Some(flow) = &app.oauth_flow {
        app.notice = Some(format!(
            "a sign-in for {} is already running",
            flow.account
        ));
        return;
    }
    match flow_for(app, file_stem) {
        Ok(flow) => app.oauth_flow = Some(flow),
        Err(error) => app.notice = Some(error),
    }
}

fn flow_for(app: &App, file_stem: &str) -> Result<OauthFlow, String> {
    let loaded = antiphon_config::load(&app.dirs)
        .map_err(|error| error.to_string())?;
    let entry = loaded
        .accounts
        .iter()
        .find(|entry| entry.file_stem == file_stem)
        .ok_or(format!("no account named {file_stem}"))?;
    let account = entry.account.account.name.clone();
    let oauth = entry.account.oauth.as_ref().ok_or(format!(
        "{file_stem} has no oauth provider; e edits the account"
    ))?;
    let client_id = resolve_client_id(oauth, &account, |var| {
        std::env::var(var).ok()
    })?;
    let specs = account_grants(
        &account,
        oauth,
        &client_id,
        entry.account.graph.as_ref(),
    );
    let (tx, updates) = channel();
    let dirs = app.dirs.clone();
    let worker_account = account.clone();
    std::thread::spawn(move || {
        worker(&dirs, &worker_account, &specs, &tx)
    });
    Ok(OauthFlow {
        status: format!("starting the sign-in for {account}..."),
        account,
        updates,
    })
}

fn worker(
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

/// Pumps worker updates into the UI state; called once per
/// event-loop pass, so the status line follows the flow while
/// keys keep being served.
pub(super) fn poll(app: &mut App) {
    loop {
        let Some(flow) = app.oauth_flow.as_mut() else {
            return;
        };
        match flow.updates.try_recv() {
            Ok(FlowUpdate::Status(text)) => flow.status = text,
            Ok(FlowUpdate::Done(result)) => {
                finish(app, result);
                return;
            }
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                app.oauth_flow = None;
                return;
            }
        }
    }
}

fn finish(app: &mut App, result: Result<String, String>) {
    let Some(flow) = app.oauth_flow.take() else {
        return;
    };
    app.notice = Some(match result {
        Ok(notice) => match super::request_reload() {
            None => notice,
            Some(warning) => format!("{notice} ({warning})"),
        },
        Err(error) => {
            format!("sign-in for {} failed: {error}", flow.account)
        }
    });
    super::oauth_status::refresh_auth_failures(app);
    app.refresh_settings_accounts();
}

pub(super) fn cancel(app: &mut App) {
    let Some(flow) = app.oauth_flow.take() else {
        return;
    };
    app.notice =
        Some(format!("sign-in for {} cancelled", flow.account));
}

#[cfg(test)]
pub(super) fn test_flow(account: &str) -> OauthFlow {
    OauthFlow {
        account: account.to_string(),
        status: format!("waiting for {account}..."),
        updates: channel().1,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use antiphon_config::{Oauth, OauthProvider};
    use secrecy::SecretString;

    use super::super::testkit::TempDir;
    use super::*;

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

    #[test]
    fn poll_moves_worker_updates_into_the_ui() {
        let mut app = super::super::testkit::app_with_messages(1);
        let (tx, rx) = channel();
        app.oauth_flow = Some(OauthFlow {
            account: "work".to_string(),
            status: "starting...".to_string(),
            updates: rx,
        });
        tx.send(FlowUpdate::Status("waiting...".to_string()))
            .unwrap();
        poll(&mut app);
        assert_eq!(
            app.oauth_flow.as_ref().unwrap().status,
            "waiting..."
        );
        tx.send(FlowUpdate::Done(Err("declined".to_string())))
            .unwrap();
        poll(&mut app);
        assert!(app.oauth_flow.is_none());
        assert!(
            app.notice.as_deref().unwrap().contains("declined"),
            "{:?}",
            app.notice
        );
    }

    #[test]
    fn cancelling_drops_the_flow_and_says_so() {
        let mut app = super::super::testkit::app_with_messages(1);
        app.oauth_flow = Some(test_flow("work"));
        cancel(&mut app);
        assert!(app.oauth_flow.is_none());
        assert!(app.notice.as_deref().unwrap().contains("cancelled"));
    }

    #[test]
    fn authorising_without_an_oauth_table_is_a_notice() {
        let dir = TempDir::new();
        let accounts = dir.path.join("accounts");
        std::fs::create_dir_all(&accounts).unwrap();
        std::fs::write(
            accounts.join("work.toml"),
            "[account]\nname = \"work\"\n\n\
             [imap]\nhost = \"h\"\nuser = \"u\"\n",
        )
        .unwrap();
        let mut app = super::super::testkit::app_with_messages(1);
        app.dirs.config = dir.path.clone();
        authorise(&mut app, "work");
        assert!(app.oauth_flow.is_none());
        assert!(
            app.notice.as_deref().unwrap().contains("oauth"),
            "{:?}",
            app.notice
        );
    }
}
