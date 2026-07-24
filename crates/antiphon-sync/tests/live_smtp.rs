use std::env;
use std::fs;
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use antiphon_store::{Op, OpKind, SearchIndex, StoreLayout};
use antiphon_sync::{
    Auth, SmtpAccount, SyncAccount, replay, send, sync,
};

const SMTP_HOST_VAR: &str = "ANTIPHON_TEST_SMTP_HOST";
const SMTP_PORT_VAR: &str = "ANTIPHON_TEST_SMTP_PORT";
const IMAP_HOST_VAR: &str = "ANTIPHON_TEST_IMAP_HOST";
const USER_VAR: &str = "ANTIPHON_TEST_IMAP_USER";
const PASSWORD_FILE_VAR: &str = "ANTIPHON_TEST_IMAP_PASSWORD_FILE";
const IMAPS_PORT: u16 = 993;
const ACCOUNT_NAME: &str = "live-test";
const DELIVERY_ATTEMPTS: usize = 10;
const DELIVERY_WAIT: Duration = Duration::from_secs(3);

fn password() -> Option<String> {
    let password_file = env::var(PASSWORD_FILE_VAR).ok()?;
    let password = fs::read_to_string(&password_file)
        .unwrap_or_else(|error| {
            panic!("reading {password_file}: {error}")
        })
        .trim()
        .to_owned();
    Some(password)
}

fn smtp_account() -> Option<SmtpAccount> {
    Some(SmtpAccount {
        host: env::var(SMTP_HOST_VAR).ok()?,
        port: env::var(SMTP_PORT_VAR).ok()?.parse().unwrap(),
        user: env::var(USER_VAR).ok()?,
        auth: Auth::Password(password()?),
    })
}

fn imap_account() -> Option<SyncAccount> {
    Some(SyncAccount {
        name: String::from(ACCOUNT_NAME),
        host: env::var(IMAP_HOST_VAR).ok()?,
        port: IMAPS_PORT,
        user: env::var(USER_VAR).ok()?,
        auth: Auth::Password(password()?),
    })
}

/// Sends one message through the SMTP submission path, waits
/// for it to arrive back in the same test mailbox over IMAP,
/// then deletes it through the replay path so the mailbox is
/// left exactly as found. The recipient is always the test
/// account itself; no mail ever leaves the test mailbox.
#[test]
#[ignore = "live SMTP; set ANTIPHON_TEST_SMTP_* to run"]
fn live_smtp_send_and_verify() {
    let (Some(smtp), Some(imap)) = (smtp_account(), imap_account())
    else {
        eprintln!("live SMTP env vars unset; skipping");
        return;
    };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_micros();
    let message_id =
        format!("smtp-{stamp}-{}@test.invalid", std::process::id());
    let recipient = &imap.user;
    let message = format!(
        "From: Antiphon Test <{recipient}>\r\n\
         To: Antiphon Test <{recipient}>\r\n\
         Subject: [antiphon test] smtp delivery {stamp}\r\n\
         Message-ID: <{message_id}>\r\n\
         Date: Thu, 01 Jan 2026 00:00:00 +0000\r\n\
         \r\n\
         smtp delivery test body\r\n"
    );

    send(&smtp, message.as_bytes()).unwrap();
    eprintln!("smtp accepted message <{message_id}>");

    let dir = tempfile::tempdir().unwrap();
    let layout = StoreLayout::new(dir.path().join("store"));
    layout.init().unwrap();
    let mut located = None;
    for attempt in 1..=DELIVERY_ATTEMPTS {
        sync(&imap, &layout).unwrap();
        located = SearchIndex::open(&layout)
            .unwrap()
            .locate(&message_id)
            .unwrap();
        if located.is_some() {
            eprintln!("delivered after {attempt} sync pass(es)");
            break;
        }
        sleep(DELIVERY_WAIT);
    }
    assert!(located.is_some(), "sent message never arrived over IMAP");

    let delete = Op {
        id: 1,
        account: String::from(ACCOUNT_NAME),
        message_id: message_id.clone(),
        kind: OpKind::Delete,
    };
    let report = replay(&imap, &layout, &[delete]).unwrap();
    assert_eq!(report.synced, [1]);
    eprintln!("test message deleted; mailbox left as found");
}
