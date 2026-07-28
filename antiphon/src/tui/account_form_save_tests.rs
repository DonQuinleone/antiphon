use super::super::account_form::AccountType;
use super::super::account_form::tests::{filled_answers, filled_form};
use super::super::testkit::TempDir;
use super::*;

fn dirs_at(root: &Path) -> Dirs {
    Dirs {
        config: root.to_path_buf(),
        state: root.join("state"),
        cache: root.join("cache"),
        data: root.join("data"),
    }
}

#[test]
fn a_blank_password_command_fails_validation_off_macos() {
    if cfg!(target_os = "macos") {
        return;
    }
    let mut form = filled_form();
    form.password_cmd = String::new();
    assert!(resolve_password_cmd(&form).is_err());
}

#[test]
fn an_oauth_account_needs_no_password_at_all() {
    let mut form = filled_form();
    form.password_cmd = String::new();
    form.account_type = AccountType::Google;
    assert_eq!(resolve_password_cmd(&form), Ok(String::new()));
}

#[test]
fn the_from_name_is_written_to_the_identity() {
    let root = TempDir::new();
    let dirs = dirs_at(&root.path);
    let mut form = filled_form();
    form.from_name = "Quin at Work".to_string();
    build_and_write(&dirs, &form).expect("save");

    let loaded = antiphon_config::load(&dirs).expect("parse");
    let identity = &loaded.accounts[0].account.identities[0];
    assert_eq!(identity.address, "quin@example.com");
    assert_eq!(identity.name.as_deref(), Some("Quin at Work"));
}

#[test]
fn saving_an_edit_overwrites_only_the_one_file() {
    let root = TempDir::new();
    let dirs = dirs_at(&root.path);
    account_file::write_account_file(&dirs, &filled_answers(), None)
        .expect("seed the account file");

    let mut form = filled_form();
    form.editing = Some("work".to_string());
    form.imap_host = "imap2.example.com".to_string();
    let name = build_and_write(&dirs, &form).expect("save");
    assert_eq!(name, "work");

    let text =
        std::fs::read_to_string(dirs.config.join("accounts/work.toml"))
            .unwrap();
    assert!(text.contains("imap2.example.com"));
}

#[test]
fn renaming_on_save_removes_the_old_file() {
    let root = TempDir::new();
    let dirs = dirs_at(&root.path);
    account_file::write_account_file(&dirs, &filled_answers(), None)
        .expect("seed the account file");

    let mut form = filled_form();
    form.editing = Some("work".to_string());
    form.name = "personal".to_string();
    build_and_write(&dirs, &form).expect("save");

    let accounts_dir = dirs.config.join("accounts");
    assert!(!accounts_dir.join("work.toml").exists());
    assert!(accounts_dir.join("personal.toml").exists());
}

#[test]
fn adding_over_an_existing_name_is_refused() {
    let root = TempDir::new();
    let dirs = dirs_at(&root.path);
    account_file::write_account_file(&dirs, &filled_answers(), None)
        .expect("seed the account file");

    let form = filled_form();
    assert!(build_and_write(&dirs, &form).is_err());
}

#[test]
fn a_google_add_writes_a_parseable_oauth_table() {
    let root = TempDir::new();
    let dirs = dirs_at(&root.path);
    let mut form = filled_form();
    form.password_cmd = String::new();
    form.account_type = AccountType::Google;
    form.client_id = "app-1".to_string();
    build_and_write(&dirs, &form).expect("save");

    let text =
        std::fs::read_to_string(dirs.config.join("accounts/work.toml"))
            .unwrap();
    assert!(text.contains("[oauth]"), "{text}");
    assert!(text.contains("provider = \"google\""));
    assert!(text.contains("client_id = \"app-1\""));
    assert!(!text.contains("password_cmd"), "{text}");
    assert!(text.contains("host = \"imap.gmail.com\""), "{text}");
    assert!(text.contains("host = \"smtp.gmail.com\""), "{text}");
    assert!(text.contains("user = \"quin@example.com\""), "{text}");

    let loaded = antiphon_config::load(&dirs).expect("parse");
    let account = &loaded.accounts[0].account;
    let oauth = account.oauth.as_ref().expect("oauth table");
    assert_eq!(oauth.provider, OauthProvider::Google);
    assert_eq!(oauth.client_id.as_deref(), Some("app-1"));
    assert_eq!(account.imap.host, "imap.gmail.com");
    assert_eq!(account.imap.user, "quin@example.com");
    assert_eq!(
        account.smtp.as_ref().map(|smtp| smtp.host.as_str()),
        Some("smtp.gmail.com")
    );
}

/// A Google OAuth add ignores whatever the hidden server
/// fields hold and writes the provider's fixed hosts.
#[test]
fn an_oauth_add_fills_the_provider_hosts_over_the_form() {
    let root = TempDir::new();
    let dirs = dirs_at(&root.path);
    let mut form = filled_form();
    form.password_cmd = String::new();
    form.account_type = AccountType::Microsoft;
    form.imap_host = "stale.example.com".to_string();
    form.smtp_host = "stale.example.com".to_string();
    build_and_write(&dirs, &form).expect("save");

    let loaded = antiphon_config::load(&dirs).expect("parse");
    let account = &loaded.accounts[0].account;
    assert_eq!(account.imap.host, "outlook.office365.com");
    assert_eq!(account.imap.user, "quin@example.com");
    assert_eq!(
        account.smtp.as_ref().map(|smtp| smtp.host.as_str()),
        Some("smtp.office365.com")
    );
}

#[test]
fn microsoft_graph_send_writes_the_graph_table() {
    let root = TempDir::new();
    let dirs = dirs_at(&root.path);
    let mut form = filled_form();
    form.account_type = AccountType::Microsoft;
    form.graph_send = true;
    form.tenant = "tenant-1".to_string();
    build_and_write(&dirs, &form).expect("save");

    let loaded = antiphon_config::load(&dirs).expect("parse");
    let account = &loaded.accounts[0].account;
    let graph = account.graph.as_ref().expect("graph table");
    assert!(graph.send);
    assert_eq!(graph.tenant.as_deref(), Some("tenant-1"));
    assert_eq!(graph.auth, GraphAuth::Delegated);
    assert!(account.oauth.is_some());
}

fn oauth_toml() -> &'static str {
    "[account]\n\
     name = \"work\"\n\
     \n\
     [imap]\n\
     host = \"imap.example.com\"\n\
     user = \"quin@example.com\"\n\
     \n\
     [smtp]\n\
     host = \"smtp.example.com\"\n\
     \n\
     [[identity]]\n\
     address = \"quin@example.com\"\n\
     \n\
     [oauth]\n\
     provider = \"microsoft\"\n\
     client_id = \"app-1\"\n\
     \n\
     [graph]\n\
     send = true\n\
     tenant = \"tenant-1\"\n\
     auth = \"app_only\"\n\
     secret_cmd = \"pass show graph\"  # rotate\n"
}

fn seeded(root: &TempDir) -> Dirs {
    let dirs = dirs_at(&root.path);
    let accounts = dirs.config.join("accounts");
    std::fs::create_dir_all(&accounts).unwrap();
    std::fs::write(accounts.join("work.toml"), oauth_toml()).unwrap();
    dirs
}

#[test]
fn choosing_none_drops_oauth_but_keeps_graph() {
    let root = TempDir::new();
    let dirs = seeded(&root);
    let mut form = filled_form();
    form.editing = Some("work".to_string());
    build_and_write(&dirs, &form).expect("save");

    let text =
        std::fs::read_to_string(dirs.config.join("accounts/work.toml"))
            .unwrap();
    assert!(!text.contains("[oauth]"), "{text}");
    assert!(!text.contains("provider ="), "{text}");
    assert!(text.contains("[graph]"), "graph survives: {text}");
    assert!(text.contains("secret_cmd = \"pass show graph\""));
}

/// An edit that re-affirms the inferred graph settings (as
/// opening the form does) keeps the app-only flow and the
/// secret command's hand-written comment.
#[test]
fn an_edit_keeps_the_graph_secret_command_and_comment() {
    let root = TempDir::new();
    let dirs = seeded(&root);
    let mut form = filled_form();
    form.editing = Some("work".to_string());
    form.password_cmd = String::new();
    form.account_type = AccountType::Microsoft;
    form.client_id = "app-2".to_string();
    form.graph_send = true;
    form.graph_auth = GraphAuth::AppOnly;
    form.graph_secret_cmd = "pass show graph".to_string();
    form.tenant = String::new();
    build_and_write(&dirs, &form).expect("save");

    let text =
        std::fs::read_to_string(dirs.config.join("accounts/work.toml"))
            .unwrap();
    assert!(text.contains("client_id = \"app-2\""));
    assert!(
        !text.contains("tenant ="),
        "an emptied tenant is removed: {text}"
    );
    assert!(text.contains("auth = \"app_only\""));
    assert!(
        text.contains("secret_cmd = \"pass show graph\"  # rotate"),
        "the command and its comment survive: {text}"
    );
}

/// Switching an app-only account to delegated drops the
/// now-unused secret command.
#[test]
fn delegated_drops_the_graph_secret_command() {
    let root = TempDir::new();
    let dirs = seeded(&root);
    let mut form = filled_form();
    form.editing = Some("work".to_string());
    form.password_cmd = String::new();
    form.account_type = AccountType::Microsoft;
    form.graph_send = true;
    form.graph_auth = GraphAuth::Delegated;
    build_and_write(&dirs, &form).expect("save");

    let text =
        std::fs::read_to_string(dirs.config.join("accounts/work.toml"))
            .unwrap();
    assert!(text.contains("auth = \"delegated\""), "{text}");
    assert!(!text.contains("secret_cmd"), "{text}");
}

#[test]
fn turning_graph_send_off_keeps_the_table() {
    let root = TempDir::new();
    let dirs = seeded(&root);
    let mut form = filled_form();
    form.editing = Some("work".to_string());
    form.account_type = AccountType::Microsoft;
    form.graph_send = false;
    build_and_write(&dirs, &form).expect("save");

    let text =
        std::fs::read_to_string(dirs.config.join("accounts/work.toml"))
            .unwrap();
    assert!(text.contains("send = false"), "{text}");
    assert!(text.contains("secret_cmd = \"pass show graph\""));
}

fn rich_imap_toml() -> &'static str {
    "folder_order = [\"INBOX\", \"lists\"]\n\
     folders_hidden = [\"Junk\"]\n\
     folders_unsynced = [\"Archive\"]\n\
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
     match_sender = \"ci@example.com\"\n\
     move_to = \"ci\"\n"
}

/// Changing an IMAP account to Microsoft 365 adds the
/// [oauth]/[graph] tables while every hand-written line
/// (rules, folder lists, comments) is left untouched.
#[test]
fn changing_type_preserves_hand_written_content() {
    let root = TempDir::new();
    let dirs = dirs_at(&root.path);
    let accounts = dirs.config.join("accounts");
    std::fs::create_dir_all(&accounts).unwrap();
    std::fs::write(accounts.join("work.toml"), rich_imap_toml())
        .unwrap();

    let mut form = filled_form();
    form.editing = Some("work".to_string());
    form.password_cmd = String::new();
    form.account_type = AccountType::Microsoft;
    form.client_id = "app-9".to_string();
    form.graph_send = true;
    form.graph_auth = GraphAuth::AppOnly;
    form.tenant = "tenant-9".to_string();
    form.graph_secret_cmd = "pass show graph".to_string();
    build_and_write(&dirs, &form).expect("save");

    let text =
        std::fs::read_to_string(accounts.join("work.toml")).unwrap();
    assert!(text.contains("[[rules]]"), "rules survive: {text}");
    assert!(text.contains("match_sender = \"ci@example.com\""));
    assert!(
        text.contains("folder_order = [\"INBOX\", \"lists\"]"),
        "{text}"
    );
    assert!(text.contains("folders_hidden = [\"Junk\"]"));
    assert!(text.contains("folders_unsynced = [\"Archive\"]"));
    assert!(
        text.contains(
            "password_cmd = \"pass show mail/work\"  # rotate soon"
        ),
        "comments survive: {text}"
    );
    assert!(text.contains("provider = \"microsoft\""));
    assert!(text.contains("client_id = \"app-9\""));
    assert!(text.contains("auth = \"app_only\""));
    assert!(text.contains("secret_cmd = \"pass show graph\""));

    let loaded = antiphon_config::load(&dirs).expect("parse");
    let account = &loaded.accounts[0].account;
    assert_eq!(
        account.oauth.as_ref().map(|oauth| oauth.provider),
        Some(OauthProvider::Microsoft)
    );
    assert_eq!(account.rules.len(), 1);
    assert_eq!(account.folders_unsynced, vec!["Archive".to_string()]);
}
