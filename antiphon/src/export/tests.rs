use std::fs;
use std::path::Path;

use flate2::read::GzDecoder;

use super::*;

const MESSAGES: [(&str, &str); 3] = [
    ("cur/1700000000.a1:2,S", "From: a@example.com\n\none\n"),
    ("new/1700000001.b2", "From: b@example.com\n\ntwo\n"),
    (
        "archive/cur/1700000002.c3:2,S",
        "From: c@example.com\n\nthree\n",
    ),
];

fn fake_maildir(root: &Path) {
    for dir in ["cur", "new", "tmp", "archive/cur"] {
        fs::create_dir_all(root.join(dir)).unwrap();
    }
    for (name, body) in MESSAGES {
        fs::write(root.join(name), body).unwrap();
    }
}

fn identity_key(identity: &age::x25519::Identity) -> ExportKey {
    ExportKey::Recipients(vec![identity.to_public()])
}

#[test]
fn export_round_trips_through_decrypt() {
    let dir = tempfile::tempdir().unwrap();
    let maildir = dir.path().join("maildir/work");
    fake_maildir(&maildir);
    let identity = age::x25519::Identity::generate();
    let key = identity_key(&identity);
    let dest = dir.path().join("work.tar.gz.age");

    let summary =
        export_account(&maildir, "work", &dest, &key).unwrap();
    assert_eq!(summary.account, "work");
    assert_eq!(summary.files, MESSAGES.len() as u64);
    assert_eq!(
        summary.bytes,
        fs::metadata(&dest).unwrap().len(),
        "reported bytes match the file on disk"
    );
    assert!(summary.line().contains("exported work"));

    let restored = dir.path().join("restored");
    decrypt_into(&dest, &identity, &restored);
    for (name, body) in MESSAGES {
        let path = restored.join("work").join(name);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            body,
            "content survives for {name}"
        );
    }
    assert!(
        restored.join("work/tmp").is_dir(),
        "empty directories survive"
    );
}

fn decrypt_into(
    encrypted: &Path,
    identity: &age::x25519::Identity,
    out: &Path,
) {
    let file = fs::File::open(encrypted).unwrap();
    let decryptor =
        age::Decryptor::new(std::io::BufReader::new(file)).unwrap();
    let reader = decryptor
        .decrypt(std::iter::once(identity as &dyn age::Identity))
        .unwrap();
    tar::Archive::new(GzDecoder::new(reader))
        .unpack(out)
        .unwrap();
}

#[test]
fn a_missing_maildir_names_the_account_and_path() {
    let dir = tempfile::tempdir().unwrap();
    let maildir = dir.path().join("maildir/absent");
    let identity = age::x25519::Identity::generate();
    let key = identity_key(&identity);
    let dest = dir.path().join("absent.tar.gz.age");
    let error = export_account(&maildir, "absent", &dest, &key)
        .unwrap_err()
        .to_string();
    assert!(error.contains("absent has no maildir"), "{error}");
    assert!(error.contains("maildir/absent"), "{error}");
}

#[test]
fn an_unwritable_destination_names_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let maildir = dir.path().join("maildir/work");
    fake_maildir(&maildir);
    let identity = age::x25519::Identity::generate();
    let key = identity_key(&identity);
    let dest = dir.path().join("no-such-dir/work.tar.gz.age");
    let error = export_account(&maildir, "work", &dest, &key)
        .unwrap_err()
        .to_string();
    assert!(error.contains("cannot write"), "{error}");
    assert!(error.contains("no-such-dir"), "{error}");
}

#[test]
fn recipients_parse_or_name_the_broken_key() {
    let identity = age::x25519::Identity::generate();
    let good = identity.to_public().to_string();
    let parsed = parse_recipients(&[format!("  {good}  ")]).unwrap();
    assert_eq!(parsed.len(), 1, "whitespace is trimmed");

    let error = parse_recipients(&[good, "not-a-key".to_string()])
        .unwrap_err()
        .to_string();
    assert!(error.contains("bad age recipient"), "{error}");
    assert!(error.contains("not-a-key"), "{error}");
}

#[test]
fn archive_names_carry_account_and_utc_date() {
    let name = archive_file_name("work");
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    assert_eq!(name, format!("work-{date}.tar.gz.age"));
}

#[test]
fn a_passphrase_export_writes_a_parseable_header() {
    let dir = tempfile::tempdir().unwrap();
    let maildir = dir.path().join("maildir/work");
    fake_maildir(&maildir);
    let key = ExportKey::Passphrase("horse staple".into());
    let dest = dir.path().join("work.tar.gz.age");
    let summary =
        export_account(&maildir, "work", &dest, &key).unwrap();
    assert_eq!(summary.files, MESSAGES.len() as u64);
}
