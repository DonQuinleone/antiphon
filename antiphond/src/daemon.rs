use std::os::unix::net::UnixStream;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};

use antiphon_config::{Dirs, Loaded, load};
use antiphon_ipc::{IpcServer, VaultState, socket_path};
use antiphon_store::{OpLog, StoreLayout};
use antiphon_sync::{DeliveryRule, SmtpAccount, SyncAccount};

use crate::accounts::{
    OauthAccount, delivery_rules, oauth_accounts, smtp_accounts,
    sync_accounts,
};
use crate::vaultctl;

const ACCEPT_POLL: Duration = Duration::from_millis(200);
const CLIENT_READ_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct Daemon {
    pub(crate) layout: StoreLayout,
    pub(crate) log: OpLog,
    pub(crate) accounts: Vec<SyncAccount>,
    pub(crate) oauth: Vec<OauthAccount>,
    pub(crate) smtp: Vec<(String, SmtpAccount)>,
    pub(crate) rules: Vec<(String, Vec<DeliveryRule>)>,
    pub(crate) last_sync_unix: Option<u64>,
    pub(crate) notify: bool,
    pub(crate) vault: VaultState,
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
    let vault = match vaultctl::ensure_open(&loaded, &layout) {
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
    let accounts = sync_accounts(&loaded);
    let oauth = oauth_accounts(&loaded);
    let smtp = smtp_accounts(&loaded);
    let rules = delivery_rules(&loaded);
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
    let mut daemon = Daemon {
        layout,
        log,
        accounts,
        oauth,
        smtp,
        rules,
        last_sync_unix: None,
        notify: loaded.config.notifications.enabled,
        vault,
    };
    daemon.sync_pass(false);
    let interval = sync_interval(&loaded);
    let idle_lock = idle_lock_interval(&loaded, daemon.vault);
    let shutdown = install_shutdown();
    let outcome = serve_with_timer(
        &server,
        &mut daemon,
        &loaded,
        interval,
        idle_lock,
        &shutdown,
    );
    // Seal the vault on the way out so a graceful stop leaves
    // ciphertext at rest, not an open mount.
    if let Err(error) = vaultctl::lock(&loaded, &daemon.layout) {
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
    loaded: &Loaded,
    interval: Option<Duration>,
    idle_lock: Option<Duration>,
    shutdown: &Arc<AtomicBool>,
) -> std::io::Result<()> {
    server.set_nonblocking(true)?;
    let mut last_sync = Instant::now();
    let mut last_client = Instant::now();
    while !shutdown.load(Ordering::Relaxed) {
        match server.accept() {
            Ok(stream) => {
                wake(daemon, loaded);
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
        if daemon.vault != VaultState::Sealed
            && due(last_sync, interval)
        {
            daemon.sync_pass(true);
            last_sync = Instant::now();
        }
        if daemon.vault == VaultState::Open
            && due(last_client, idle_lock)
        {
            idle_seal(daemon, loaded);
        }
    }
    println!("shutting down; sealing the vault");
    Ok(())
}

/// Sync pauses while sealed: the store only exists inside the
/// mounted vault.
fn idle_seal(daemon: &mut Daemon, loaded: &Loaded) {
    match vaultctl::lock(loaded, &daemon.layout) {
        Ok(()) => {
            daemon.vault = VaultState::Sealed;
            println!("idle: vault sealed, sync paused");
        }
        Err(error) => eprintln!("idle lock: {error}"),
    }
}

fn wake(daemon: &mut Daemon, loaded: &Loaded) {
    if daemon.vault != VaultState::Sealed {
        return;
    }
    match vaultctl::ensure_open(loaded, &daemon.layout) {
        Ok(state) => {
            daemon.vault = state;
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
