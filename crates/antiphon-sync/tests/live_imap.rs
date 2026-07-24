use std::env;
use std::fs;
use std::time::Instant;

use antiphon_store::{SearchIndex, StoreLayout};
use antiphon_sync::{SyncAccount, sync};

const HOST_VAR: &str = "ANTIPHON_TEST_IMAP_HOST";
const USER_VAR: &str = "ANTIPHON_TEST_IMAP_USER";
const PASSWORD_FILE_VAR: &str = "ANTIPHON_TEST_IMAP_PASSWORD_FILE";
const IMAPS_PORT: u16 = 993;

fn live_account() -> Option<SyncAccount> {
    let host = env::var(HOST_VAR).ok()?;
    let user = env::var(USER_VAR).ok()?;
    let password_file = env::var(PASSWORD_FILE_VAR).ok()?;
    let password = fs::read_to_string(&password_file)
        .unwrap_or_else(|error| {
            panic!("reading {password_file}: {error}")
        })
        .trim()
        .to_owned();
    Some(SyncAccount {
        name: String::from("live-test"),
        host,
        port: IMAPS_PORT,
        user,
        password,
    })
}

#[test]
#[ignore = "live IMAP; set ANTIPHON_TEST_IMAP_* to run"]
fn live_initial_and_incremental_sync() {
    let Some(account) = live_account() else {
        eprintln!("live IMAP env vars unset; skipping");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let layout = StoreLayout::new(dir.path().join("store"));
    layout.init().unwrap();

    let started = Instant::now();
    let initial = sync(&account, &layout).unwrap();
    let initial_elapsed = started.elapsed();
    assert!(!initial.folders.is_empty(), "server listed no folders");
    eprintln!(
        "initial sync: {} new, {} updated, {} folders, \
         {initial_elapsed:.2?}",
        initial.total_new(),
        initial.total_updated(),
        initial.folders.len()
    );
    for folder in &initial.folders {
        eprintln!(
            "  {}: {} new, {} updated",
            folder.folder, folder.new_messages, folder.updated_messages
        );
    }

    let index = SearchIndex::open(&layout).unwrap();
    let indexed = index.count("*").unwrap() as usize;
    eprintln!("notmuch indexed {indexed} messages");
    assert!(
        indexed > 0 || initial.total_new() == 0,
        "synced {} messages but the index is empty",
        initial.total_new()
    );
    assert!(
        indexed <= initial.total_new(),
        "index holds {indexed} messages but only {} were synced",
        initial.total_new()
    );

    let again = Instant::now();
    let incremental = sync(&account, &layout).unwrap();
    let incremental_elapsed = again.elapsed();
    eprintln!(
        "incremental sync: {} new, {} updated, \
         {incremental_elapsed:.2?}",
        incremental.total_new(),
        incremental.total_updated()
    );
    assert_eq!(
        incremental.total_new(),
        0,
        "a second sync refetched messages"
    );
    let recount =
        SearchIndex::open(&layout).unwrap().count("*").unwrap();
    assert_eq!(recount as usize, indexed);
}
