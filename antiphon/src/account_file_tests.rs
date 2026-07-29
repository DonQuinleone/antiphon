use antiphon_config::Identity;

use super::*;

const FINGERPRINT: &str = "1234567890ABCDEF1234567890ABCDEF12345678";

fn dirs_at(root: &Path) -> Dirs {
    Dirs {
        config: root.to_path_buf(),
        state: root.join("state"),
        cache: root.join("cache"),
        data: root.join("data"),
    }
}

fn answers() -> AccountAnswers {
    AccountAnswers {
        name: "work".to_string(),
        address: "quin@example.com".to_string(),
        from_name: "Quin at Work".to_string(),
        imap_host: "imap.example.com".to_string(),
        imap_user: "quin@example.com".to_string(),
        smtp_host: "smtp.example.com".to_string(),
        password_cmd: "pass show mail/work".to_string(),
    }
}

fn identity(address: &str, name: Option<&str>) -> Identity {
    Identity {
        address: address.to_string(),
        name: name.map(str::to_string),
        signature: None,
        matches: vec![address.to_string()],
        pgp_sign: false,
        pgp_key: None,
    }
}

#[test]
fn account_toml_carries_every_field() {
    let text = account_toml(&answers());
    assert!(text.contains("name = \"work\""));
    assert!(text.contains("host = \"imap.example.com\""));
    assert!(text.contains("host = \"smtp.example.com\""));
    assert!(text.contains("address = \"quin@example.com\""));
    assert!(text.contains("name = \"Quin at Work\""));
    assert!(text.contains("password_cmd = \"pass show mail/work\""));
}

#[test]
fn an_empty_from_name_is_left_out_of_a_fresh_identity() {
    let mut no_name = answers();
    no_name.from_name = String::new();
    let text = account_toml(&no_name);
    assert!(text.contains("address = \"quin@example.com\""));
    assert!(
        !text.contains("name = \"Quin"),
        "no identity name line without a from name: {text}"
    );
}

#[test]
fn an_empty_password_command_is_left_out_entirely() {
    let mut oauth_answers = answers();
    oauth_answers.password_cmd = String::new();
    let text = account_toml(&oauth_answers);
    assert!(!text.contains("password_cmd"), "{text}");
    assert!(text.contains("[smtp]"));

    let edited =
        edited_account_toml(hand_written_toml(), &oauth_answers);
    assert!(
        edited.contains(
            "password_cmd = \"pass show mail/work\"  # rotate soon"
        ),
        "an existing command is not blanked: {edited}"
    );
}

#[test]
fn write_account_only_succeeds_once() {
    let root = tempfile::tempdir().unwrap();
    let dirs = dirs_at(root.path());
    write_account(&dirs, &answers()).expect("first write");
    assert!(
        write_account(&dirs, &answers()).is_err(),
        "a second write must not overwrite silently"
    );
}

#[test]
fn write_account_file_creates_and_then_overwrites() {
    let root = tempfile::tempdir().unwrap();
    let dirs = dirs_at(root.path());
    write_account_file(&dirs, &answers(), None).expect("first write");
    let path = dirs.config.join("accounts").join("work.toml");
    assert!(path.exists());

    let mut changed = answers();
    changed.imap_host = "imap2.example.com".to_string();
    write_account_file(&dirs, &changed, Some("work"))
        .expect("overwrite");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("imap2.example.com"));
}

fn hand_written_toml() -> &'static str {
    "folder_order = [\"INBOX\", \"lists\"]\n\
     folders_hidden = [\"Junk\"]\n\
     \n\
     [account]\n\
     name = \"work\"\n\
     \n\
     [imap]\n\
     host = \"imap.example.com\"\n\
     user = \"quin@example.com\"\n\
     password_cmd = \"pass show mail/work\"  # rotate soon\n\
     \n\
     [smtp]\n\
     host = \"smtp.example.com\"\n\
     \n\
     [[identity]]\n\
     address = \"quin@example.com\"\n\
     match = [\"quin@example.com\"]\n\
     \n\
     [[rules]]\n\
     from = \"ci@example.com\"\n\
     move_to = \"ci\"\n\
     \n\
     [oauth]\n\
     provider = \"microsoft\"\n"
}

#[test]
fn an_edit_preserves_hand_written_config() {
    let root = tempfile::tempdir().unwrap();
    let dirs = dirs_at(root.path());
    let accounts = dirs.config.join("accounts");
    std::fs::create_dir_all(&accounts).unwrap();
    let path = accounts.join("work.toml");
    std::fs::write(&path, hand_written_toml()).unwrap();

    let mut changed = answers();
    changed.imap_host = "imap2.example.com".to_string();
    write_account_file(&dirs, &changed, Some("work"))
        .expect("edit in place");

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("host = \"imap2.example.com\""));
    assert!(
        text.contains("folder_order = [\"INBOX\", \"lists\"]"),
        "folder_order survives: {text}"
    );
    assert!(text.contains("folders_hidden = [\"Junk\"]"));
    assert!(text.contains("[[rules]]"), "rules survive: {text}");
    assert!(text.contains("from = \"ci@example.com\""));
    assert!(text.contains("move_to = \"ci\""));
    assert!(text.contains("[oauth]"), "oauth survives: {text}");
    assert!(text.contains("provider = \"microsoft\""));
    assert!(
        text.contains(
            "password_cmd = \"pass show mail/work\"  # rotate soon"
        ),
        "comments survive: {text}"
    );
}

/// The identity blocks are regenerated from the given list while
/// every other table and hand-written line survives.
#[test]
fn write_account_identities_replaces_the_blocks_in_place() {
    let root = tempfile::tempdir().unwrap();
    let accounts = root.path().join("accounts");
    std::fs::create_dir_all(&accounts).unwrap();
    let path = accounts.join("work.toml");
    std::fs::write(&path, hand_written_toml()).unwrap();

    let identities = [
        identity("quin@example.com", Some("Quin")),
        identity("extra@example.com", None),
    ];
    write_account_identities(&path, &identities).expect("write");

    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        text.matches("[[identity]]").count(),
        2,
        "both identities are written: {text}"
    );
    assert!(text.contains("address = \"extra@example.com\""));
    assert!(text.contains("name = \"Quin\""));
    assert!(text.contains("[[rules]]"), "rules survive: {text}");
    assert!(text.contains("provider = \"microsoft\""));
    assert!(text.contains("folder_order = [\"INBOX\", \"lists\"]"));
    assert!(
        text.contains("match = [\"quin@example.com\"]\n\n[[identity]]"),
        "the new blocks sit before the rules: {text}"
    );
}

/// Every identity field survives a write and reparse through
/// `antiphon_config::load`.
#[test]
fn identities_round_trip_through_config_load() {
    let root = tempfile::tempdir().unwrap();
    let dirs = dirs_at(root.path());
    write_account_file(&dirs, &answers(), None).expect("seed");
    let path = dirs.config.join("accounts").join("work.toml");

    let identities = [
        Identity {
            address: "quin@example.com".to_string(),
            name: Some("Quin".to_string()),
            signature: Some("~/.sig".to_string()),
            matches: vec!["quin@example.com".to_string()],
            pgp_sign: true,
            pgp_key: Some(FINGERPRINT.to_string()),
        },
        Identity {
            address: "side@example.com".to_string(),
            name: Some("Side".to_string()),
            signature: None,
            matches: vec![
                "side@example.com".to_string(),
                "alt@example.com".to_string(),
            ],
            pgp_sign: false,
            pgp_key: None,
        },
    ];
    write_account_identities(&path, &identities).expect("write");

    let loaded = antiphon_config::load(&dirs).expect("parse");
    let account = &loaded.accounts[0].account;
    assert_eq!(account.identities.len(), 2);

    let first = &account.identities[0];
    assert_eq!(first.address, "quin@example.com");
    assert_eq!(first.name.as_deref(), Some("Quin"));
    assert_eq!(first.signature.as_deref(), Some("~/.sig"));
    assert!(first.pgp_sign);
    assert_eq!(first.pgp_key.as_deref(), Some(FINGERPRINT));
    assert_eq!(first.matches, vec!["quin@example.com".to_string()]);

    let second = &account.identities[1];
    assert_eq!(second.address, "side@example.com");
    assert_eq!(second.name.as_deref(), Some("Side"));
    assert!(!second.pgp_sign);
    assert!(second.pgp_key.is_none());
    assert_eq!(
        second.matches,
        vec![
            "side@example.com".to_string(),
            "alt@example.com".to_string()
        ]
    );
}

/// A block signature is escaped into a TOML basic string on
/// write and comes back with its newlines intact on load.
#[test]
fn a_multi_line_signature_round_trips() {
    let root = tempfile::tempdir().unwrap();
    let dirs = dirs_at(root.path());
    write_account_file(&dirs, &answers(), None).expect("seed");
    let path = dirs.config.join("accounts").join("work.toml");

    let signature = "Quin\n--\nSent with \"Antiphon\"";
    let identities = [Identity {
        address: "quin@example.com".to_string(),
        name: Some("Quin".to_string()),
        signature: Some(signature.to_string()),
        matches: vec!["quin@example.com".to_string()],
        pgp_sign: false,
        pgp_key: None,
    }];
    write_account_identities(&path, &identities).expect("write");

    let loaded = antiphon_config::load(&dirs).expect("parse");
    let first = &loaded.accounts[0].account.identities[0];
    assert_eq!(first.signature.as_deref(), Some(signature));
}

#[test]
fn a_rename_carries_the_hand_written_content_across() {
    let root = tempfile::tempdir().unwrap();
    let dirs = dirs_at(root.path());
    let accounts = dirs.config.join("accounts");
    std::fs::create_dir_all(&accounts).unwrap();
    std::fs::write(accounts.join("work.toml"), hand_written_toml())
        .unwrap();

    let mut renamed = answers();
    renamed.name = "personal".to_string();
    write_account_file(&dirs, &renamed, Some("work")).expect("rename");

    assert!(!accounts.join("work.toml").exists());
    let text = std::fs::read_to_string(accounts.join("personal.toml"))
        .unwrap();
    assert!(text.contains("name = \"personal\""));
    assert!(text.contains("[[rules]]"));
    assert!(text.contains("folder_order = [\"INBOX\", \"lists\"]"));
}

#[test]
fn write_account_file_renames_when_the_name_changes() {
    let root = tempfile::tempdir().unwrap();
    let dirs = dirs_at(root.path());
    write_account_file(&dirs, &answers(), None).expect("first write");

    let mut renamed = answers();
    renamed.name = "personal".to_string();
    write_account_file(&dirs, &renamed, Some("work")).expect("rename");

    let accounts = dirs.config.join("accounts");
    assert!(!accounts.join("work.toml").exists());
    assert!(accounts.join("personal.toml").exists());
}

#[test]
fn remove_renamed_only_acts_when_the_name_actually_changed() {
    let root = tempfile::tempdir().unwrap();
    let accounts = root.path().join("accounts");
    std::fs::create_dir_all(&accounts).unwrap();
    std::fs::write(accounts.join("work.toml"), "x").unwrap();

    remove_renamed(&accounts, Some("work"), "work")
        .expect("unchanged name is a no-op");
    assert!(accounts.join("work.toml").exists());

    remove_renamed(&accounts, Some("work"), "personal")
        .expect("a changed name removes the old file");
    assert!(!accounts.join("work.toml").exists());

    remove_renamed(&accounts, None, "personal")
        .expect("no previous name is a no-op");
}
