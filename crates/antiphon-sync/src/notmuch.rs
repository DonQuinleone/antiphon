use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::error::SyncError;

/// notmuch's Xapian database takes one exclusive writer at a
/// time: a second `notmuch new` (or tag, or index refresh)
/// racing the first fails to acquire the lock. Concurrent
/// account and folder syncs therefore funnel every notmuch
/// write through this process-global gate.
fn write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Held for the duration of one notmuch invocation and no
/// longer; callers never take it across a network round-trip. A
/// panic mid-write poisons the lock, so recover the guard rather
/// than wedge every later sync behind a poisoned mutex.
pub(crate) fn write_guard() -> MutexGuard<'static, ()> {
    write_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn run_notmuch_new(config: &Path) -> Result<(), SyncError> {
    let _guard = write_guard();
    let output = Command::new("notmuch")
        .arg("new")
        .env("NOTMUCH_CONFIG", config)
        .output()
        .map_err(|source| SyncError::NotmuchSpawn { source })?;
    if output.status.success() {
        return Ok(());
    }
    Err(SyncError::Notmuch {
        detail: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_write_gate_serialises_concurrent_holders() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let live = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    let _guard = write_guard();
                    let now = live.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    std::thread::yield_now();
                    live.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        assert_eq!(peak.load(Ordering::SeqCst), 1);
    }
}
