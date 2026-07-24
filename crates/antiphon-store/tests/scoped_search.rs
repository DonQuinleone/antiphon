mod common;

use std::fs;

use antiphon_store::{Scope, SearchIndex, StoreLayout};
use common::{notmuch_available_or_skip, run_notmuch_new};

const VISIBLE: &str = "visible";
const HIDDEN: &str = "hidden";

const MSG_VISIBLE_ONE: &str = "\
From: Alice Example <alice@example.com>
To: bob@example.com
Subject: Visible one
Message-ID: <visible-one@example.com>
Date: Mon, 01 Jun 2026 10:00:00 +0000

Plainly in view.
";

const MSG_VISIBLE_TWO: &str = "\
From: Bob Example <bob@example.com>
To: alice@example.com
Subject: Visible two
Message-ID: <visible-two@example.com>
Date: Tue, 02 Jun 2026 11:00:00 +0000

Also in view.
";

const MSG_HIDDEN_ONE: &str = "\
From: Mallory Example <mallory@example.com>
To: alice@example.com
Subject: Hidden one
Message-ID: <hidden-one@example.com>
Date: Wed, 03 Jun 2026 12:00:00 +0000

Must never surface in the visible view.
";

const MSG_HIDDEN_TWO: &str = "\
From: Mallory Example <mallory@example.com>
To: bob@example.com
Subject: Hidden two
Message-ID: <hidden-two@example.com>
Date: Thu, 04 Jun 2026 13:00:00 +0000

Nor this one.
";

const NASTY_QUERIES: &[&str] = &[
    "*",
    "",
    "   ",
    "tag:unread",
    "path:hidden/** or tag:unread",
    "path:\"hidden/**\" or tag:unread",
    "not path:visible/**",
    "id:hidden-one@example.com",
    "from:mallory@example.com or tag:unread",
    "tag:unread) or (path:hidden/**",
    "subject:\"unterminated",
];

fn write_account(
    layout: &StoreLayout,
    account: &str,
    entries: &[(&str, &str)],
) {
    let maildir = layout.account_maildir(account);
    for sub in ["cur", "new", "tmp"] {
        fs::create_dir_all(maildir.join(sub)).unwrap();
    }
    for (name, body) in entries {
        fs::write(maildir.join(name), body).unwrap();
    }
}

fn indexed_fixture_store(
    dir: &tempfile::TempDir,
) -> (StoreLayout, SearchIndex) {
    let layout = StoreLayout::new(dir.path().join("store"));
    layout.init().unwrap();
    write_account(
        &layout,
        VISIBLE,
        &[
            ("new/1717236000.vis1.host", MSG_VISIBLE_ONE),
            ("new/1717322400.vis2.host", MSG_VISIBLE_TWO),
        ],
    );
    write_account(
        &layout,
        HIDDEN,
        &[
            ("new/1717408800.hid1.host", MSG_HIDDEN_ONE),
            ("new/1717495200.hid2.host", MSG_HIDDEN_TWO),
        ],
    );
    run_notmuch_new(&layout.notmuch_config_path());
    let index = SearchIndex::open(&layout).unwrap();
    (layout, index)
}

#[test]
fn hidden_account_never_leaks_into_a_scoped_view() {
    if !notmuch_available_or_skip() {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let (layout, index) = indexed_fixture_store(&dir);
    let scope = Scope::one(VISIBLE);
    let visible_root = layout.account_maildir(VISIBLE);

    for user_query in NASTY_QUERIES {
        let Ok(hits) = index.query_scoped(&scope, user_query, None)
        else {
            continue;
        };
        for hit in &hits {
            assert!(
                hit.path.starts_with(&visible_root),
                "user query {user_query:?} leaked {} ({})",
                hit.id,
                hit.path.display()
            );
            assert!(
                !hit.id.starts_with("hidden-"),
                "user query {user_query:?} leaked id {}",
                hit.id
            );
        }
    }
}

#[test]
fn scoped_view_still_sees_its_own_account() {
    if !notmuch_available_or_skip() {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let (_layout, index) = indexed_fixture_store(&dir);
    let scope = Scope::one(VISIBLE);

    let all = index.query_scoped(&scope, "*", None).unwrap();
    let mut ids: Vec<&str> =
        all.iter().map(|hit| hit.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        ["visible-one@example.com", "visible-two@example.com"]
    );
    assert_eq!(index.count_scoped(&scope, "*").unwrap(), 2);
    assert_eq!(index.count_scoped(&scope, "tag:unread").unwrap(), 2);
}

#[test]
fn unified_scope_covers_every_account() {
    if !notmuch_available_or_skip() {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let (_layout, index) = indexed_fixture_store(&dir);
    let scope = Scope::all(&[VISIBLE.to_owned(), HIDDEN.to_owned()]);

    assert_eq!(index.count_scoped(&scope, "*").unwrap(), 4);
    let unread =
        index.query_scoped(&scope, "tag:unread", Some(3)).unwrap();
    assert_eq!(unread.len(), 3, "limit still applies");
}

#[test]
fn duplicated_message_ids_cannot_cross_accounts() {
    if !notmuch_available_or_skip() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let layout = StoreLayout::new(dir.path().join("store"));
    layout.init().unwrap();
    let twin = "From: Twin <twin@example.com>\n\
        To: you@example.com\n\
        Subject: duplicated across accounts\n\
        Message-ID: <twin@example.com>\n\
        Date: Mon, 01 Jun 2026 10:00:00 +0000\n\n\
        Same id in both maildirs.\n";
    for account in ["visible", "hidden"] {
        let cur = layout.account_maildir(account).join("cur");
        std::fs::create_dir_all(&cur).unwrap();
        std::fs::write(cur.join("1000.twin.host:2,S"), twin).unwrap();
    }
    run_notmuch_new(&layout.notmuch_config_path());

    let index = SearchIndex::open(&layout).unwrap();
    let scope = Scope::one("visible");
    let hits = index
        .query_scoped(&scope, "id:twin@example.com", None)
        .unwrap();
    for hit in &hits {
        assert!(
            hit.path.starts_with(layout.account_maildir("visible")),
            "leaked path {}",
            hit.path.display()
        );
    }
}
