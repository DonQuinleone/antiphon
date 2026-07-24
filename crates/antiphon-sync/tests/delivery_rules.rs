use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use antiphon_store::{OpKind, OpLog, SearchIndex, StoreLayout};
use antiphon_sync::{DeliveryRule, apply_rules};

const ACCOUNT: &str = "test";
const LIST_FOLDER: &str = "lists/aerc";

const MSG_LIST: &str = "\
From: Mara Voss <mara@example.com>
To: quin@example.com
Subject: [PATCH] fix the thing
Message-ID: <patch-1@example.com>
List-Id: aerc-devel <~sircmpwn/aerc-devel.lists.sr.ht>
Date: Mon, 01 Jun 2026 10:00:00 +0000

Patch body.
";

const MSG_PLAIN: &str = "\
From: Carol Example <carol@example.com>
To: quin@example.com
Subject: lunch?
Message-ID: <lunch-1@example.com>
Date: Tue, 02 Jun 2026 11:00:00 +0000

Plain body.
";

fn notmuch_available_or_skip() -> bool {
    let available = Command::new("notmuch")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success());
    if available {
        return true;
    }
    eprintln!("skipping: notmuch CLI not installed");
    false
}

fn run_notmuch_new(config: &Path) {
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

struct Store {
    layout: StoreLayout,
    list_path: PathBuf,
    plain_path: PathBuf,
}

fn setup_store(dir: &tempfile::TempDir) -> Store {
    let layout = StoreLayout::new(dir.path().join("store"));
    layout.init().unwrap();
    let inbox = layout.account_maildir(ACCOUNT);
    for sub in ["cur", "new", "tmp"] {
        fs::create_dir_all(inbox.join(sub)).unwrap();
    }
    let list_path = inbox.join("new/1000.list.host");
    let plain_path = inbox.join("new/2000.plain.host");
    fs::write(&list_path, MSG_LIST).unwrap();
    fs::write(&plain_path, MSG_PLAIN).unwrap();
    run_notmuch_new(&layout.notmuch_config_path());
    Store {
        layout,
        list_path,
        plain_path,
    }
}

fn list_rule() -> DeliveryRule {
    DeliveryRule {
        match_list: Some("~sircmpwn/aerc-devel".to_owned()),
        match_sender: None,
        move_to: Some(LIST_FOLDER.to_owned()),
        tag: Some("aerc".to_owned()),
    }
}

fn sender_tag_rule() -> DeliveryRule {
    DeliveryRule {
        match_list: None,
        match_sender: Some("carol@example.com".to_owned()),
        move_to: None,
        tag: Some("from-carol".to_owned()),
    }
}

fn tags_of(layout: &StoreLayout, query: &str) -> Vec<String> {
    let index = SearchIndex::open(layout).unwrap();
    let hits = index.query(query, None).unwrap();
    assert_eq!(hits.len(), 1, "expected one hit for {query}");
    hits[0].tags.clone()
}

#[test]
fn a_sender_rule_tags_without_touching_the_oplog() {
    if !notmuch_available_or_skip() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let store = setup_store(&dir);
    let mut log = OpLog::open(&store.layout).unwrap();
    let delivered =
        vec![store.list_path.clone(), store.plain_path.clone()];
    let outcome = apply_rules(
        ACCOUNT,
        &[sender_tag_rule()],
        &delivered,
        &store.layout,
        &mut log,
    );
    assert_eq!(outcome.tagged, 1);
    assert_eq!(outcome.moved, 0);
    let tags = tags_of(&store.layout, "id:lunch-1@example.com");
    assert!(tags.contains(&"from-carol".to_owned()), "{tags:?}");
    let other = tags_of(&store.layout, "id:patch-1@example.com");
    assert!(!other.contains(&"from-carol".to_owned()), "{other:?}");
    assert!(log.unsynced().is_empty());
    assert!(store.list_path.exists());
    assert!(store.plain_path.exists());
}

#[test]
fn a_list_rule_moves_through_the_oplog_and_tags() {
    if !notmuch_available_or_skip() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let store = setup_store(&dir);
    let mut log = OpLog::open(&store.layout).unwrap();
    let delivered =
        vec![store.list_path.clone(), store.plain_path.clone()];
    let outcome = apply_rules(
        ACCOUNT,
        &[list_rule()],
        &delivered,
        &store.layout,
        &mut log,
    );
    assert_eq!(outcome.tagged, 1);
    assert_eq!(outcome.moved, 1);
    assert!(!store.list_path.exists());
    assert!(store.plain_path.exists());
    let target = store
        .layout
        .account_maildir(ACCOUNT)
        .join(LIST_FOLDER)
        .join("cur");
    let landed: Vec<_> = fs::read_dir(&target)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(landed.len(), 1, "{landed:?}");
    let pending = log.unsynced();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].account, ACCOUNT);
    assert_eq!(pending[0].message_id, "patch-1@example.com");
    assert!(matches!(
        &pending[0].kind,
        OpKind::Move { to_folder } if to_folder == LIST_FOLDER
    ));
    assert!(log.unapplied().is_empty(), "move op was applied");
    let tags = tags_of(&store.layout, "id:patch-1@example.com");
    assert!(tags.contains(&"aerc".to_owned()), "{tags:?}");
}

#[test]
fn an_unreadable_delivery_never_fails_the_pass() {
    if !notmuch_available_or_skip() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let store = setup_store(&dir);
    let mut log = OpLog::open(&store.layout).unwrap();
    let missing = store
        .layout
        .account_maildir(ACCOUNT)
        .join("new/9999.gone.host");
    let delivered = vec![missing, store.plain_path.clone()];
    let outcome = apply_rules(
        ACCOUNT,
        &[sender_tag_rule()],
        &delivered,
        &store.layout,
        &mut log,
    );
    assert_eq!(outcome.tagged, 1);
    let tags = tags_of(&store.layout, "id:lunch-1@example.com");
    assert!(tags.contains(&"from-carol".to_owned()), "{tags:?}");
}
