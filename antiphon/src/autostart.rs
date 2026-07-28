use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use antiphon_config::Dirs;
use antiphon_ipc::{IpcClient, Request, Response, socket_path};

/// How long a cold daemon may take to appear: it has to run
/// passphrase_cmd and mount the vault before it binds.
const STARTUP_WAIT: Duration = Duration::from_secs(20);
const STARTUP_POLL: Duration = Duration::from_millis(200);
/// A stopping daemon holds its listening socket open until the
/// vault seal finishes, so it reads reachable throughout the
/// unmount; this bounds how long we wait for it to disappear.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(30);
const STATUS_WAIT: Duration = Duration::from_secs(2);
/// The version buildinfo emits when there is no git tag to
/// describe; see antiphon-buildinfo.
const UNVERSIONED: &str = "unversioned";

/// Ensure antiphond is reachable, spawning it when absent.
/// Errors are advisory: the client works offline without a
/// daemon, so the caller only reports the message.
pub fn ensure_daemon(enabled: bool, dirs: &Dirs) -> Result<(), String> {
    if !enabled || reachable() {
        return Ok(());
    }
    spawn_daemon(dirs)?;
    wait_reachable()
}

/// Ensure the reachable daemon is the same build as this
/// client: a daemon left running from an older build talks a
/// subtly different IPC and applies ops with superseded logic,
/// so a version mismatch is restarted rather than trusted.
/// Returns a notice when a restart happened.
pub fn ensure_matching_daemon(
    enabled: bool,
    dirs: &Dirs,
) -> Result<Option<String>, String> {
    if !enabled {
        return Ok(None);
    }
    if !reachable() {
        spawn_daemon(dirs)?;
        wait_reachable()?;
        return Ok(None);
    }
    let ours = env!("ANTIPHON_VERSION");
    let Some(running) = daemon_version() else {
        return Ok(None);
    };
    // A build with no git version reports "unversioned"; two
    // of those tell us nothing, so never restart on them, or a
    // non-git build would bounce the daemon every launch.
    if running == ours || running == UNVERSIONED || ours == UNVERSIONED
    {
        return Ok(None);
    }
    stop_daemon()?;
    spawn_daemon(dirs)?;
    wait_reachable()?;
    Ok(Some(format!(
        "restarted antiphond: it was {running}, this client is \
         {ours}"
    )))
}

fn spawn_daemon(dirs: &Dirs) -> Result<(), String> {
    let binary = daemon_binary()
        .ok_or("antiphond is not installed alongside antiphon")?;
    let log = daemon_log(&dirs.state)
        .map_err(|error| format!("daemon log: {error}"))?;
    use std::os::unix::process::CommandExt;
    // Its own process group, or closing the terminal that
    // launched the client would take the daemon down with it.
    Command::new(&binary)
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            log.try_clone()
                .map_err(|error| format!("daemon log: {error}"))?,
        ))
        .stderr(Stdio::from(log))
        .process_group(0)
        .spawn()
        .map_err(|error| {
            format!("starting {}: {error}", binary.display())
        })?;
    Ok(())
}

fn daemon_version() -> Option<String> {
    let path = socket_path(|var| std::env::var_os(var));
    let mut client = IpcClient::connect(&path).ok()?;
    let _ = client.set_read_timeout(STATUS_WAIT);
    match client.request(&Request::Status).ok()? {
        Response::Status(status) => Some(status.version),
        _ => None,
    }
}

/// Asks the daemon to stop and waits until it is gone. The
/// listening socket stays up through the vault seal, so an
/// unreachable socket means the old daemon has sealed and
/// released the vault: only then is a fresh mount safe.
fn stop_daemon() -> Result<(), String> {
    let path = socket_path(|var| std::env::var_os(var));
    if let Ok(mut client) = IpcClient::connect(&path) {
        let _ = client.set_read_timeout(STATUS_WAIT);
        let _ = client.request(&Request::Shutdown);
    }
    let deadline = Instant::now() + SHUTDOWN_WAIT;
    while Instant::now() < deadline {
        if !reachable() {
            return Ok(());
        }
        std::thread::sleep(STARTUP_POLL);
    }
    Err("the running antiphond did not stop; restart it by hand"
        .to_string())
}

fn reachable() -> bool {
    let path = socket_path(|var| std::env::var_os(var));
    IpcClient::connect(&path).is_ok()
}

fn wait_reachable() -> Result<(), String> {
    let deadline = Instant::now() + STARTUP_WAIT;
    while Instant::now() < deadline {
        if reachable() {
            return Ok(());
        }
        std::thread::sleep(STARTUP_POLL);
    }
    Err(format!(
        "antiphond did not come up within {} seconds; check \
         `antiphon doctor` and the vault passphrase_cmd",
        STARTUP_WAIT.as_secs()
    ))
}

fn daemon_log(state: &Path) -> std::io::Result<std::fs::File> {
    std::fs::create_dir_all(state)?;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(state.join("antiphond.log"))
}

fn daemon_binary() -> Option<PathBuf> {
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("antiphond")))
        .filter(|path| path.is_file());
    if sibling.is_some() {
        return sibling;
    }
    on_path("antiphond")
}

fn on_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}
