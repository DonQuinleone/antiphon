use std::env;
use std::fs;
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use antiphon_store::{Op, OpKind, SearchIndex, StoreLayout};
use antiphon_sync::{SyncAccount, replay, sync};
use imap::Session;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, StreamOwned};

const HOST_VAR: &str = "ANTIPHON_TEST_IMAP_HOST";
const USER_VAR: &str = "ANTIPHON_TEST_IMAP_USER";
const PASSWORD_FILE_VAR: &str = "ANTIPHON_TEST_IMAP_PASSWORD_FILE";
const IMAPS_PORT: u16 = 993;
const INBOX: &str = "INBOX";
const ACCOUNT_NAME: &str = "live-test";
const UID_MARKER: &str = ",U=";

type TestSession = Session<StreamOwned<ClientConnection, TcpStream>>;

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
        name: String::from(ACCOUNT_NAME),
        host,
        port: IMAPS_PORT,
        user,
        password,
    })
}

fn open_session(account: &SyncAccount) -> TestSession {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let server_name =
        ServerName::try_from(account.host.clone()).unwrap();
    let connection =
        ClientConnection::new(Arc::new(config), server_name).unwrap();
    let tcp = TcpStream::connect((account.host.as_str(), account.port))
        .unwrap();
    let mut client =
        imap::Client::new(StreamOwned::new(connection, tcp));
    client.read_greeting().unwrap();
    client
        .login(&account.user, &account.password)
        .map_err(|(error, _)| error)
        .unwrap()
}

fn server_flags(
    account: &SyncAccount,
    uid: u32,
) -> Option<Vec<String>> {
    let mut session = open_session(account);
    session.select(INBOX).unwrap();
    let fetches =
        session.uid_fetch(uid.to_string(), "(UID FLAGS)").unwrap();
    let flags = fetches
        .iter()
        .find(|fetch| fetch.uid == Some(uid))
        .map(|fetch| {
            fetch
                .flags()
                .iter()
                .map(|flag| format!("{flag:?}"))
                .collect()
        });
    let _ = session.logout();
    flags
}

fn uid_from_path(path: &std::path::Path) -> u32 {
    let name = path.file_name().unwrap().to_str().unwrap();
    let (_, after) = name.rsplit_once(UID_MARKER).unwrap();
    let digits: String =
        after.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().unwrap()
}

fn op(id: u64, message_id: &str, kind: OpKind) -> Op {
    Op {
        id,
        account: String::from(ACCOUNT_NAME),
        message_id: message_id.to_owned(),
        kind,
    }
}

fn flag_op(
    id: u64,
    message_id: &str,
    add: &[&str],
    remove: &[&str],
) -> Op {
    let owned =
        |tags: &[&str]| tags.iter().map(|&t| t.to_owned()).collect();
    op(
        id,
        message_id,
        OpKind::Flag {
            add: owned(add),
            remove: owned(remove),
        },
    )
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

/// Appends its own message, flips flags on it through the
/// replay path, verifies each change on the server through a
/// fresh connection, and finally deletes the message via a
/// replayed Delete, leaving the mailbox as it found it.
#[test]
#[ignore = "live IMAP; set ANTIPHON_TEST_IMAP_* to run"]
fn live_flag_replay_round_trip() {
    let Some(account) = live_account() else {
        eprintln!("live IMAP env vars unset; skipping");
        return;
    };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_micros();
    let message_id =
        format!("replay-{stamp}-{}@test.invalid", std::process::id());
    let message = format!(
        "From: Antiphon Test <sender@example.com>\r\n\
         To: Antiphon Test <recipient@example.com>\r\n\
         Subject: replay round trip\r\n\
         Message-ID: <{message_id}>\r\n\
         Date: Thu, 01 Jan 2026 00:00:00 +0000\r\n\
         \r\n\
         replay test body\r\n"
    );
    let mut session = open_session(&account);
    session.append(INBOX, &message).unwrap();
    let _ = session.logout();

    let dir = tempfile::tempdir().unwrap();
    let layout = StoreLayout::new(dir.path().join("store"));
    layout.init().unwrap();
    sync(&account, &layout).unwrap();
    let index = SearchIndex::open(&layout).unwrap();
    let path = index
        .locate(&message_id)
        .unwrap()
        .expect("appended message missing after sync");
    let uid = uid_from_path(&path);
    let before = server_flags(&account, uid)
        .expect("appended message missing on the server");
    eprintln!("uid {uid} before replay: {before:?}");
    assert!(
        !before.iter().any(|flag| flag.contains("Flagged")),
        "test message arrived already flagged: {before:?}"
    );

    let flip = flag_op(1, &message_id, &["flagged"], &["unread"]);
    let report = replay(&account, &layout, &[flip]).unwrap();
    assert_eq!(report.synced, [1]);
    let flagged = server_flags(&account, uid).unwrap();
    eprintln!("uid {uid} after flag replay: {flagged:?}");
    assert!(
        flagged.iter().any(|flag| flag.contains("Flagged")),
        "server does not report \\Flagged: {flagged:?}"
    );
    assert!(
        flagged.iter().any(|flag| flag.contains("Seen")),
        "server does not report \\Seen: {flagged:?}"
    );

    let restore = flag_op(2, &message_id, &["unread"], &["flagged"]);
    let report = replay(&account, &layout, &[restore]).unwrap();
    assert_eq!(report.synced, [2]);
    let restored = server_flags(&account, uid).unwrap();
    eprintln!("uid {uid} after restore replay: {restored:?}");
    assert!(
        !restored.iter().any(|flag| {
            flag.contains("Flagged") || flag.contains("Seen")
        }),
        "flags were not restored: {restored:?}"
    );

    let unsupported_move = op(
        3,
        &message_id,
        OpKind::Move {
            to_folder: String::from("archive"),
        },
    );
    let ghost =
        flag_op(4, &format!("ghost-{message_id}"), &["flagged"], &[]);
    let delete = op(5, &message_id, OpKind::Delete);
    let report =
        replay(&account, &layout, &[unsupported_move, ghost, delete])
            .unwrap();
    assert_eq!(report.unsupported, [3]);
    assert_eq!(report.dropped, [4]);
    assert_eq!(report.synced, [5]);
    assert!(
        server_flags(&account, uid).is_none(),
        "deleted message still on the server"
    );
    eprintln!("uid {uid} deleted; mailbox left as found");
}
