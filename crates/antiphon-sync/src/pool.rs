use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::engine::RemoteFolder;
use crate::error::SyncError;
use crate::indexer::{IndexNudge, Indexer};
use crate::report::{FolderReport, SyncReport};
use crate::state::{AccountState, FolderState};

/// The network side of folder syncing, kept behind a trait so
/// the pool's scheduling can be driven by a fake that never
/// opens a connection.
pub(crate) trait FolderSync: Sync {
    type Conn;

    /// A fresh worker connection, or `None` when it cannot be
    /// opened; the pool carries on with the workers it has.
    fn connect(&self) -> Option<Self::Conn>;

    fn sync_folder(
        &self,
        conn: &mut Self::Conn,
        folder: &RemoteFolder,
        stored: Option<FolderState>,
        nudge: &IndexNudge,
    ) -> Result<(FolderReport, FolderState), SyncError>;

    fn finish(&self, conn: Self::Conn);
}

/// One account's folders fetched by a bounded set of workers,
/// each holding its own IMAP connection. Worker zero reuses the
/// `control` connection that already listed the folders; the
/// rest connect fresh, so a large folder occupies one connection
/// while the others drain the queue. The per-folder state read
/// and save are pure logic and stay here where a fake can drive
/// them.
pub(crate) fn run<S: FolderSync>(
    control: S::Conn,
    limit: usize,
    jobs: Vec<RemoteFolder>,
    state: &Mutex<AccountState>,
    state_path: &Path,
    indexer: &Indexer,
    syncer: &S,
) -> Result<SyncReport, SyncError> {
    let next = AtomicUsize::new(0);
    let extra = extra_workers(limit, jobs.len());
    let context = Context {
        jobs: &jobs,
        next: &next,
        state,
        state_path,
        syncer,
    };
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..extra)
            .map(|_| {
                let nudge = indexer.nudge_channel();
                scope.spawn(|| run_spawned(&context, nudge))
            })
            .collect();
        let mut all = vec![run_worker(
            control,
            &context,
            indexer.nudge_channel(),
        )];
        for handle in handles {
            all.push(handle.join().unwrap_or_default());
        }
        all
    });
    merge(results)
}

/// Worker zero always exists; further workers help only when
/// folders outnumber the single connection, and never exceed
/// the bound.
fn extra_workers(limit: usize, folders: usize) -> usize {
    limit
        .max(1)
        .saturating_sub(1)
        .min(folders.saturating_sub(1))
}

struct Context<'a, S: FolderSync> {
    jobs: &'a [RemoteFolder],
    next: &'a AtomicUsize,
    state: &'a Mutex<AccountState>,
    state_path: &'a Path,
    syncer: &'a S,
}

#[derive(Default)]
struct WorkerResult {
    reports: Vec<FolderReport>,
    errors: Vec<String>,
    fatal: Option<SyncError>,
}

fn run_spawned<S: FolderSync>(
    context: &Context<'_, S>,
    nudge: IndexNudge,
) -> WorkerResult {
    let Some(conn) = context.syncer.connect() else {
        return WorkerResult::default();
    };
    run_worker(conn, context, nudge)
}

fn run_worker<S: FolderSync>(
    mut conn: S::Conn,
    context: &Context<'_, S>,
    nudge: IndexNudge,
) -> WorkerResult {
    let mut result = WorkerResult::default();
    while let Some(job) = next_job(context) {
        let stored = lock(context.state).folder(&job.name);
        match context.syncer.sync_folder(&mut conn, job, stored, &nudge)
        {
            Ok((report, folder_state)) => {
                if let Err(error) =
                    commit(context, &job.name, folder_state)
                {
                    result.fatal = Some(error);
                    break;
                }
                result.reports.push(report);
            }
            Err(error) => {
                result.errors.push(format!("{}: {error}", job.name));
            }
        }
    }
    context.syncer.finish(conn);
    result
}

fn next_job<'a, S: FolderSync>(
    context: &Context<'a, S>,
) -> Option<&'a RemoteFolder> {
    let index = context.next.fetch_add(1, Ordering::Relaxed);
    context.jobs.get(index)
}

/// The cursor advances only once its folder's state is durable,
/// so a save failure aborts the account rather than letting a
/// later pass trust a cursor the store never recorded.
fn commit<S: FolderSync>(
    context: &Context<'_, S>,
    folder: &str,
    folder_state: FolderState,
) -> Result<(), SyncError> {
    let mut state = lock(context.state);
    state.set_folder(folder, folder_state);
    state.save(context.state_path)
}

fn merge(results: Vec<WorkerResult>) -> Result<SyncReport, SyncError> {
    let mut report = SyncReport::default();
    let mut fatal = None;
    for result in results {
        report.folders.extend(result.reports);
        report.errors.extend(result.errors);
        fatal = fatal.or(result.fatal);
    }
    match fatal {
        Some(error) => Err(error),
        None => Ok(report),
    }
}

fn lock(
    state: &Mutex<AccountState>,
) -> std::sync::MutexGuard<'_, AccountState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use antiphon_store::StoreLayout;

    use super::*;

    fn folder(name: &str) -> RemoteFolder {
        RemoteFolder {
            name: name.to_owned(),
            delimiter: None,
        }
    }

    fn report(name: &str) -> FolderReport {
        FolderReport {
            folder: name.to_owned(),
            new_messages: 1,
            updated_messages: 0,
            removed_messages: 0,
            delivered: Vec::new(),
        }
    }

    fn folder_state() -> FolderState {
        FolderState {
            uid_validity: 1,
            last_uid: 1,
            last_sweep_unix: 0,
        }
    }

    fn state_path() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("acct.state");
        (dir, path)
    }

    /// A fake connection that never touches the network. It
    /// records only enough to prove which folders it handled.
    struct FakeConn {
        handled: usize,
    }

    /// The fake syncer counts connections opened and folders
    /// synced, and can be told to fail one folder by name.
    struct FakeSync {
        connections: AtomicUsize,
        synced: AtomicUsize,
        fail: Option<&'static str>,
    }

    impl FakeSync {
        fn new(fail: Option<&'static str>) -> FakeSync {
            FakeSync {
                connections: AtomicUsize::new(0),
                synced: AtomicUsize::new(0),
                fail,
            }
        }
    }

    impl FolderSync for FakeSync {
        type Conn = FakeConn;

        fn connect(&self) -> Option<FakeConn> {
            self.connections.fetch_add(1, Ordering::SeqCst);
            Some(FakeConn { handled: 0 })
        }

        fn sync_folder(
            &self,
            conn: &mut FakeConn,
            folder: &RemoteFolder,
            _stored: Option<FolderState>,
            _nudge: &IndexNudge,
        ) -> Result<(FolderReport, FolderState), SyncError> {
            if self.fail == Some(folder.name.as_str()) {
                return Err(SyncError::Folder {
                    folder: folder.name.clone(),
                    detail: "server said no".into(),
                });
            }
            conn.handled += 1;
            self.synced.fetch_add(1, Ordering::SeqCst);
            Ok((report(&folder.name), folder_state()))
        }

        fn finish(&self, _conn: FakeConn) {}
    }

    #[test]
    fn every_folder_is_synced_exactly_once_across_workers() {
        let (_dir, path) = state_path();
        let state = Mutex::new(AccountState::default());
        let layout = StoreLayout::new(std::path::PathBuf::from("/x"));
        let indexer = Indexer::start(&layout, "acct");
        let names = ["INBOX", "Sent", "Archive", "Spam", "Lists"];
        let jobs: Vec<RemoteFolder> =
            names.iter().map(|name| folder(name)).collect();
        let syncer = FakeSync::new(None);
        let outcome = run(
            FakeConn { handled: 0 },
            4,
            jobs,
            &state,
            &path,
            &indexer,
            &syncer,
        )
        .unwrap();
        indexer.finish().unwrap();
        assert_eq!(syncer.synced.load(Ordering::SeqCst), names.len());
        assert_eq!(outcome.folders.len(), names.len());
        assert_eq!(syncer.connections.load(Ordering::SeqCst), 3);
        let mut synced: Vec<String> = outcome
            .folders
            .iter()
            .map(|folder| folder.folder.clone())
            .collect();
        synced.sort();
        let mut expected: Vec<String> =
            names.iter().map(|name| name.to_string()).collect();
        expected.sort();
        assert_eq!(synced, expected);
    }

    #[test]
    fn one_worker_handles_everything_when_bounded_to_one() {
        let (_dir, path) = state_path();
        let state = Mutex::new(AccountState::default());
        let layout = StoreLayout::new(std::path::PathBuf::from("/x"));
        let indexer = Indexer::start(&layout, "acct");
        let jobs = vec![folder("INBOX"), folder("Sent")];
        let syncer = FakeSync::new(None);
        let outcome = run(
            FakeConn { handled: 0 },
            1,
            jobs,
            &state,
            &path,
            &indexer,
            &syncer,
        )
        .unwrap();
        indexer.finish().unwrap();
        assert_eq!(syncer.connections.load(Ordering::SeqCst), 0);
        assert_eq!(outcome.folders.len(), 2);
    }

    #[test]
    fn a_failing_folder_is_recorded_and_the_rest_still_sync() {
        let (_dir, path) = state_path();
        let state = Mutex::new(AccountState::default());
        let layout = StoreLayout::new(std::path::PathBuf::from("/x"));
        let indexer = Indexer::start(&layout, "acct");
        let jobs =
            vec![folder("INBOX"), folder("Broken"), folder("Sent")];
        let syncer = FakeSync::new(Some("Broken"));
        let outcome = run(
            FakeConn { handled: 0 },
            1,
            jobs,
            &state,
            &path,
            &indexer,
            &syncer,
        )
        .unwrap();
        indexer.finish().unwrap();
        assert_eq!(outcome.folders.len(), 2);
        assert_eq!(outcome.errors.len(), 1);
        assert!(outcome.errors[0].starts_with("Broken:"));
    }

    #[test]
    fn extra_workers_never_exceed_folders_or_the_bound() {
        assert_eq!(extra_workers(4, 5), 3);
        assert_eq!(extra_workers(4, 2), 1);
        assert_eq!(extra_workers(4, 1), 0);
        assert_eq!(extra_workers(4, 0), 0);
        assert_eq!(extra_workers(1, 9), 0);
        assert_eq!(extra_workers(0, 9), 0);
    }
}
