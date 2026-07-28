use std::os::unix::net::UnixStream;

use antiphon_ipc::{
    DaemonStatus, Operation, Request, Response, read_frame, write_frame,
};
use antiphon_store::{
    OpKind, OpLog, SearchIndex, StoreLayout, apply_op,
};

use crate::daemon::{Daemon, lock_state};
use crate::worker::Job;

impl Daemon {
    pub(crate) fn serve_connection(&mut self, mut stream: UnixStream) {
        loop {
            let request: Request = match read_frame(&mut stream) {
                Ok(request) => request,
                Err(_) => return,
            };
            let response = self.respond(request);
            if write_frame(&mut stream, &response).is_err() {
                return;
            }
        }
    }

    /// SyncNow and DrainOutbox Ack as soon as the work is
    /// queued on the worker; only EnqueueOp still means done
    /// on Ack, because clients rely on applied-on-Ack.
    pub(crate) fn respond(&mut self, request: Request) -> Response {
        match request {
            Request::Ping => Response::Pong,
            Request::EnqueueOp(operation) => self.enqueue(operation),
            Request::Status => Response::Status(self.status()),
            Request::SyncNow => {
                self.jobs.request(Job::Pass { announce: true });
                Response::Ack
            }
            Request::Reload => self.reload(),
            Request::DrainOutbox => {
                self.jobs.request(Job::DrainOutbox);
                Response::Ack
            }
            Request::Unsubscribe { url } => {
                match crate::unsubscribe::validate(&url) {
                    Ok(()) => {
                        crate::unsubscribe::spawn_post(url);
                        Response::Ack
                    }
                    Err(error) => Response::Error(error),
                }
            }
            Request::Subscribe => Response::Error(
                "events arrive with the sync loop".to_string(),
            ),
        }
    }

    fn enqueue(&mut self, operation: Operation) -> Response {
        let kind = store_kind(operation.kind);
        let mut state = lock_state(&self.state);
        let op = match state.log.append(
            &operation.account,
            &operation.message_id,
            kind,
        ) {
            Ok(op) => op,
            Err(error) => {
                return Response::Error(error.to_string());
            }
        };
        let response = apply(&self.layout, &mut state.log, &op);
        drop(state);
        if response == Response::Ack {
            self.jobs.request(Job::DrainOps);
        }
        response
    }

    fn status(&self) -> DaemonStatus {
        let state = lock_state(&self.state);
        DaemonStatus {
            version: env!("ANTIPHON_VERSION").to_string(),
            vault: self.vault,
            last_sync_unix: state.last_sync_unix,
            pending_ops: state.log.unsynced().len() as u64,
            auth_failures: state
                .auth_failures
                .iter()
                .cloned()
                .collect(),
        }
    }
}

/// Runs while a pass may be indexing on the worker: apply_op's
/// `notmuch new` then waits for the indexer's write lock rather
/// than failing, since notmuch (0.23+, built_with.retry_lock)
/// blocks writers until the lock frees. Verified against
/// notmuch 0.40: concurrent new/tag runs queue up and all
/// succeed, so the wait here is one incremental index run.
fn apply(
    layout: &StoreLayout,
    log: &mut OpLog,
    op: &antiphon_store::Op,
) -> Response {
    let index = match SearchIndex::open(layout) {
        Ok(index) => index,
        Err(error) => {
            return Response::Error(error.to_string());
        }
    };
    if let Err(error) = apply_op(layout, &index, op) {
        return Response::Error(error.to_string());
    }
    if let Err(error) = log.mark_applied(op.id) {
        return Response::Error(error.to_string());
    }
    Response::Ack
}

fn store_kind(kind: antiphon_ipc::OpKind) -> OpKind {
    match kind {
        antiphon_ipc::OpKind::Flag { add, remove } => {
            OpKind::Flag { add, remove }
        }
        antiphon_ipc::OpKind::Move {
            to_folder,
            from_folder,
        } => OpKind::Move {
            to_folder,
            from_folder,
        },
        antiphon_ipc::OpKind::Delete => OpKind::Delete,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use antiphon_config::{Config, Dirs, Loaded};
    use antiphon_ipc::OpKind as WireKind;
    use antiphon_ipc::{OpId, VaultState};

    use crate::accounts::AccountSet;
    use crate::daemon::{MailState, lock_set};
    use crate::worker::{self, JobQueue, Plan};

    use super::*;

    const WAIT: Duration = Duration::from_secs(5);
    const LAST_SYNC: u64 = 11;

    #[test]
    fn wire_kinds_map_onto_store_kinds() {
        let flag = store_kind(WireKind::Flag {
            add: vec!["flagged".to_string()],
            remove: Vec::new(),
        });
        assert!(matches!(flag, OpKind::Flag { .. }));
        let moved = store_kind(WireKind::Move {
            to_folder: "archive".to_string(),
            from_folder: Some(String::new()),
        });
        assert!(matches!(
            moved,
            OpKind::Move { to_folder, from_folder }
                if to_folder == "archive"
                    && from_folder.as_deref() == Some("")
        ));
        assert!(matches!(store_kind(WireKind::Delete), OpKind::Delete));
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        daemon: Daemon,
        plans: Receiver<Plan>,
        release: Sender<()>,
        worker: std::thread::JoinHandle<()>,
    }

    /// A daemon over a real temp store whose worker blocks
    /// inside every plan until released: a sync pass that
    /// lasts exactly as long as the test needs it to.
    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let layout = StoreLayout::new(dir.path().join("store"));
        layout.init().unwrap();
        let log = OpLog::open(&layout).unwrap();
        let state = Arc::new(Mutex::new(MailState {
            log,
            last_sync_unix: Some(LAST_SYNC),
            auth_failures: Default::default(),
        }));
        let (plan_tx, plans) = channel();
        let (release, release_rx) = channel::<()>();
        let (jobs, worker) = worker::spawn_with(move |plan| {
            let _ = plan_tx.send(plan);
            let _ = release_rx.recv();
        });
        let dirs = Dirs {
            config: dir.path().join("config"),
            state: dir.path().join("state"),
            cache: dir.path().join("cache"),
            data: dir.path().join("data"),
        };
        let loaded = Loaded {
            config: Config::default(),
            accounts: Vec::new(),
        };
        let set =
            Arc::new(Mutex::new(AccountSet::from_loaded(&loaded)));
        let daemon = Daemon {
            layout,
            state,
            jobs,
            vault: VaultState::Absent,
            watchers: None,
            dirs,
            loaded,
            set,
        };
        Fixture {
            _dir: dir,
            daemon,
            plans,
            release,
            worker,
        }
    }

    /// Releases are buffered, so a stack of them unblocks
    /// whatever plans are still queued before the closed job
    /// queue lets the worker exit.
    fn finish(fixture: Fixture) {
        let Fixture {
            daemon,
            plans,
            release,
            worker,
            ..
        } = fixture;
        drop(daemon);
        drop(plans);
        for _ in 0..RELEASE_HEADROOM {
            let _ = release.send(());
        }
        worker.join().unwrap();
    }

    const RELEASE_HEADROOM: usize = 8;

    fn flag_read(id: &str) -> Request {
        Request::EnqueueOp(Operation {
            op_id: OpId(0),
            account: "work".to_string(),
            message_id: id.to_string(),
            kind: WireKind::Flag {
                add: Vec::new(),
                remove: vec!["unread".to_string()],
            },
        })
    }

    fn start_pass(fixture: &mut Fixture) -> Plan {
        fixture.daemon.jobs.request(Job::Pass { announce: false });
        let plan = fixture.plans.recv_timeout(WAIT).unwrap();
        assert!(plan.pass);
        plan
    }

    #[test]
    fn ipc_answers_while_a_pass_is_running() {
        let mut fixture = fixture();
        start_pass(&mut fixture);
        assert!(!fixture.daemon.jobs.idle());

        assert_eq!(
            fixture.daemon.respond(Request::Ping),
            Response::Pong
        );
        let Response::Status(status) =
            fixture.daemon.respond(Request::Status)
        else {
            panic!("status must answer mid-pass");
        };
        assert_eq!(status.last_sync_unix, Some(LAST_SYNC));
        assert_eq!(status.pending_ops, 0);
        assert!(!fixture.daemon.jobs.idle());
        finish(fixture);
    }

    #[test]
    fn enqueue_mid_pass_is_applied_on_ack() {
        let mut fixture = fixture();
        start_pass(&mut fixture);

        let response =
            fixture.daemon.respond(flag_read("<a@example.com>"));
        assert_eq!(response, Response::Ack);
        let state = lock_state(&fixture.daemon.state);
        assert!(state.log.unapplied().is_empty());
        assert_eq!(state.log.unsynced().len(), 1);
        drop(state);
        finish(fixture);
    }

    #[test]
    fn sync_now_acks_at_once_and_queues_a_pass() {
        let mut fixture = fixture();
        assert_eq!(
            fixture.daemon.respond(Request::SyncNow),
            Response::Ack
        );
        let plan = fixture.plans.recv_timeout(WAIT).unwrap();
        assert!(plan.pass && plan.announce);
        finish(fixture);
    }

    #[test]
    fn drain_outbox_acks_at_once_and_queues_a_drain() {
        let mut fixture = fixture();
        assert_eq!(
            fixture.daemon.respond(Request::DrainOutbox),
            Response::Ack
        );
        let plan = fixture.plans.recv_timeout(WAIT).unwrap();
        assert!(plan.outbox && !plan.pass);
        finish(fixture);
    }

    #[test]
    fn an_acked_op_requests_a_prompt_replay() {
        let mut fixture = fixture();
        assert_eq!(
            fixture.daemon.respond(flag_read("<b@example.com>")),
            Response::Ack
        );
        let plan = fixture.plans.recv_timeout(WAIT).unwrap();
        assert!(plan.ops && !plan.pass);
        finish(fixture);
    }

    fn worker_queue(fixture: &Fixture) -> JobQueue {
        fixture.daemon.jobs.clone()
    }

    #[test]
    fn reload_picks_up_an_account_added_after_startup() {
        let mut fixture = fixture();
        assert!(lock_set(&fixture.daemon.set).accounts.is_empty());
        let accounts = fixture.daemon.dirs.config.join("accounts");
        std::fs::create_dir_all(&accounts).unwrap();
        std::fs::write(
            accounts.join("late.toml"),
            "[account]\n\
             name = \"late\"\n\
             [imap]\n\
             host = \"imap.example.com\"\n\
             user = \"quin@example.com\"\n\
             password_cmd = \"echo hunter2\"\n",
        )
        .unwrap();
        assert_eq!(
            fixture.daemon.respond(Request::Reload),
            Response::Ack
        );
        let names: Vec<String> = lock_set(&fixture.daemon.set)
            .accounts
            .iter()
            .map(|account| account.name.clone())
            .collect();
        assert_eq!(names, ["late"]);
        let plan = fixture.plans.recv_timeout(WAIT).unwrap();
        assert!(plan.pass && !plan.announce);
        finish(fixture);
    }

    #[test]
    fn reload_reports_a_broken_config_and_keeps_the_old_set() {
        let mut fixture = fixture();
        let accounts = fixture.daemon.dirs.config.join("accounts");
        std::fs::create_dir_all(&accounts).unwrap();
        std::fs::write(accounts.join("bad.toml"), "not toml [")
            .unwrap();
        let Response::Error(_) =
            fixture.daemon.respond(Request::Reload)
        else {
            panic!("a broken config must surface as an error");
        };
        assert!(lock_set(&fixture.daemon.set).accounts.is_empty());
        finish(fixture);
    }

    #[test]
    fn requests_stack_into_one_pending_batch_mid_pass() {
        let mut fixture = fixture();
        start_pass(&mut fixture);
        let queue = worker_queue(&fixture);
        queue.request(Job::Pass { announce: true });
        queue.request(Job::Pass { announce: false });
        queue.request(Job::DrainOutbox);
        fixture.release.send(()).unwrap();
        let merged = fixture.plans.recv_timeout(WAIT).unwrap();
        assert!(merged.pass && merged.announce && merged.outbox);
        drop(queue);
        finish(fixture);
    }
}
