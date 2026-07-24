use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use crate::mailflow::Mailflow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Job {
    Pass { announce: bool },
    DrainOutbox,
    DrainOps,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Plan {
    pub(crate) pass: bool,
    pub(crate) announce: bool,
    pub(crate) outbox: bool,
    pub(crate) ops: bool,
}

impl Plan {
    fn merge(jobs: &[Job]) -> Plan {
        let mut plan = Plan::default();
        for job in jobs {
            match job {
                Job::Pass { announce } => {
                    plan.pass = true;
                    plan.announce |= announce;
                }
                Job::DrainOutbox => plan.outbox = true,
                Job::DrainOps => plan.ops = true,
            }
        }
        plan
    }
}

#[derive(Clone)]
pub(crate) struct JobQueue {
    sender: Sender<Job>,
    outstanding: Arc<AtomicUsize>,
}

impl JobQueue {
    pub(crate) fn request(&self, job: Job) {
        self.outstanding.fetch_add(1, Ordering::SeqCst);
        if self.sender.send(job).is_err() {
            self.outstanding.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// True only when nothing is queued and nothing is running;
    /// the serve loop's idle seal and sync timer gate on this.
    pub(crate) fn idle(&self) -> bool {
        self.outstanding.load(Ordering::SeqCst) == 0
    }
}

pub(crate) fn spawn(flow: Mailflow) -> (JobQueue, JoinHandle<()>) {
    spawn_with(move |plan| execute(&flow, plan))
}

pub(crate) fn spawn_with(
    mut run: impl FnMut(Plan) + Send + 'static,
) -> (JobQueue, JoinHandle<()>) {
    let (sender, receiver) = channel();
    let outstanding = Arc::new(AtomicUsize::new(0));
    let counter = outstanding.clone();
    let handle = std::thread::spawn(move || {
        serve_jobs(&receiver, &counter, &mut run);
    });
    (
        JobQueue {
            sender,
            outstanding,
        },
        handle,
    )
}

/// Jobs queued while one runs collapse into a single merged
/// plan, so any number of requests leaves at most one pending
/// execution. The counter drops only after the batch has run:
/// `idle` must never report idle mid-execution.
fn serve_jobs(
    jobs: &Receiver<Job>,
    outstanding: &AtomicUsize,
    run: &mut impl FnMut(Plan),
) {
    while let Ok(first) = jobs.recv() {
        let mut batch = vec![first];
        while let Ok(job) = jobs.try_recv() {
            batch.push(job);
        }
        run(Plan::merge(&batch));
        outstanding.fetch_sub(batch.len(), Ordering::SeqCst);
    }
}

fn execute(flow: &Mailflow, plan: Plan) {
    if plan.pass {
        flow.sync_pass(plan.announce);
        return;
    }
    if plan.outbox {
        flow.drain_outbox();
    }
    if plan.ops {
        flow.drain_ops();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    const WAIT: Duration = Duration::from_secs(5);

    struct MergeCase {
        name: &'static str,
        jobs: &'static [Job],
        plan: Plan,
    }

    const MERGE_CASES: [MergeCase; 4] = [
        MergeCase {
            name: "passes collapse and announce sticks",
            jobs: &[
                Job::Pass { announce: false },
                Job::Pass { announce: true },
                Job::Pass { announce: false },
            ],
            plan: Plan {
                pass: true,
                announce: true,
                outbox: false,
                ops: false,
            },
        },
        MergeCase {
            name: "drains merge without a pass",
            jobs: &[Job::DrainOutbox, Job::DrainOps],
            plan: Plan {
                pass: false,
                announce: false,
                outbox: true,
                ops: true,
            },
        },
        MergeCase {
            name: "a pass folds queued drains in",
            jobs: &[
                Job::DrainOutbox,
                Job::Pass { announce: false },
                Job::DrainOps,
            ],
            plan: Plan {
                pass: true,
                announce: false,
                outbox: true,
                ops: true,
            },
        },
        MergeCase {
            name: "no jobs plan nothing",
            jobs: &[],
            plan: Plan {
                pass: false,
                announce: false,
                outbox: false,
                ops: false,
            },
        },
    ];

    #[test]
    fn merged_plans_follow_the_table() {
        for case in MERGE_CASES {
            assert_eq!(
                Plan::merge(case.jobs),
                case.plan,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn requests_during_a_run_coalesce_into_one_pending_plan() {
        let (plan_tx, plan_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let (queue, worker) = spawn_with(move |plan| {
            plan_tx.send(plan).unwrap();
            release_rx.recv().unwrap();
        });
        queue.request(Job::Pass { announce: false });
        let first = plan_rx.recv_timeout(WAIT).unwrap();
        assert!(first.pass);
        queue.request(Job::Pass { announce: true });
        queue.request(Job::Pass { announce: false });
        queue.request(Job::DrainOps);
        release_tx.send(()).unwrap();
        let second = plan_rx.recv_timeout(WAIT).unwrap();
        assert!(second.pass && second.announce && second.ops);
        release_tx.send(()).unwrap();
        assert!(plan_rx.recv_timeout(WAIT).is_err());
        drop(queue);
        worker.join().unwrap();
    }

    #[test]
    fn the_queue_reads_busy_until_the_batch_has_run() {
        let (busy_tx, busy_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let (queue, worker) = spawn_with(move |_| {
            busy_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        assert!(queue.idle());
        queue.request(Job::DrainOutbox);
        busy_rx.recv_timeout(WAIT).unwrap();
        assert!(!queue.idle());
        release_tx.send(()).unwrap();
        drop(queue);
        worker.join().unwrap();
    }

    #[test]
    fn requests_after_shutdown_never_wedge_the_idle_count() {
        let (sender, receiver) = mpsc::channel();
        drop(receiver);
        let queue = JobQueue {
            sender,
            outstanding: Arc::new(AtomicUsize::new(0)),
        };
        queue.request(Job::DrainOps);
        assert!(queue.idle());
    }
}
