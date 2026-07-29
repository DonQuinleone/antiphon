//! The in-client OAuth sign-in: `o` on a settings account row
//! runs the provider flow on a background thread, keeps the
//! status line talking while the browser is open, and stores
//! the same grant set `antiphon oauth login` would.

use std::sync::mpsc::{Receiver, TryRecvError, channel};

use super::app::App;
use super::oauthflow_worker::worker;
use crate::oauthgrants::{account_grants, resolve_client_id};

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
    let succeeded = result.is_ok();
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
    // A completed sign-in outranks the daemon's last report: drop
    // the stale failure so the row reads authorised at once rather
    // than staying on "needs sign-in" until the next status poll.
    if succeeded {
        clear_auth_failure(&mut app.auth_failures, &flow.account);
    }
    app.refresh_settings_accounts();
}

fn clear_auth_failure(failures: &mut Vec<String>, account: &str) {
    failures.retain(|name| name != account);
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
    use super::super::testkit::TempDir;
    use super::*;

    #[test]
    fn a_successful_sign_in_drops_the_stale_failure() {
        let mut failures =
            vec!["work".to_string(), "other".to_string()];
        clear_auth_failure(&mut failures, "work");
        assert_eq!(
            failures,
            vec!["other".to_string()],
            "the signed-in account is no longer flagged failed",
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
