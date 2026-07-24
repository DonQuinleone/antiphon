use std::path::Path;
use std::process::Command;

pub fn notmuch_available_or_skip() -> bool {
    let available = Command::new("notmuch")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success());
    if available {
        return true;
    }
    eprintln!(
        "skipping: notmuch CLI not installed \
         (brew install notmuch / apt install notmuch)"
    );
    false
}

pub fn run_notmuch_new(config: &Path) {
    let out = Command::new("notmuch")
        .arg("new")
        .env("NOTMUCH_CONFIG", config)
        .output()
        .expect("failed to run notmuch new");
    assert!(
        out.status.success(),
        "notmuch new failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
