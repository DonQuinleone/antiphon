use std::path::Path;
use std::process::Command;

use antiphon_store::{SearchIndex, StoreLayout};

const MESSAGE: &str = "From: a@example.com\n\
    To: b@example.com\n\
    Subject: SUBJECT\n\
    Message-ID: <ID>\n\
    Date: Thu, 24 Jul 2026 12:00:00 +0000\n\
    \n\
    body\n";

fn deliver(dir: &Path, id: &str, subject: &str) {
    std::fs::create_dir_all(dir.join("cur")).unwrap();
    std::fs::create_dir_all(dir.join("new")).unwrap();
    std::fs::create_dir_all(dir.join("tmp")).unwrap();
    let body = MESSAGE.replace("SUBJECT", subject).replace("ID", id);
    std::fs::write(
        dir.join("cur").join(format!("{id}.antiphon:2,")),
        body,
    )
    .unwrap();
}

fn notmuch(config: &Path, args: &[&str]) {
    let output = Command::new("notmuch")
        .args(args)
        .env("NOTMUCH_CONFIG", config)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

fn tags(index: &SearchIndex, query: &str) -> Vec<String> {
    let hits = index.query(query, None).unwrap();
    assert_eq!(hits.len(), 1, "{query}");
    hits[0].tags.clone()
}

#[test]
fn only_the_inbox_folder_keeps_the_inbox_tag() {
    let dir = tempfile::tempdir().unwrap();
    let layout = StoreLayout::new(dir.path().join("store"));
    layout.init().unwrap();
    let account = layout.account_maildir("personal");
    deliver(&account, "root@example.com", "in the inbox");
    deliver(
        &account.join("archive"),
        "filed@example.com",
        "in the archive",
    );
    let config = layout.notmuch_config_path();
    notmuch(&config, &["new"]);
    antiphon_sync::test_retag(&config, "personal").unwrap();
    let index = SearchIndex::open(&layout).unwrap();
    assert!(
        tags(&index, "subject:inbox").contains(&"inbox".to_string())
    );
    assert!(
        !tags(&index, "subject:archive").contains(&"inbox".to_string())
    );
}
