use std::os::unix::net::UnixStream;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};

use antiphon_config::{Dirs, Loaded, load};
use antiphon_ipc::{IpcServer, Response, VaultState, socket_path};
use antiphon_store::{OpLog, StoreLayout};

use crate::accounts::AccountSet;
use crate::idle;
use crate::mailflow::{Mailflow, SharedAccounts};
use crate::vaultctl;
use crate::worker::{self, Job, JobQueue};

const ACCEPT_POLL: Duration = Duration::from_millis(200);
const CLIENT_READ_TIMEOUT: Duration = Duration::from_secs(5);

/// The one piece of state the serve loop and the worker both
/// mutate. Everything else is either owned by exactly one side,
/// cloned read-only into the worker, or swapped wholesale
/// through the account-set lock. Neither lock is ever taken
/// while holding the other, so no lock ordering can arise.
pub(crate) struct MailState {
    pub(crate) log: OpLog,
    pub(crate) last_sync_unix: Option<u64>,
    /// Accounts whose OAuth tokens failed beyond refresh this
    /// epoch; cleared per account the moment a sync gets a
    /// token again, and surfaced through Status.
    pub(crate) auth_failures: std::collections::BTreeSet<String>,
}

pub(crate) type SharedState = Arc<Mutex<MailState>>;

pub(crate) fn lock_state(
    state: &SharedState,
) -> MutexGuard<'_, MailState> {
    state
        .lock()
        .expect("a state holder panicked; cursors untrusted")
}

pub(crate) fn lock_set(
    set: &SharedAccounts,
) -> MutexGuard<'_, AccountSet> {
    set.lock()
        .expect("a set holder panicked; accounts untrusted")
}

pub(crate) struct Daemon {
    pub(crate) layout: StoreLayout,
    pub(crate) state: SharedState,
    pub(crate) jobs: JobQueue,
    pub(crate) vault: VaultState,
    pub(crate) shutdown: Arc<AtomicBool>,
    /// Set only by a Restart request: the successor daemon
    /// re-opens the still-mounted vault, so sealing here would
    /// unmount then force it to remount, the slow path we are
    /// avoiding. A signal or a plain Shutdown leaves this false
    /// and seals as normal.
    pub(crate) skip_seal: Arc<AtomicBool>,
    pub(crate) watchers: Option<idle::IdleWatchers>,
    pub(crate) dirs: Dirs,
    pub(crate) loaded: Loaded,
    pub(crate) set: SharedAccounts,
}

impl Daemon {
    fn idle_wanted(&self) -> bool {
        self.loaded.config.sync.idle
    }

    fn start_watchers(&mut self) {
        if !self.idle_wanted() || self.watchers.is_some() {
            return;
        }
        let set = lock_set(&self.set).clone();
        let mut specs: Vec<idle::WatchSpec> = set
            .accounts
            .into_iter()
            .map(idle::WatchSpec::Plain)
            .collect();
        specs.extend(set.oauth.into_iter().map(idle::WatchSpec::Oauth));
        if specs.is_empty() {
            return;
        }
        self.watchers =
            Some(idle::spawn(&self.layout, specs, &self.jobs));
    }

    fn stop_watchers(&mut self) {
        if let Some(watchers) = self.watchers.take() {
            watchers.stop();
        }
    }

    /// Re-reads every account and the global config off disk
    /// and swaps the worker's account set, so accounts added or
    /// edited while the daemon runs sync without a restart. The
    /// queued pass picks the new set up immediately.
    pub(crate) fn reload(&mut self) -> Response {
        let loaded = match load(&self.dirs) {
            Ok(loaded) => loaded,
            Err(error) => return Response::Error(error.to_string()),
        };
        let set = AccountSet::from_loaded(&loaded);
        let count = set.len();
        *lock_set(&self.set) = set;
        self.loaded = loaded;
        self.stop_watchers();
        self.start_watchers();
        self.jobs.request(Job::Pass { announce: false });
        println!("configuration reloaded: {count} accounts");
        Response::Ack
    }

    /// Hands the configuration back for the final vault seal
    /// and drops everything else, including this daemon's job
    /// queue handle.
    fn finish(self) -> Loaded {
        self.loaded
    }
}

pub fn run() -> ExitCode {
    let Some(dirs) = Dirs::from_process() else {
        eprintln!("cannot resolve the home directory");
        return ExitCode::FAILURE;
    };
    let layout = StoreLayout::new(dirs.store_root());
    let loaded = match load(&dirs) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    // Unlock before touching the store: behind a sealed vault
    // the store only exists once the vault is mounted.
    let vault =
        match vaultctl::ensure_open(&loaded, &layout, &dirs.state) {
            Ok(state) => state,
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        };
    if !layout.exists() {
        eprintln!(
            "no message store at {}; run \
             `antiphon doctor --init-store` to create it",
            layout.root().display()
        );
        return ExitCode::FAILURE;
    }
    let log = match OpLog::open(&layout) {
        Ok(log) => log,
        Err(error) => {
            eprintln!("oplog: {error}");
            return ExitCode::FAILURE;
        }
    };
    let path = socket_path(|var| std::env::var_os(var));
    let server = match IpcServer::bind(&path) {
        Ok(server) => server,
        Err(error) => {
            eprintln!("cannot bind {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
    };
    println!(
        "antiphond {} listening on {}",
        env!("ANTIPHON_VERSION"),
        path.display()
    );
    let state: SharedState = Arc::new(Mutex::new(MailState {
        log,
        last_sync_unix: None,
        auth_failures: Default::default(),
    }));
    let set: SharedAccounts =
        Arc::new(Mutex::new(AccountSet::from_loaded(&loaded)));
    let flow = Mailflow {
        layout: layout.clone(),
        set: set.clone(),
        state: state.clone(),
    };
    let (jobs, worker) = worker::spawn(flow);
    let shutdown = install_shutdown();
    let skip_seal = Arc::new(AtomicBool::new(false));
    let mut daemon = Daemon {
        layout: layout.clone(),
        state,
        jobs: jobs.clone(),
        vault,
        shutdown: shutdown.clone(),
        skip_seal: skip_seal.clone(),
        watchers: None,
        dirs,
        loaded,
        set,
    };
    jobs.request(Job::Pass { announce: false });
    daemon.start_watchers();
    let outcome = serve_with_timer(&server, &mut daemon, &shutdown);
    daemon.stop_watchers();
    // The worker must finish its pass before the seal below
    // unmounts the store it is writing to; closing the queue
    // stops it after the current batch.
    let loaded = daemon.finish();
    drop(jobs);
    if worker.join().is_err() {
        eprintln!("the worker thread panicked");
    }
    // Seal the vault on the way out so a graceful stop leaves
    // ciphertext at rest, not an open mount. A Restart skips
    // this: the successor daemon reuses the open mount, and
    // sealing here would force a slow unmount then remount.
    let sealed = if skip_seal.load(Ordering::Relaxed) {
        Ok(())
    } else {
        vaultctl::lock(&loaded, &layout)
    };
    if let Err(error) = sealed {
        eprintln!("{error}");
    }
    if let Err(error) = outcome {
        eprintln!("accept: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn install_shutdown() -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    for signal in [SIGHUP, SIGINT, SIGTERM] {
        let _ = signal_hook::flag::register(signal, flag.clone());
    }
    flag
}

fn sync_interval(loaded: &Loaded) -> Option<Duration> {
    minutes(loaded.config.sync.interval_minutes)
}

/// A plain store never seals, whatever the config says.
fn idle_lock_interval(
    loaded: &Loaded,
    vault: VaultState,
) -> Option<Duration> {
    if vault != VaultState::Open {
        return None;
    }
    minutes(loaded.config.vault.idle_lock_minutes)
}

fn minutes(count: u32) -> Option<Duration> {
    if count == 0 {
        return None;
    }
    Some(Duration::from_secs(u64::from(count) * 60))
}

fn due(last: Instant, interval: Option<Duration>) -> bool {
    interval.is_some_and(|interval| last.elapsed() >= interval)
}

fn serve_with_timer(
    server: &IpcServer,
    daemon: &mut Daemon,
    shutdown: &Arc<AtomicBool>,
) -> std::io::Result<()> {
    server.set_nonblocking(true)?;
    let mut last_sync = Instant::now();
    let mut last_client = Instant::now();
    while !shutdown.load(Ordering::Relaxed) {
        match server.accept() {
            Ok(stream) => {
                wake(daemon);
                last_client = Instant::now();
                serve_client(daemon, stream);
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock =>
            {
                std::thread::sleep(ACCEPT_POLL);
            }
            // A client that vanishes between connect and accept
            // surfaces here (macOS says EINVAL, others say
            // ECONNABORTED); no accept error may kill the
            // daemon, it serves until told to stop.
            Err(error) => {
                eprintln!("accept: {error}");
                std::thread::sleep(ACCEPT_POLL);
            }
        }
        // A busy worker holds both timers off: stacking a pass
        // on a running one only queues duplicates, and sealing
        // would unmount the store under the running pass.
        // Intervals are read fresh each turn so a reload's new
        // config takes effect without a restart.
        if daemon.vault != VaultState::Sealed
            && due(last_sync, sync_interval(&daemon.loaded))
            && daemon.jobs.idle()
        {
            daemon.jobs.request(Job::Pass { announce: true });
            last_sync = Instant::now();
        }
        if daemon.vault == VaultState::Open
            && due(
                last_client,
                idle_lock_interval(&daemon.loaded, daemon.vault),
            )
            && daemon.jobs.idle()
        {
            idle_seal(daemon);
        }
    }
    if daemon.skip_seal.load(Ordering::Relaxed) {
        println!("restarting; handing the open vault across");
    } else {
        println!("shutting down; sealing the vault");
    }
    Ok(())
}

/// Sync pauses while sealed: the store only exists inside the
/// mounted vault.
fn idle_seal(daemon: &mut Daemon) {
    daemon.stop_watchers();
    match vaultctl::lock(&daemon.loaded, &daemon.layout) {
        Ok(()) => {
            daemon.vault = VaultState::Sealed;
            println!("idle: vault sealed, sync paused");
        }
        Err(error) => eprintln!("idle lock: {error}"),
    }
}

fn wake(daemon: &mut Daemon) {
    if daemon.vault != VaultState::Sealed {
        return;
    }
    match vaultctl::ensure_open(
        &daemon.loaded,
        &daemon.layout,
        &daemon.dirs.state,
    ) {
        Ok(state) => {
            daemon.vault = state;
            daemon.start_watchers();
            println!("client connected: vault unlocked");
        }
        Err(error) => eprintln!("wake unlock: {error}"),
    }
}

/// A connection may die between accept and setup; a client
/// lost that early is dropped, never a reason to stop serving.
fn serve_client(daemon: &mut Daemon, stream: UnixStream) {
    let ready = stream.set_nonblocking(false).and_then(|()| {
        // A silent client must not starve the accept loop and
        // the sync timer behind it.
        stream.set_read_timeout(Some(CLIENT_READ_TIMEOUT))
    });
    if let Err(error) = ready {
        eprintln!("client setup: {error}");
        return;
    }
    daemon.serve_connection(stream);
}

#[cfg(test)]
mod timer_tests {
    use super::*;

    #[test]
    fn zero_interval_disables_the_timer() {
        assert!(!due(Instant::now(), None));
    }

    #[test]
    fn elapsed_interval_is_due() {
        let past =
            Instant::now() - Duration::from_secs(SIXTY_ONE_SECONDS);
        assert!(due(past, Some(Duration::from_secs(60))));
        assert!(!due(Instant::now(), Some(Duration::from_secs(60))));
    }

    const SIXTY_ONE_SECONDS: u64 = 61;
}
