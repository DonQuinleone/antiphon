use super::super::testkit::TempDir;
use super::*;

fn with_theme(contents: &str, name: &str) -> String {
    with_key(contents, "ui", "theme", &format!("\"{name}\""))
}

#[test]
fn toml_string_array_quotes_and_joins() {
    assert_eq!(toml_string_array(&[]), "[]");
    assert_eq!(
        toml_string_array(&["a".to_string(), "b".to_string()]),
        "[\"a\", \"b\"]"
    );
}

#[test]
fn a_root_key_lands_before_the_first_table() {
    let before = "[account]\nname = \"work\"\n";
    let after = with_root_key(before, "folder_order", "[\"lists\"]");
    assert_eq!(
        after,
        "folder_order = [\"lists\"]\n\n\
         [account]\nname = \"work\"\n"
    );
    let updated = with_root_key(&after, "folder_order", "[\"spam\"]");
    assert!(updated.starts_with("folder_order = [\"spam\"]\n"));
    assert!(!updated.contains("lists"), "replaced in place: {updated}");
    assert_eq!(
        with_root_key("", "folders_hidden", "[]"),
        "folders_hidden = []\n"
    );
}

#[test]
fn a_root_key_inside_a_table_is_never_matched() {
    let before = "[account]\nname = \"work\"\n";
    let after = with_root_key(before, "name", "\"other\"");
    assert!(after.starts_with("name = \"other\"\n"));
    assert!(
        after.contains("name = \"work\""),
        "the table's own key survives: {after}"
    );
}

#[test]
fn an_existing_key_is_replaced_in_place() {
    let before = "[ui]\ntheme = \"vespers\"  # see docs\n\
                  list_rows = 7\n";
    let after = with_theme(before, "nord");
    assert_eq!(
        after,
        "[ui]\ntheme = \"nord\"  # see docs\n\
         list_rows = 7\n"
    );
}

#[test]
fn a_missing_key_is_inserted_under_the_header() {
    let before = "[ui]\nlist_rows = 7\n\n[sync]\nidle = false\n";
    let after = with_theme(before, "nord");
    assert_eq!(
        after,
        "[ui]\ntheme = \"nord\"\nlist_rows = 7\n\n\
         [sync]\nidle = false\n"
    );
}

#[test]
fn a_missing_table_is_appended() {
    let before = "[sync]\nidle = false\n";
    let after = with_theme(before, "nord");
    assert_eq!(
        after,
        "[sync]\nidle = false\n\n[ui]\ntheme = \"nord\"\n"
    );
}

#[test]
fn an_empty_document_gets_a_fresh_table() {
    let after = with_theme("", "nord");
    assert_eq!(after, "[ui]\ntheme = \"nord\"\n");
}

#[test]
fn a_key_needing_quotes_is_written_and_found_quoted() {
    let before = "[folder_names]\nother = \"x\"\n";
    let after =
        with_key(before, "folder_names", "lists/aerc", "\"aerc\"");
    assert_eq!(
        after,
        "[folder_names]\n\"lists/aerc\" = \"aerc\"\nother = \"x\"\n"
    );
    let updated =
        with_key(&after, "folder_names", "lists/aerc", "\"list\"");
    assert!(updated.contains("\"lists/aerc\" = \"list\""));
    assert!(!updated.contains("\"aerc\"\n\"lists/aerc\""));
}

#[test]
fn a_hand_written_bare_key_is_still_found_and_replaced() {
    let before = "[folder_names]\narchive = \"Archive\"\n";
    let after = with_key(before, "folder_names", "archive", "\"Old\"");
    assert_eq!(after, "[folder_names]\narchive = \"Old\"\n");
}

#[test]
fn without_key_drops_only_the_named_entry() {
    let before = "[folder_names]\n\"lists/aerc\" = \"aerc\"\n\
                  archive = \"Archive\"\n";
    let after = without_key(before, "folder_names", "lists/aerc");
    assert_eq!(after, "[folder_names]\narchive = \"Archive\"\n");
}

#[test]
fn without_key_is_a_no_op_when_nothing_matches() {
    let before = "[folder_names]\narchive = \"Archive\"\n";
    assert_eq!(without_key(before, "folder_names", "missing"), before);
    assert_eq!(without_key(before, "elsewhere", "archive"), before);
    assert_eq!(without_key("", "folder_names", "archive"), "");
}

#[test]
fn an_array_table_key_is_edited_in_the_first_entry_only() {
    let before = "[[identity]]\naddress = \"a@example.com\"\n\n\
                  [[identity]]\naddress = \"b@example.com\"\n";
    let after = with_array_key(
        before,
        "identity",
        "address",
        "\"c@example.com\"",
    );
    assert!(after.contains("address = \"c@example.com\""));
    assert!(after.contains("address = \"b@example.com\""));
    assert!(!after.contains("a@example.com"));
}

#[test]
fn a_missing_array_table_gets_a_fresh_entry() {
    let before = "[account]\nname = \"work\"\n";
    let after = with_array_key(
        before,
        "identity",
        "address",
        "\"a@example.com\"",
    );
    assert_eq!(
        after,
        "[account]\nname = \"work\"\n\n\
         [[identity]]\naddress = \"a@example.com\"\n"
    );
}

#[test]
fn array_key_value_reads_the_first_entry_or_nothing() {
    let text = "[[identity]]\naddress = \"a@example.com\"\n\
                match = [\"a@example.com\", \"b@example.com\"]\n";
    assert_eq!(
        array_key_value(text, "identity", "address").as_deref(),
        Some("\"a@example.com\"")
    );
    assert_eq!(
        array_key_value(text, "identity", "match").as_deref(),
        Some("[\"a@example.com\", \"b@example.com\"]")
    );
    assert_eq!(array_key_value(text, "identity", "name"), None);
    assert_eq!(array_key_value(text, "rules", "from"), None);
}

#[test]
fn remove_key_on_a_missing_file_is_a_no_op() {
    let dir = TempDir::new();
    let path = dir.path.join("missing.toml");
    remove_key(&path, "folder_names", "archive")
        .expect("a missing file is not an error");
    assert!(!path.exists());
}

#[test]
fn remove_key_deletes_the_line_from_a_real_file() {
    let dir = TempDir::new();
    let path = dir.path.join("account.toml");
    std::fs::write(
        &path,
        "[folder_names]\n\"lists/aerc\" = \"aerc\"\n",
    )
    .unwrap();
    remove_key(&path, "folder_names", "lists/aerc")
        .expect("remove an existing key");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "[folder_names]\n"
    );
}

/// Every essentials key that `settingscmd` writes goes
/// through the very same generic edit, over the four
/// shapes a config file can be in: key present, key
/// absent, table absent, file absent.
#[test]
fn every_essentials_key_supports_all_four_edit_cases() {
    let cases: &[(&str, &str, &str)] = &[
        ("ui", "theme", "\"nord\""),
        ("sync", "interval_minutes", "5"),
        ("sync", "idle", "true"),
        ("ui", "reading_pane", "\"right\""),
        ("ui", "list_rows", "12"),
        ("ui", "sidebar_width", "20"),
    ];
    for (table, key, value) in cases {
        let want = key_line(key, value);

        let present = format!("[{table}]\n{key} = old\nother = 1\n");
        let after = with_key(&present, table, key, value);
        assert!(after.contains(&want), "{table}.{key} present");
        assert!(
            after.contains("other = 1"),
            "{table}.{key} keeps siblings"
        );

        let absent = format!("[{table}]\nother = 1\n");
        let after = with_key(&absent, table, key, value);
        assert!(after.contains(&want), "{table}.{key} key absent");

        let no_table = "[elsewhere]\nx = 1\n";
        let after = with_key(no_table, table, key, value);
        assert!(
            after.contains(&format!("[{table}]")),
            "{table}.{key} table absent"
        );
        assert!(after.contains(&want), "{table}.{key} table absent");

        let dir = TempDir::new();
        let path = dir.path.join("config.toml");
        persist_key(&path, table, key, value)
            .expect("persist into a missing file");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(&want), "{table}.{key} file absent");
    }
}
