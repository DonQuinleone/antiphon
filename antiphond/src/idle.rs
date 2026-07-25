use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use antiphon_store::StoreLayout;
use antiphon_sync::{IdleSession, IdleWait, SyncAccount};

use crate::accounts::OauthAccount;
use crate::tokens::imap_access_token;
use crate::worker::{Job, JobQueue};

/// One wait window; shutdown is observed between windows, so
/// this bounds how long a stop can take.
const WATCH_WINDOW: Duration = Duration::from_secs(5);
/// RFC 2177 wants IDLE re-issued at least every 30 minutes;
/// re-issuing at 29 leaves a clear margin.
const REISSUE_AFTER: Duration = Duration::from_secs(29 * 60);
const BACKOFF_FLOOR: Duration = Duration::from_secs(5);
const BACKOFF_CEILING: Duration = Duration::from_secs(5 * 60);
const STOP_POLL: Duration = Duration::from_millis(250);

/// What one watcher connects as: a password account carries
/// its credentials; an OAuth account resolves a fresh access
/// token at every connect, since tokens outlive no reconnect.
pub(crate) enum WatchSpec {
    Plain(SyncAccount),
    Oauth(OauthAccount),
}

impl WatchSpec {
    fn name(&self) -> &str {
        match self {
            WatchSpec::Plain(account) => &account.name,
            WatchSpec::Oauth(spec) => &spec.name,
        }
    }

    fn account(
        &self,
        layout: &StoreLayout,
    ) -> Result<SyncAccount, String> {
        match self {
            WatchSpec::Plain(account) => Ok(account.clone()),
            WatchSpec::Oauth(spec) => {
                imap_access_token(layout, spec, false)
                    .map(|token| spec.sync_account(token))
            }
        }
    }
}

/// The INBOX watchers of one daemon epoch: sealed away with
/// the vault, respawned on wake.
pub(crate) struct IdleWatchers {
    stop: Arc<AtomicBool>,
    handles: Vec<JoinHandle<()>>,
}

pub(crate) fn spawn(
    layout: &StoreLayout,
    specs: Vec<WatchSpec>,
    jobs: &JobQueue,
) -> IdleWatchers {
    let stop = Arc::new(AtomicBool::new(false));
    let handles = specs
        .into_iter()
        .map(|spec| {
            let layout = layout.clone();
            let jobs = jobs.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                watch(&layout, &spec, &jobs, &stop);
            })
        })
        .collect();
    IdleWatchers { stop, handles }
}

impl IdleWatchers {
    pub(crate) fn stop(self) {
        self.stop.store(true, Ordering::SeqCst);
        for handle in self.handles {
            let _ = handle.join();
        }
    }
}

fn watch(
    layout: &StoreLayout,
    spec: &WatchSpec,
    jobs: &JobQueue,
    stop: &AtomicBool,
) {
    let mut backoff = BACKOFF_FLOOR;
    while !stop.load(Ordering::SeqCst) {
        match connect(layout, spec) {
            Ok(Some(session)) => {
                backoff = BACKOFF_FLOOR;
                run_session(session, spec.name(), jobs, stop);
            }
            Ok(None) => return,
            Err(error) => eprintln!("idle {}: {error}", spec.name()),
        }
        sleep_observing(stop, backoff);
        backoff = next_backoff(backoff);
    }
}

/// `Ok(None)` means the server lacks IDLE: said once, then the
/// account is left to the interval timer alone.
fn connect(
    layout: &StoreLayout,
    spec: &WatchSpec,
) -> Result<Option<IdleSession>, String> {
    let account = spec.account(layout)?;
    let session = IdleSession::connect(&account)
        .map_err(|error| error.to_string())?;
    if !session.supports_idle() {
        eprintln!(
            "idle {}: server lacks IDLE; interval sync only",
            spec.name()
        );
        session.close();
        return Ok(None);
    }
    Ok(Some(session))
}

fn run_session(
    mut session: IdleSession,
    name: &str,
    jobs: &JobQueue,
    stop: &AtomicBool,
) {
    let mut issued = Instant::now();
    loop {
        if stop.load(Ordering::SeqCst) {
            session.close();
            return;
        }
        if issued.elapsed() >= REISSUE_AFTER {
            if let Err(error) = session.refresh() {
                eprintln!("idle {name}: {error}");
                return;
            }
            issued = Instant::now();
        }
        match session.wait(WATCH_WINDOW) {
            Ok(IdleWait::Quiet) => {}
            Ok(IdleWait::Update) => {
                jobs.request(Job::Pass { announce: false });
                issued = Instant::now();
            }
            Err(error) => {
                eprintln!("idle {name}: {error}");
                return;
            }
        }
    }
}

fn next_backoff(current: Duration) -> Duration {
    (current * 2).min(BACKOFF_CEILING)
}

fn sleep_observing(stop: &AtomicBool, duration: Duration) {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        std::thread::sleep(
            STOP_POLL.min(
                deadline.saturating_duration_since(Instant::now()),
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_and_caps() {
        let mut delay = BACKOFF_FLOOR;
        let mut seen = Vec::new();
        for _ in 0..8 {
            seen.push(delay.as_secs());
            delay = next_backoff(delay);
        }
        assert_eq!(seen, [5, 10, 20, 40, 80, 160, 300, 300]);
    }

    #[test]
    fn stopped_sleep_returns_early() {
        let stop = AtomicBool::new(true);
        let before = Instant::now();
        sleep_observing(&stop, Duration::from_secs(10));
        assert!(before.elapsed() < Duration::from_secs(1));
    }
}
