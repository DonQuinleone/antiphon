use std::os::unix::net::UnixStream;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};

use antiphon_config::{Dirs, Loaded, load};
use antiphon_ipc::{IpcServer, socket_path};
use antiphon_store::{OpLog, StoreLayout};
use antiphon_sync::{DeliveryRule, SmtpAccount, SyncAccount};

use crate::accounts::{delivery_rules, smtp_accounts, sync_accounts};
use crate::vaultctl;

const ACCEPT_POLL: Duration = Duration::from_millis(200);
const CLIENT_READ_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct Daemon {
    pub(crate) layout: StoreLayout,
    pub(crate) log: OpLog,
    pub(crate) accounts: Vec<SyncAccount>,
    pub(crate) smtp: Vec<(String, SmtpAccount)>,
    pub(crate) rules: Vec<(String, Vec<DeliveryRule>)>,
    pub(crate) last_sync_unix: Option<u64>,
    pub(crate) notify: bool,
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
    if let Err(error) = vaultctl::ensure_open(&loaded, &layout) {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }
    if !layout.exists() {
        eprintln!(
            "no message store at {}; run \
             `antiphon doctor --init-store` to create it",
            layout.root().display()
        );
        return ExitCode::FAILURE;
    }
    let accounts = sync_accounts(&loaded);
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
        smtp,
        rules,
        last_sync_unix: None,
        notify: loaded.config.notifications.enabled,
    };
    daemon.sync_pass(false);
    let interval = sync_interval(&loaded);
    let shutdown = install_shutdown();
    let outcome =
        serve_with_timer(&server, &mut daemon, interval, &shutdown);
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
    let minutes = loaded.config.sync.interval_minutes;
    if minutes == 0 {
        return None;
    }
    Some(Duration::from_secs(u64::from(minutes) * 60))
}

fn due(last: Instant, interval: Option<Duration>) -> bool {
    interval.is_some_and(|interval| last.elapsed() >= interval)
}

fn serve_with_timer(
    server: &IpcServer,
    daemon: &mut Daemon,
    interval: Option<Duration>,
    shutdown: &Arc<AtomicBool>,
) -> std::io::Result<()> {
    server.set_nonblocking(true)?;
    let mut last_sync = Instant::now();
    while !shutdown.load(Ordering::Relaxed) {
        match server.accept() {
            Ok(stream) => serve_client(daemon, stream),
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
        if due(last_sync, interval) {
            daemon.sync_pass(true);
            last_sync = Instant::now();
        }
    }
    println!("shutting down; sealing the vault");
    Ok(())
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
