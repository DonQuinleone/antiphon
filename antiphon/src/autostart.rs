use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use antiphon_config::Dirs;
use antiphon_ipc::{IpcClient, socket_path};

/// How long a cold daemon may take to appear: it has to run
/// passphrase_cmd and mount the vault before it binds.
const STARTUP_WAIT: Duration = Duration::from_secs(20);
const STARTUP_POLL: Duration = Duration::from_millis(200);

/// Ensure antiphond is reachable, spawning it when absent.
/// Errors are advisory: the client works offline without a
/// daemon, so the caller only reports the message.
pub fn ensure_daemon(enabled: bool, dirs: &Dirs) -> Result<(), String> {
    if !enabled || reachable() {
        return Ok(());
    }
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
    wait_reachable()
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
