use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use antiphon_store::{SearchIndex, StoreLayout};

const MESSAGE_COUNT: usize = 500;
const SEED: &str = "42";

fn notmuch_cli_available() -> bool {
    Command::new("notmuch")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

fn run_mailgen(root: &Path) {
    let output = Command::new(env!("CARGO"))
        .args([
            "run",
            "--quiet",
            "--example",
            "mailgen",
            "--",
            "--root",
        ])
        .arg(root)
        .args(["--messages", &MESSAGE_COUNT.to_string()])
        .args(["--seed", SEED])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("failed to run the mailgen example");
    assert!(
        output.status.success(),
        "mailgen failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn maildir_files(layout: &StoreLayout) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let root = layout.maildir_root();
    collect_files(&root, &root, &mut files);
    files
}

fn collect_files(
    base: &Path,
    dir: &Path,
    files: &mut BTreeMap<PathBuf, Vec<u8>>,
) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_files(base, &path, files);
            continue;
        }
        let relative = path.strip_prefix(base).unwrap().to_path_buf();
        files.insert(relative, fs::read(&path).unwrap());
    }
}

#[test]
fn same_seed_reproduces_the_same_file_set() {
    if !notmuch_cli_available() {
        eprintln!("skipping: notmuch CLI not installed");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let first = StoreLayout::new(dir.path().join("first"));
    let second = StoreLayout::new(dir.path().join("second"));
    run_mailgen(first.root());
    run_mailgen(second.root());

    let first_files = maildir_files(&first);
    let second_files = maildir_files(&second);
    assert_eq!(first_files.len(), MESSAGE_COUNT);
    assert_eq!(
        first_files.keys().collect::<Vec<_>>(),
        second_files.keys().collect::<Vec<_>>(),
        "file names differ between runs"
    );
    assert!(
        first_files == second_files,
        "file contents differ between runs"
    );
}

#[test]
fn generated_store_indexes_and_answers_queries() {
    if !notmuch_cli_available() {
        eprintln!("skipping: notmuch CLI not installed");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let layout = StoreLayout::new(dir.path().join("store"));
    run_mailgen(layout.root());

    let index = SearchIndex::open(&layout).unwrap();
    let all = index.query("*", None).unwrap();
    assert_eq!(all.len(), MESSAGE_COUNT);

    let threads: HashSet<&str> =
        all.iter().map(|m| m.thread_id.as_str()).collect();
    assert!(
        threads.len() < MESSAGE_COUNT,
        "replies should share thread ids"
    );

    let unread = index.query("tag:unread", None).unwrap();
    assert!(!unread.is_empty(), "recent messages stay unread");
    assert!(
        unread.len() < MESSAGE_COUNT,
        "seen flags should strip unread"
    );

    let scoped = index.query("path:acct0/**", None).unwrap();
    assert!(!scoped.is_empty(), "account scoping works");

    let by_word = index.query("archive", None).unwrap();
    assert!(!by_word.is_empty(), "body text is searchable");
}
