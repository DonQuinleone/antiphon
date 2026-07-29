//! The account form's connection test: `^t` reaches the
//! account's IMAP server on a worker thread and reports whether
//! it is reachable, authenticated, or refused, so a wrong host
//! or credential is caught at entry rather than surfacing later
//! as a silent daemon sync failure. The connect runs off the
//! event loop (like the OAuth sign-in) so keystrokes keep being
//! served, and it is bounded by a timeout so a stalled server
//! cannot hang the client.

use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

use antiphon_sync::{Auth, SyncAccount, SyncError};

use super::account_form::AccountFormState;
use super::account_form_fields::AccountType;
use super::account_form_save::oauth_hosts;
use super::app::App;

/// How long a single connect may take before it is reported as
/// unreachable; a stalled server can then never hang the client.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// The implicit IMAPS port the daemon assumes; the form has no
/// port field, so the test uses the same default.
const IMAPS_PORT: u16 = 993;

/// How the result reads, driving both the message and its
/// colour: still running, a success, or a failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tone {
    Working,
    Good,
    Bad,
}

/// The connection test attached to the open form: the last
/// message and its tone, plus the worker channel while a check
/// is still in flight.
#[derive(Debug)]
pub(super) struct ConnTest {
    pub(super) message: String,
    pub(super) tone: Tone,
    pending: Option<Receiver<ConnReport>>,
}

struct ConnReport {
    message: String,
    tone: Tone,
}

/// What the worker checks, resolved from the form on the event
/// loop (cheap) so the worker only does the blocking network and
/// subprocess work.
struct TestSpec {
    host: String,
    port: u16,
    user: String,
    kind: TestKind,
}

/// A password account authenticates with a resolved secret; an
/// OAuth account has no token here, so only reachability is
/// checked and the stored-grant state is reported alongside.
enum TestKind {
    Password { command: String },
    Oauth { signed_in: bool },
}

/// Starts a connection test for the open form, replacing any
/// previous result. A form that cannot yet be tested reports why
/// at once rather than spawning a worker.
pub(super) fn start(app: &mut App) {
    match spec_from_form(app) {
        Ok(spec) => spawn(app, spec),
        Err(message) => set_test(
            app,
            ConnTest {
                message,
                tone: Tone::Bad,
                pending: None,
            },
        ),
    }
}

/// Pumps the worker's result into the form; called once per
/// event-loop pass like the OAuth poll, so the message follows
/// the check while keys keep being served.
pub(super) fn poll(app: &mut App) {
    let Some(form) = app.account_form.as_mut() else {
        return;
    };
    let Some(test) = form.conn_test.as_mut() else {
        return;
    };
    let Some(pending) = test.pending.as_ref() else {
        return;
    };
    match pending.try_recv() {
        Ok(report) => {
            test.message = report.message;
            test.tone = report.tone;
            test.pending = None;
        }
        Err(TryRecvError::Empty) => {}
        Err(TryRecvError::Disconnected) => {
            test.message =
                "the connection test stopped unexpectedly".to_string();
            test.tone = Tone::Bad;
            test.pending = None;
        }
    }
}

fn spawn(app: &mut App, spec: TestSpec) {
    let host = spec.host.clone();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let report = run_test(
            &spec,
            run_command,
            |account| {
                map_probe(antiphon_sync::probe_login(
                    account,
                    CONNECT_TIMEOUT,
                ))
            },
            |host, port| {
                map_probe(antiphon_sync::probe_reachable(
                    host,
                    port,
                    CONNECT_TIMEOUT,
                ))
            },
        );
        let _ = tx.send(report);
    });
    set_test(
        app,
        ConnTest {
            message: format!("testing the connection to {host}..."),
            tone: Tone::Working,
            pending: Some(rx),
        },
    );
}

fn set_test(app: &mut App, test: ConnTest) {
    if let Some(form) = app.account_form.as_mut() {
        form.conn_test = Some(test);
    }
}

fn spec_from_form(app: &App) -> Result<TestSpec, String> {
    let form = app
        .account_form
        .as_ref()
        .ok_or_else(|| "no account form is open".to_string())?;
    match form.account_type {
        AccountType::Imap => imap_spec(form),
        _ => oauth_spec(app, form),
    }
}

fn imap_spec(form: &AccountFormState) -> Result<TestSpec, String> {
    let host = form.imap_host.trim();
    if host.is_empty() {
        return Err("give an imap host to test".to_string());
    }
    let user = form.imap_user.trim();
    if user.is_empty() {
        return Err("give an imap user to test".to_string());
    }
    Ok(TestSpec {
        host: host.to_string(),
        port: IMAPS_PORT,
        user: user.to_string(),
        kind: TestKind::Password {
            command: password_command(form)?,
        },
    })
}

fn oauth_spec(
    app: &App,
    form: &AccountFormState,
) -> Result<TestSpec, String> {
    let provider = form
        .provider()
        .ok_or_else(|| "choose an account type first".to_string())?;
    Ok(TestSpec {
        host: oauth_hosts(provider).imap.to_string(),
        port: IMAPS_PORT,
        user: form.address.trim().to_string(),
        kind: TestKind::Oauth {
            signed_in: grant_present(app, form),
        },
    })
}

/// The command whose output is the account's password: the
/// typed one in command mode, or the Keychain lookup in Keychain
/// mode (macOS), matching how the daemon reads it.
fn password_command(form: &AccountFormState) -> Result<String, String> {
    if cfg!(target_os = "macos") && form.keychain {
        let name = form.name.trim();
        if name.is_empty() {
            return Err(
                "name the account so its Keychain entry can be \
                 found"
                    .to_string(),
            );
        }
        return Ok(crate::setup::keychain_lookup_command(name));
    }
    let command = form.password_cmd.trim();
    if command.is_empty() {
        return Err("give a password command to test".to_string());
    }
    Ok(command.to_string())
}

fn grant_present(app: &App, form: &AccountFormState) -> bool {
    let name = form.name.trim();
    if name.is_empty() {
        return false;
    }
    let Some(store) =
        super::oauth_status::open_store_if_present(&app.dirs)
    else {
        return false;
    };
    store.load(&antiphon_oauth::imap_grant(name)).is_ok()
}

/// Why a probe did not authenticate: the server was out of
/// reach, or it answered but refused the credentials. Keeping
/// the decision on this small type (not `SyncError`) lets the
/// tests drive `run_test` without the imap-client stack.
enum ProbeError {
    Unreachable(String),
    Refused(String),
}

impl ProbeError {
    fn detail(&self) -> &str {
        match self {
            ProbeError::Unreachable(detail)
            | ProbeError::Refused(detail) => detail,
        }
    }
}

/// The seam the tests stub: `run_command` runs the password
/// command, `connect` authenticates, `reach` checks TLS
/// reachability. The live wiring passes the real subprocess and
/// probes; a test passes stubs, so the suite never spawns a
/// process or touches a socket.
fn run_test(
    spec: &TestSpec,
    run_command: impl Fn(&str) -> Result<String, String>,
    connect: impl Fn(&SyncAccount) -> Result<(), ProbeError>,
    reach: impl Fn(&str, u16) -> Result<(), ProbeError>,
) -> ConnReport {
    match &spec.kind {
        TestKind::Password { command } => {
            let password = match run_command(command) {
                Ok(password) => password,
                Err(message) => {
                    return ConnReport {
                        message,
                        tone: Tone::Bad,
                    };
                }
            };
            login_report(connect(&sync_account(spec, password)))
        }
        TestKind::Oauth { signed_in } => {
            reach_report(reach(&spec.host, spec.port), *signed_in)
        }
    }
}

fn sync_account(spec: &TestSpec, password: String) -> SyncAccount {
    SyncAccount {
        name: "connection-test".to_string(),
        host: spec.host.clone(),
        port: spec.port,
        user: spec.user.clone(),
        auth: Auth::Password(password),
        excluded_folders: Vec::new(),
    }
}

fn login_report(outcome: Result<(), ProbeError>) -> ConnReport {
    match outcome {
        Ok(()) => ConnReport {
            message: "reached the server and signed in".to_string(),
            tone: Tone::Good,
        },
        Err(ProbeError::Refused(detail)) => ConnReport {
            message: format!(
                "reached the server, but it refused the \
                 credentials: {detail}"
            ),
            tone: Tone::Bad,
        },
        Err(ProbeError::Unreachable(detail)) => ConnReport {
            message: format!("could not reach the server: {detail}"),
            tone: Tone::Bad,
        },
    }
}

fn reach_report(
    outcome: Result<(), ProbeError>,
    signed_in: bool,
) -> ConnReport {
    let Ok(()) = outcome else {
        return ConnReport {
            message: format!(
                "could not reach the server: {}",
                outcome.unwrap_err().detail()
            ),
            tone: Tone::Bad,
        };
    };
    let message = if signed_in {
        "reached the server; a sign-in grant is already stored"
    } else {
        "reached the server; sign in to finish"
    };
    ConnReport {
        message: message.to_string(),
        tone: Tone::Good,
    }
}

/// Maps a probe's `SyncError` onto the form's decision type: a
/// login rejection is `Refused` (surfacing the server text from
/// the source chain), everything else is `Unreachable`.
fn map_probe(outcome: Result<(), SyncError>) -> Result<(), ProbeError> {
    match outcome {
        Ok(()) => Ok(()),
        Err(SyncError::Login { source, .. }) => {
            Err(ProbeError::Refused(error_chain(source.as_ref())))
        }
        Err(other) => Err(ProbeError::Unreachable(other.to_string())),
    }
}

/// The full source chain, so the deepest server text reaches the
/// message: the imap-client errors carry it below a generic
/// top-level display that would otherwise hide it.
fn error_chain(error: &dyn std::error::Error) -> String {
    let mut parts = vec![error.to_string()];
    let mut source = error.source();
    while let Some(inner) = source {
        parts.push(inner.to_string());
        source = inner.source();
    }
    parts.join(": ")
}

fn run_command(command: &str) -> Result<String, String> {
    let output = std::process::Command::new("sh")
        .args(["-c", command])
        .output()
        .map_err(|error| {
            format!("running the password command: {error}")
        })?;
    if !output.status.success() {
        return Err("the password command failed".to_string());
    }
    let password = String::from_utf8_lossy(&output.stdout)
        .trim_end_matches(['\r', '\n'])
        .to_string();
    if password.is_empty() {
        return Err("the password command produced nothing".to_string());
    }
    Ok(password)
}

#[cfg(test)]
pub(super) fn test_result(message: &str, tone: Tone) -> ConnTest {
    ConnTest {
        message: message.to_string(),
        tone,
        pending: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn password_spec() -> TestSpec {
        TestSpec {
            host: "imap.example.com".to_string(),
            port: IMAPS_PORT,
            user: "quin@example.com".to_string(),
            kind: TestKind::Password {
                command: "print-secret".to_string(),
            },
        }
    }

    fn oauth_spec(signed_in: bool) -> TestSpec {
        TestSpec {
            host: "imap.gmail.com".to_string(),
            port: IMAPS_PORT,
            user: "quin@example.com".to_string(),
            kind: TestKind::Oauth { signed_in },
        }
    }

    fn never_run_command(_: &str) -> Result<String, String> {
        panic!("the password command must not run");
    }

    fn never_connect(_: &SyncAccount) -> Result<(), ProbeError> {
        panic!("the connect must not run");
    }

    fn never_reach(_: &str, _: u16) -> Result<(), ProbeError> {
        panic!("the reachability check must not run");
    }

    #[test]
    fn a_successful_login_reports_authenticated() {
        let report = run_test(
            &password_spec(),
            |_| Ok("secret".to_string()),
            |account| {
                assert_eq!(account.host, "imap.example.com");
                assert!(matches!(&account.auth, Auth::Password(p)
                    if p == "secret"));
                Ok(())
            },
            never_reach,
        );
        assert_eq!(report.tone, Tone::Good);
        assert!(report.message.contains("signed in"));
    }

    #[test]
    fn a_refused_login_reports_the_server_message() {
        let report = run_test(
            &password_spec(),
            |_| Ok("secret".to_string()),
            |_| {
                Err(ProbeError::Refused(
                    "AUTHENTICATIONFAILED".to_string(),
                ))
            },
            never_reach,
        );
        assert_eq!(report.tone, Tone::Bad);
        assert!(report.message.contains("refused the credentials"));
        assert!(report.message.contains("AUTHENTICATIONFAILED"));
    }

    #[test]
    fn an_unreachable_server_reports_the_error() {
        let report = run_test(
            &password_spec(),
            |_| Ok("secret".to_string()),
            |_| Err(ProbeError::Unreachable("timed out".to_string())),
            never_reach,
        );
        assert_eq!(report.tone, Tone::Bad);
        assert!(report.message.contains("could not reach"));
        assert!(report.message.contains("timed out"));
    }

    #[test]
    fn a_failed_password_command_never_connects() {
        let report = run_test(
            &password_spec(),
            |_| Err("no such command".to_string()),
            never_connect,
            never_reach,
        );
        assert_eq!(report.tone, Tone::Bad);
        assert_eq!(report.message, "no such command");
    }

    #[test]
    fn an_oauth_account_reports_reachable_and_sign_in_needed() {
        let report = run_test(
            &oauth_spec(false),
            never_run_command,
            never_connect,
            |host, port| {
                assert_eq!(host, "imap.gmail.com");
                assert_eq!(port, IMAPS_PORT);
                Ok(())
            },
        );
        assert_eq!(report.tone, Tone::Good);
        assert!(report.message.contains("sign in to finish"));
    }

    #[test]
    fn an_oauth_account_with_a_grant_says_so() {
        let report = run_test(
            &oauth_spec(true),
            never_run_command,
            never_connect,
            |_, _| Ok(()),
        );
        assert_eq!(report.tone, Tone::Good);
        assert!(report.message.contains("already stored"));
    }

    #[test]
    fn an_unreachable_oauth_server_reports_the_error() {
        let report = run_test(
            &oauth_spec(true),
            never_run_command,
            never_connect,
            |_, _| {
                Err(ProbeError::Unreachable(
                    "connection refused".to_string(),
                ))
            },
        );
        assert_eq!(report.tone, Tone::Bad);
        assert!(report.message.contains("could not reach"));
    }

    #[test]
    fn map_probe_flags_a_timeout_as_unreachable() {
        let mapped = map_probe(Err(SyncError::Timeout {
            host: "imap.example.com".to_string(),
            port: IMAPS_PORT,
        }));
        assert!(matches!(
            mapped,
            Err(ProbeError::Unreachable(detail))
                if detail.contains("timed out")
        ));
    }

    #[test]
    fn error_chain_joins_the_whole_source_chain() {
        let error = Layered {
            message: "cannot resolve IMAP task",
            source: Some(Box::new(Layered {
                message: "AUTHENTICATIONFAILED",
                source: None,
            })),
        };
        let joined = error_chain(&error);
        assert!(joined.contains("cannot resolve IMAP task"));
        assert!(joined.contains("AUTHENTICATIONFAILED"));
    }

    #[derive(Debug)]
    struct Layered {
        message: &'static str,
        source: Option<Box<Layered>>,
    }

    impl std::fmt::Display for Layered {
        fn fmt(
            &self,
            out: &mut std::fmt::Formatter<'_>,
        ) -> std::fmt::Result {
            out.write_str(self.message)
        }
    }

    impl std::error::Error for Layered {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.source
                .as_ref()
                .map(|inner| inner.as_ref() as &dyn std::error::Error)
        }
    }
}
