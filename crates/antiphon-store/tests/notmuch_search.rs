use std::fs;
use std::path::Path;
use std::process::Command;

use antiphon_store::{SearchIndex, StoreLayout};

const ACCOUNT: &str = "test";

const MSG_VAULT: &str = "\
From: Alice Example <alice@example.com>
To: bob@example.com
Subject: Provisioning the vault
Message-ID: <vault@example.com>
Date: Mon, 01 Jun 2026 10:00:00 +0000

The vault mounts cleanly on both platforms.
";

const MSG_OPLOG: &str = "\
From: Bob Example <bob@example.com>
To: alice@example.com
Subject: Oplog replay ordering
Message-ID: <oplog@example.com>
Date: Tue, 02 Jun 2026 11:00:00 +0000

Replay must stay idempotent.
";

const MSG_SEEN: &str = "\
From: Carol Example <carol@example.com>
To: bob@example.com
Subject: Already read this one
Message-ID: <seen@example.com>
Date: Wed, 03 Jun 2026 12:00:00 +0000

Filed in cur/ with the seen flag.
";

fn notmuch_cli_available() -> bool {
    Command::new("notmuch")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

fn write_fixture_maildir(layout: &StoreLayout) {
    let account = layout.account_maildir(ACCOUNT);
    for sub in ["cur", "new", "tmp"] {
        fs::create_dir_all(account.join(sub)).unwrap();
    }
    let entries = [
        ("new/1717236000.vault.host", MSG_VAULT),
        ("new/1717322400.oplog.host", MSG_OPLOG),
        ("cur/1717408800.seen.host:2,S", MSG_SEEN),
    ];
    for (name, body) in entries {
        fs::write(account.join(name), body).unwrap();
    }
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

#[test]
fn wrapper_reads_an_index_built_by_notmuch_new() {
    if !notmuch_cli_available() {
        eprintln!(
            "skipping: notmuch CLI not installed \
             (brew install notmuch / apt install notmuch)"
        );
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let layout = StoreLayout::new(dir.path().join("store"));
    layout.init().unwrap();
    write_fixture_maildir(&layout);

    run_notmuch_new(&layout.notmuch_config_path());

    let index = SearchIndex::open(&layout).unwrap();
    let all = index.query("*", None).unwrap();
    assert_eq!(all.len(), 3);

    let subjects: Vec<&str> =
        all.iter().map(|m| m.subject.as_str()).collect();
    for expected in [
        "Provisioning the vault",
        "Oplog replay ordering",
        "Already read this one",
    ] {
        assert!(
            subjects.contains(&expected),
            "missing subject {expected:?} in {subjects:?}"
        );
    }

    let vault =
        all.iter().find(|m| m.id == "vault@example.com").unwrap();
    assert!(vault.unread);
    assert!(vault.tags.iter().any(|t| t == "inbox"));
    assert!(vault.from.contains("alice@example.com"));

    let seen = all.iter().find(|m| m.id == "seen@example.com").unwrap();
    assert!(!seen.unread, "seen flag should strip unread");
    assert!(seen.tags.iter().any(|t| t == "inbox"));

    let scoped = index
        .query(&format!("path:{ACCOUNT}/** and tag:unread"), None)
        .unwrap();
    assert_eq!(scoped.len(), 2);
    assert!(scoped[0].date_unix > scoped[1].date_unix, "newest first");
}
