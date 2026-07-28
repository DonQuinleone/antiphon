use std::fs;
use std::io::Write;
use std::path::Path;

use flate2::Compression;
use flate2::write::GzEncoder;

use super::*;
use crate::export::{ExportKey, export_account};

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

fn exported_archive(
    dir: &Path,
    identity: &age::x25519::Identity,
) -> std::path::PathBuf {
    let maildir = dir.join("maildir/work");
    fake_maildir(&maildir);
    let dest = dir.join("work-2026-07-28.tar.gz.age");
    let key = ExportKey::Recipients(vec![identity.to_public()]);
    export_account(&maildir, "work", &dest, &key).unwrap();
    dest
}

fn identity_key(identity: &age::x25519::Identity) -> ViewKey {
    ViewKey::Identities(vec![identity.clone()])
}

#[test]
fn an_export_unpacks_indexed_under_the_archive_stem() {
    let dir = tempfile::tempdir().unwrap();
    let identity = age::x25519::Identity::generate();
    let archive = exported_archive(dir.path(), &identity);
    let store_root = dir.path().join("view/work-2026-07-28");
    let account = archive_stem(&archive);
    assert_eq!(account, "work-2026-07-28");

    let opened = open_archive(
        &archive,
        &store_root,
        &account,
        &identity_key(&identity),
    )
    .unwrap();
    assert_eq!(
        opened,
        Opened::Unpacked {
            files: MESSAGES.len() as u64
        }
    );
    let layout = StoreLayout::new(&store_root);
    for (name, body) in MESSAGES {
        let path = layout.account_maildir(&account).join(name);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            body,
            "content survives for {name}"
        );
    }
    assert!(store_root.join(COMPLETE_MARKER).is_file());
    assert_eq!(layout.account_folders(&account), ["archive"]);
    let index = antiphon_store::SearchIndex::open(&layout).unwrap();
    assert_eq!(index.count("*").unwrap(), MESSAGES.len() as u32);
}

#[test]
fn a_complete_unpack_is_reused_and_a_partial_one_redone() {
    let dir = tempfile::tempdir().unwrap();
    let identity = age::x25519::Identity::generate();
    let archive = exported_archive(dir.path(), &identity);
    let store_root = dir.path().join("view/work-2026-07-28");
    let key = identity_key(&identity);

    // A leftover directory without the marker is a partial
    // unpack: it must be discarded, not trusted.
    let leftover = store_root.join("maildir/junk");
    fs::create_dir_all(&leftover).unwrap();
    let opened =
        open_archive(&archive, &store_root, "work-2026-07-28", &key)
            .unwrap();
    assert!(matches!(opened, Opened::Unpacked { .. }));
    assert!(!leftover.exists(), "the partial unpack is gone");

    let sentinel = store_root.join("maildir/sentinel");
    fs::write(&sentinel, "untouched").unwrap();
    let opened =
        open_archive(&archive, &store_root, "work-2026-07-28", &key)
            .unwrap();
    assert_eq!(opened, Opened::Reused);
    assert!(sentinel.is_file(), "a complete unpack is left alone");
}

#[test]
fn a_passphrase_archive_decrypts_with_the_passphrase() {
    let dir = tempfile::tempdir().unwrap();
    let maildir = dir.path().join("maildir/work");
    fake_maildir(&maildir);
    let archive = dir.path().join("work.tar.gz.age");
    let key = ExportKey::Passphrase("horse staple".into());
    export_account(&maildir, "work", &archive, &key).unwrap();

    let store_root = dir.path().join("view/work");
    let wrong = ViewKey::Passphrase("wrong".into());
    let error = open_archive(&archive, &store_root, "work", &wrong)
        .unwrap_err()
        .to_string();
    assert!(error.contains("decryption failed"), "{error}");
    assert!(
        !store_root.join(COMPLETE_MARKER).exists(),
        "no marker after a failure"
    );

    let right = ViewKey::Passphrase("horse staple".into());
    let opened =
        open_archive(&archive, &store_root, "work", &right).unwrap();
    assert!(matches!(opened, Opened::Unpacked { .. }));
}

/// A tar.gz.age built by hand around a hostile entry, since
/// the exporter never writes one.
fn hostile_archive(
    dir: &Path,
    identity: &age::x25519::Identity,
    entry_name: &str,
) -> std::path::PathBuf {
    let destination = dir.join("hostile.tar.gz.age");
    let file = fs::File::create(&destination).unwrap();
    let encryptor = age::Encryptor::with_recipients(std::iter::once(
        &identity.to_public() as &dyn age::Recipient,
    ))
    .unwrap();
    let stream = encryptor.wrap_output(file).unwrap();
    let gz = GzEncoder::new(stream, Compression::default());
    let mut builder = tar::Builder::new(gz);
    let body = b"owned\n";
    let mut header = tar::Header::new_gnu();
    // set_path refuses "..", so the name bytes are forged the
    // way a hostile archive would carry them.
    let name = entry_name.as_bytes();
    header.as_gnu_mut().unwrap().name[..name.len()]
        .copy_from_slice(name);
    header.set_size(body.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append(&header, body.as_slice()).unwrap();
    let gz = builder.into_inner().unwrap();
    let stream = gz.finish().unwrap();
    stream.finish().unwrap().flush().unwrap();
    destination
}

#[test]
fn traversal_entries_are_rejected_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    let identity = age::x25519::Identity::generate();
    let archive =
        hostile_archive(dir.path(), &identity, "work/../../escape");
    let store_root = dir.path().join("view/hostile");
    let error = open_archive(
        &archive,
        &store_root,
        "hostile",
        &identity_key(&identity),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("refusing archive entry"), "{error}");
    assert!(error.contains("escapes the store"), "{error}");
    let layout = StoreLayout::new(&store_root);
    assert!(
        !layout.maildir_root().join("escape").exists()
            && !dir.path().join("escape").exists(),
        "nothing was written outside the maildir"
    );
    assert!(!store_root.join(COMPLETE_MARKER).exists());
}

#[test]
fn archive_stems_strip_only_archive_extensions() {
    let cases = [
        ("work-2026-07-28.tar.gz.age", "work-2026-07-28"),
        ("inbox.backup.tar.gz.age", "inbox.backup"),
        ("plain.tar", "plain"),
        ("noext", "noext"),
    ];
    for (name, expected) in cases {
        assert_eq!(archive_stem(Path::new(name)), expected, "{name}");
        assert_eq!(
            archive_stem(&Path::new("/exports").join(name)),
            expected,
            "{name} with a directory"
        );
    }
}
