mod common;

use std::fs;
use std::io::Write;

use antiphon_store::{
    ApplyOutcome, OpKind, OpLog, SearchIndex, StoreLayout, apply_op,
};
use common::{notmuch_available_or_skip, run_notmuch_new};

const ACCOUNT: &str = "test";
const ARCHIVE: &str = "archive";

const MSG_ALPHA: &str = "\
From: Alice Example <alice@example.com>
To: bob@example.com
Subject: Alpha
Message-ID: <alpha@example.com>
Date: Mon, 01 Jun 2026 10:00:00 +0000

Alpha body.
";

const MSG_BETA: &str = "\
From: Bob Example <bob@example.com>
To: alice@example.com
Subject: Beta
Message-ID: <beta@example.com>
Date: Tue, 02 Jun 2026 11:00:00 +0000

Beta body.
";

const MSG_GAMMA: &str = "\
From: Carol Example <carol@example.com>
To: bob@example.com
Subject: Gamma
Message-ID: <gamma@example.com>
Date: Wed, 03 Jun 2026 12:00:00 +0000

Gamma body.
";

fn setup_store(dir: &tempfile::TempDir) -> StoreLayout {
    let layout = StoreLayout::new(dir.path().join("store"));
    layout.init().unwrap();
    let account = layout.account_maildir(ACCOUNT);
    for sub in ["cur", "new", "tmp"] {
        fs::create_dir_all(account.join(sub)).unwrap();
    }
    let entries = [
        ("new/1000.alpha.host", MSG_ALPHA),
        ("new/2000.beta.host", MSG_BETA),
        ("cur/3000.gamma.host:2,S", MSG_GAMMA),
    ];
    for (name, body) in entries {
        fs::write(account.join(name), body).unwrap();
    }
    run_notmuch_new(&layout.notmuch_config_path());
    layout
}

fn flag_op(add: &[&str], remove: &[&str]) -> OpKind {
    OpKind::Flag {
        add: add.iter().map(|s| s.to_string()).collect(),
        remove: remove.iter().map(|s| s.to_string()).collect(),
    }
}

fn apply_now(
    layout: &StoreLayout,
    log: &mut OpLog,
    message_id: &str,
    kind: OpKind,
) -> (antiphon_store::Op, ApplyOutcome) {
    let op = log.append(ACCOUNT, message_id, kind).unwrap();
    let index = SearchIndex::open(layout).unwrap();
    let outcome = apply_op(layout, &index, &op).unwrap();
    (op, outcome)
}

fn reapply(
    layout: &StoreLayout,
    op: &antiphon_store::Op,
) -> ApplyOutcome {
    let index = SearchIndex::open(layout).unwrap();
    apply_op(layout, &index, op).unwrap()
}

fn maildir_state(layout: &StoreLayout) -> Vec<String> {
    let root = layout.maildir_root();
    let mut files = Vec::new();
    let mut dirs = vec![root.clone()];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            let rel = path.strip_prefix(&root).unwrap();
            files.push(rel.to_string_lossy().into_owned());
        }
    }
    files.sort();
    files
}

#[test]
fn flag_application_is_idempotent() {
    if !notmuch_available_or_skip() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let layout = setup_store(&dir);
    let mut log = OpLog::open(&layout).unwrap();
    let (op, outcome) = apply_now(
        &layout,
        &mut log,
        "alpha@example.com",
        flag_op(&["flagged"], &["unread"]),
    );
    assert_eq!(outcome, ApplyOutcome::Applied);
    let renamed = layout
        .account_maildir(ACCOUNT)
        .join("cur/1000.alpha.host:2,FS");
    assert!(renamed.is_file());

    let index = SearchIndex::open(&layout).unwrap();
    let hits = index.query("id:\"alpha@example.com\"", None).unwrap();
    assert_eq!(hits.len(), 1);
    assert!(!hits[0].unread);
    assert!(hits[0].tags.iter().any(|t| t == "flagged"));

    assert_eq!(reapply(&layout, &op), ApplyOutcome::Skipped);
    assert!(renamed.is_file());
}

#[test]
fn move_application_is_idempotent() {
    if !notmuch_available_or_skip() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let layout = setup_store(&dir);
    let mut log = OpLog::open(&layout).unwrap();
    let (op, outcome) = apply_now(
        &layout,
        &mut log,
        "gamma@example.com",
        OpKind::Move {
            to_folder: ARCHIVE.to_owned(),
            from_folder: None,
        },
    );
    assert_eq!(outcome, ApplyOutcome::Applied);
    let moved = layout
        .account_maildir(ACCOUNT)
        .join("archive/cur/3000.gamma.host:2,S");
    assert!(moved.is_file());
    assert_eq!(reapply(&layout, &op), ApplyOutcome::Skipped);
    assert!(moved.is_file());
}

#[test]
fn delete_application_is_idempotent() {
    if !notmuch_available_or_skip() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let layout = setup_store(&dir);
    let mut log = OpLog::open(&layout).unwrap();
    let (op, outcome) = apply_now(
        &layout,
        &mut log,
        "gamma@example.com",
        OpKind::Delete,
    );
    assert_eq!(outcome, ApplyOutcome::Applied);
    let removed = layout
        .account_maildir(ACCOUNT)
        .join("cur/3000.gamma.host:2,S");
    assert!(!removed.exists());
    assert_eq!(reapply(&layout, &op), ApplyOutcome::Skipped);
}

#[test]
fn missing_message_is_skipped() {
    if !notmuch_available_or_skip() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let layout = setup_store(&dir);
    let mut log = OpLog::open(&layout).unwrap();
    let (_, outcome) = apply_now(
        &layout,
        &mut log,
        "missing@example.com",
        OpKind::Delete,
    );
    assert_eq!(outcome, ApplyOutcome::Skipped);
}

struct CrashPlan {
    reopen_after_append: bool,
    torn_tail_after_append: bool,
    reopen_before_mark: bool,
}

const NO_CRASH: CrashPlan = CrashPlan {
    reopen_after_append: false,
    torn_tail_after_append: false,
    reopen_before_mark: false,
};

fn script() -> Vec<(&'static str, OpKind)> {
    vec![
        ("alpha@example.com", flag_op(&["flagged"], &["unread"])),
        ("beta@example.com", flag_op(&[], &["unread"])),
        (
            "beta@example.com",
            OpKind::Move {
                to_folder: ARCHIVE.to_owned(),
                from_folder: None,
            },
        ),
        ("alpha@example.com", flag_op(&["replied"], &[])),
        ("gamma@example.com", OpKind::Delete),
    ]
}

fn drain(layout: &StoreLayout, log: &mut OpLog) {
    for op in log.unapplied() {
        let index = SearchIndex::open(layout).unwrap();
        apply_op(layout, &index, &op).unwrap();
        log.mark_applied(op.id).unwrap();
    }
}

fn apply_without_marking(layout: &StoreLayout, log: &OpLog) {
    for op in log.unapplied() {
        let index = SearchIndex::open(layout).unwrap();
        apply_op(layout, &index, &op).unwrap();
    }
}

fn append_torn_tail(layout: &StoreLayout) {
    let path = layout.oplog_dir().join("ops.jsonl");
    let mut file =
        fs::OpenOptions::new().append(true).open(path).unwrap();
    file.write_all(b"{\"id\":99,\"account\":\"te").unwrap();
}

fn assert_fully_applied(log: &OpLog, expected_ops: u64) {
    assert!(log.unapplied().is_empty());
    let unsynced: Vec<u64> =
        log.unsynced().iter().map(|op| op.id).collect();
    let expected: Vec<u64> = (1..=expected_ops).collect();
    assert_eq!(unsynced, expected);
}

#[test]
fn crash_replay_matches_an_uninterrupted_run() {
    if !notmuch_available_or_skip() {
        return;
    }

    let calm_dir = tempfile::tempdir().unwrap();
    let calm = setup_store(&calm_dir);
    let mut calm_log = OpLog::open(&calm).unwrap();
    for (message_id, kind) in script() {
        calm_log.append(ACCOUNT, message_id, kind).unwrap();
    }
    drain(&calm, &mut calm_log);

    let crashy_dir = tempfile::tempdir().unwrap();
    let crashy = setup_store(&crashy_dir);
    let mut log = OpLog::open(&crashy).unwrap();
    let plans = [
        CrashPlan {
            reopen_before_mark: true,
            ..NO_CRASH
        },
        CrashPlan {
            reopen_after_append: true,
            ..NO_CRASH
        },
        CrashPlan {
            torn_tail_after_append: true,
            ..NO_CRASH
        },
        CrashPlan {
            reopen_after_append: true,
            reopen_before_mark: true,
            ..NO_CRASH
        },
        NO_CRASH,
    ];
    for ((message_id, kind), plan) in script().into_iter().zip(plans) {
        log.append(ACCOUNT, message_id, kind).unwrap();
        if plan.torn_tail_after_append {
            append_torn_tail(&crashy);
            log = OpLog::open(&crashy).unwrap();
        }
        if plan.reopen_after_append {
            log = OpLog::open(&crashy).unwrap();
        }
        if plan.reopen_before_mark {
            apply_without_marking(&crashy, &log);
            log = OpLog::open(&crashy).unwrap();
        }
        drain(&crashy, &mut log);
    }

    let expected_ops = script().len() as u64;
    assert_fully_applied(&calm_log, expected_ops);
    assert_fully_applied(&log, expected_ops);

    let final_state = maildir_state(&crashy);
    assert_eq!(final_state, maildir_state(&calm));
    assert_eq!(
        final_state,
        vec![
            "test/archive/cur/2000.beta.host:2,S".to_owned(),
            "test/cur/1000.alpha.host:2,FRS".to_owned(),
        ]
    );
}
