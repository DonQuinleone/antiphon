mod common;

use std::fs;
use std::path::PathBuf;

use antiphon_store::{Scope, SearchIndex, StoreLayout, scoped_query};
use common::{notmuch_available_or_skip, run_notmuch_new};

const ACCOUNT: &str = "work";

const MSG_ROOT: &str = "\
From: Alice Example <alice@example.com>
To: me@example.com
Subject: Plan
Message-ID: <root@example.com>
Date: Mon, 01 Jun 2026 10:00:00 +0000

Kick-off in the inbox.
";

const MSG_OWN_REPLY: &str = "\
From: Me Example <me@example.com>
To: alice@example.com
Subject: Re: Plan
Message-ID: <myreply@example.com>
In-Reply-To: <root@example.com>
References: <root@example.com>
Date: Mon, 01 Jun 2026 11:00:00 +0000

Sounds good, filed in Sent.
";

const MSG_THIRD: &str = "\
From: Alice Example <alice@example.com>
To: me@example.com
Subject: Re: Plan
Message-ID: <third@example.com>
In-Reply-To: <myreply@example.com>
References: <root@example.com> <myreply@example.com>
Date: Mon, 01 Jun 2026 12:00:00 +0000

Later moved to Archive.
";

/// One thread deliberately scattered across three folders of the
/// same account, the middle message being the user's own reply.
fn write_thread(layout: &StoreLayout) {
    let entries = [
        ("Inbox", "1717236000.root.host", MSG_ROOT),
        ("Sent", "1717239600.myreply.host", MSG_OWN_REPLY),
        ("Archive", "1717243200.third.host", MSG_THIRD),
    ];
    let account = layout.account_maildir(ACCOUNT);
    for (folder, name, body) in entries {
        let cur = account.join(folder).join("cur");
        fs::create_dir_all(&cur).unwrap();
        fs::write(cur.join(name), body).unwrap();
    }
}

fn indexed_store(
    dir: &tempfile::TempDir,
) -> (StoreLayout, SearchIndex) {
    let layout = StoreLayout::new(dir.path().join("store"));
    layout.init().unwrap();
    write_thread(&layout);
    run_notmuch_new(&layout.notmuch_config_path());
    let index = SearchIndex::open(&layout).unwrap();
    (layout, index)
}

fn thread_of(index: &SearchIndex, id: &str) -> String {
    let hits = index.query(&format!("id:{id}"), None).unwrap();
    hits.first().expect("the seed message").thread_id.clone()
}

#[test]
fn the_thread_pivot_gathers_every_folder_and_the_own_reply() {
    if !notmuch_available_or_skip() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (layout, index) = indexed_store(&dir);
    let thread = thread_of(&index, "root@example.com");

    let scope = Scope::one(ACCOUNT);
    let query =
        scoped_query(&scope, &format!("thread:{thread}")).unwrap();
    let hits = index.query(&query, None).unwrap();

    let mut ids: Vec<&str> =
        hits.iter().map(|hit| hit.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        [
            "myreply@example.com",
            "root@example.com",
            "third@example.com",
        ],
        "the pivot must span inbox, sent and archive"
    );

    let account = layout.account_maildir(ACCOUNT);
    let folder_of = |id: &str| -> PathBuf {
        let hit = hits.iter().find(|hit| hit.id == id).unwrap();
        hit.path
            .strip_prefix(&account)
            .unwrap()
            .components()
            .next()
            .unwrap()
            .as_os_str()
            .into()
    };
    assert_eq!(folder_of("root@example.com"), PathBuf::from("Inbox"));
    assert_eq!(
        folder_of("myreply@example.com"),
        PathBuf::from("Sent"),
        "the user's own reply belongs to the thread"
    );
    assert_eq!(
        folder_of("third@example.com"),
        PathBuf::from("Archive")
    );
}
