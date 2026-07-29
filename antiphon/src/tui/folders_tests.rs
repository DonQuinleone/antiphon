use super::super::testkit::TempDir;
use super::*;

fn account(name: &str, folders: &[&str]) -> AccountEntry {
    AccountEntry {
        name: name.to_string(),
        folders: folders
            .iter()
            .map(|folder| (*folder).to_string())
            .collect(),
        ..AccountEntry::default()
    }
}

fn row(account: &str, folder: &str, alias: &str) -> FolderRow {
    FolderRow {
        account: account.to_string(),
        folder: folder.to_string(),
        alias: alias.to_string(),
        hidden: false,
        unsynced: false,
    }
}

#[test]
fn rows_join_the_alias_by_account_and_folder() {
    let accounts = vec![
        account("work", &["archive", "lists/aerc"]),
        account("personal", &["archive"]),
    ];
    let aliases = vec![(
        "work".to_string(),
        "lists/aerc".to_string(),
        "aerc-list".to_string(),
    )];
    let built = rows(&accounts, &aliases);
    assert_eq!(
        built,
        vec![
            row("work", "inbox", ""),
            row("work", "archive", ""),
            row("work", "lists/aerc", "aerc-list"),
            row("personal", "inbox", ""),
            row("personal", "archive", ""),
        ]
    );
}

#[test]
fn rows_follow_the_order_and_keep_hidden_folders() {
    let mut work = account("work", &["archive", "spam"]);
    work.order = vec!["spam".to_string()];
    work.hidden = vec!["spam".to_string()];
    let built = rows(&[work], &[]);
    let summary: Vec<(&str, bool)> = built
        .iter()
        .map(|row| (row.folder.as_str(), row.hidden))
        .collect();
    assert_eq!(
        summary,
        [("spam", true), ("inbox", false), ("archive", false)],
    );
}

#[test]
fn an_alias_with_no_matching_folder_is_never_joined_in() {
    let accounts = vec![account("work", &["archive"])];
    let aliases = vec![(
        "work".to_string(),
        "gone".to_string(),
        "stale".to_string(),
    )];
    let built = rows(&accounts, &aliases);
    assert!(built.iter().all(|row| row.alias.is_empty()));
}

fn app_on_folders_tab(dir: &TempDir) -> App {
    use crate::tui::settings::{SettingsState, SettingsTab};

    let mut app = super::super::testkit::app_with_folders(&[(
        "work",
        &["archive", "lists/aerc", "spam"][..],
    )]);
    app.dirs.config = dir.path.clone();
    let accounts_dir = dir.path.join("accounts");
    std::fs::create_dir_all(&accounts_dir).unwrap();
    std::fs::write(
        accounts_dir.join("work.toml"),
        "[account]\nname = \"work\"\n",
    )
    .unwrap();
    app.settings = Some(SettingsState {
        tab: SettingsTab::Folders,
        accounts: Vec::new(),
        account_selected: 0,
        pending_delete: None,
        pending_revoke: None,
        essentials_selected: 0,
        daemon_hint: None,
        folders: app.folder_rows(),
        folder_selected: 0,
    });
    app
}

fn folder_names(app: &App) -> Vec<String> {
    app.settings
        .as_ref()
        .unwrap()
        .folders
        .iter()
        .map(|row| row.folder.clone())
        .collect()
}

#[test]
fn shift_j_reorders_and_persists_the_whole_account_order() {
    let dir = TempDir::new();
    let mut app = app_on_folders_tab(&dir);
    assert_eq!(
        folder_names(&app),
        ["inbox", "archive", "lists/aerc", "spam"]
    );

    apply(&mut app, Action::ReorderDown);
    assert_eq!(
        folder_names(&app),
        ["archive", "inbox", "lists/aerc", "spam"]
    );
    assert_eq!(
        app.settings.as_ref().unwrap().folder_selected,
        1,
        "the selection follows the moved row"
    );
    let text =
        std::fs::read_to_string(dir.path.join("accounts/work.toml"))
            .unwrap();
    assert!(
        text.starts_with(
            "folder_order = [\"archive\", \"inbox\", \
             \"lists/aerc\", \"spam\"]\n"
        ),
        "{text}"
    );
    assert!(text.contains("[account]"), "the rest survives");

    apply(&mut app, Action::ReorderUp);
    assert_eq!(
        folder_names(&app),
        ["inbox", "archive", "lists/aerc", "spam"]
    );
    apply(&mut app, Action::ReorderUp);
    assert_eq!(
        folder_names(&app),
        ["inbox", "archive", "lists/aerc", "spam"],
        "moving up from the top clamps"
    );
}

#[test]
fn h_hides_a_folder_from_the_sidebar_and_back() {
    let dir = TempDir::new();
    let mut app = app_on_folders_tab(&dir);
    app.settings.as_mut().unwrap().folder_selected = 3;

    apply(&mut app, Action::FolderHide);
    let state = app.settings.as_ref().unwrap();
    assert!(state.folders[3].hidden, "the row stays, marked");
    assert!(
        !app.sidebar_entries
            .iter()
            .any(|entry| entry.label() == "spam"),
        "the sidebar drops it"
    );
    let text =
        std::fs::read_to_string(dir.path.join("accounts/work.toml"))
            .unwrap();
    assert!(text.contains("folders_hidden = [\"spam\"]"), "{text}");

    apply(&mut app, Action::FolderHide);
    assert!(!app.settings.as_ref().unwrap().folders[3].hidden);
    assert!(
        app.sidebar_entries
            .iter()
            .any(|entry| entry.label() == "spam"),
    );
    let text =
        std::fs::read_to_string(dir.path.join("accounts/work.toml"))
            .unwrap();
    assert!(text.contains("folders_hidden = []"), "{text}");
}

#[test]
fn u_unsyncs_a_folder_and_drops_it_from_the_sidebar() {
    let dir = TempDir::new();
    let mut app = app_on_folders_tab(&dir);
    app.settings.as_mut().unwrap().folder_selected = 3;

    apply(&mut app, Action::FolderUnsync);
    let state = app.settings.as_ref().unwrap();
    assert!(state.folders[3].unsynced, "the row stays, marked");
    assert!(
        !app.sidebar_entries
            .iter()
            .any(|entry| entry.label() == "spam"),
        "an unsynced folder leaves the sidebar"
    );
    let text =
        std::fs::read_to_string(dir.path.join("accounts/work.toml"))
            .unwrap();
    assert!(text.contains("folders_unsynced = [\"spam\"]"), "{text}");

    apply(&mut app, Action::FolderUnsync);
    assert!(!app.settings.as_ref().unwrap().folders[3].unsynced);
    let text =
        std::fs::read_to_string(dir.path.join("accounts/work.toml"))
            .unwrap();
    assert!(text.contains("folders_unsynced = []"), "{text}");
}

#[test]
fn the_inbox_can_never_be_unsynced() {
    let dir = TempDir::new();
    let mut app = app_on_folders_tab(&dir);
    app.settings.as_mut().unwrap().folder_selected = 0;
    assert_eq!(folder_names(&app)[0], "inbox");

    apply(&mut app, Action::FolderUnsync);
    assert!(!app.settings.as_ref().unwrap().folders[0].unsynced);
    assert_eq!(
        app.notice.as_deref(),
        Some("the inbox is always synced")
    );
    assert!(
        !dir.path.join("accounts/work.toml").exists()
            || !std::fs::read_to_string(
                dir.path.join("accounts/work.toml")
            )
            .unwrap()
            .contains("folders_unsynced"),
        "nothing is persisted for the inbox"
    );
}

#[test]
fn unsync_and_hide_are_independent_per_row() {
    let dir = TempDir::new();
    let mut app = app_on_folders_tab(&dir);
    app.settings.as_mut().unwrap().folder_selected = 3;

    apply(&mut app, Action::FolderHide);
    apply(&mut app, Action::FolderUnsync);
    let state = app.settings.as_ref().unwrap();
    assert!(state.folders[3].hidden, "hide stays set");
    assert!(state.folders[3].unsynced, "unsync stacks on top");
    let text =
        std::fs::read_to_string(dir.path.join("accounts/work.toml"))
            .unwrap();
    assert!(text.contains("folders_hidden = [\"spam\"]"), "{text}");
    assert!(text.contains("folders_unsynced = [\"spam\"]"), "{text}");
}
