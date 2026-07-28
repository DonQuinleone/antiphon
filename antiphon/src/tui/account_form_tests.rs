use super::*;

pub(in super::super) fn filled_answers() -> AccountAnswers {
    AccountAnswers {
        name: "work".to_string(),
        address: "quin@example.com".to_string(),
        imap_host: "imap.example.com".to_string(),
        imap_user: "quin@example.com".to_string(),
        smtp_host: "smtp.example.com".to_string(),
        password_cmd: "pass show mail/work".to_string(),
    }
}

pub(in super::super) fn filled_form() -> AccountFormState {
    AccountFormState::from_answers(&filled_answers(), None)
}

fn labels(form: &AccountFormState) -> Vec<&'static str> {
    (0..form.field_count())
        .map(|index| form.field_label(index))
        .collect()
}

fn type_row(form: &AccountFormState) -> usize {
    labels(form)
        .iter()
        .position(|label| *label == "account type")
        .expect("type row")
}

#[test]
fn the_type_toggle_leads_the_form() {
    let form = filled_form();
    assert_eq!(form.field_id(0), Field::AccountType);
    assert_eq!(form.field_value(0), "IMAP");
    assert_eq!(form.field_segments(0), Some(&TYPE_OPTIONS as &[&str]));
}

#[test]
fn the_field_table_round_trips_every_answer() {
    let answers = filled_answers();
    let form = filled_form();
    for index in 0..form.field_count() {
        let value = form.field_value(index);
        let belongs = [
            answers.name.as_str(),
            answers.address.as_str(),
            answers.imap_host.as_str(),
            answers.imap_user.as_str(),
            answers.smtp_host.as_str(),
            answers.password_cmd.as_str(),
            "IMAP",
            "",
        ]
        .contains(&value);
        assert!(belongs, "{index}: {value:?}");
    }
}

#[test]
fn prefill_infers_imap_and_focuses_the_type_toggle() {
    let answers = filled_answers();
    let form = AccountFormState::from_answers(
        &answers,
        Some("work".to_string()),
    );
    assert_eq!(form.editing.as_deref(), Some("work"));
    assert_eq!(form.focus, 0);
    assert_eq!(form.field_id(0), Field::AccountType);
    assert_eq!(form.address, answers.address);
    assert_eq!(form.password_cmd, answers.password_cmd);
    assert!(form.keychain_secret.is_empty());
    assert_eq!(form.account_type, AccountType::Imap);
}

#[test]
fn the_type_drives_which_fields_show() {
    let mut form = filled_form();
    assert!(labels(&form).contains(&"password command"));
    assert!(labels(&form).contains(&"imap host"));
    assert!(labels(&form).contains(&"imap user"));
    assert!(labels(&form).contains(&"smtp host"));
    assert!(!labels(&form).contains(&"oauth client id"));

    form.account_type = AccountType::Google;
    let shown = labels(&form);
    assert!(!shown.contains(&"password command"));
    assert!(
        !shown.contains(&"imap host"),
        "an OAuth account has fixed servers, so hides them"
    );
    assert!(!shown.contains(&"imap user"));
    assert!(!shown.contains(&"smtp host"));
    assert!(shown.contains(&"oauth client id"));
    assert!(!shown.iter().any(|label| label.contains("graph")));

    form.account_type = AccountType::Microsoft;
    let shown = labels(&form);
    assert!(shown.contains(&"graph send"));
    assert!(
        !shown.contains(&"graph tenant"),
        "the graph rows hide until graph send is on"
    );
    assert!(!shown.contains(&"graph auth"));

    form.graph_send = true;
    let shown = labels(&form);
    assert!(shown.contains(&"graph tenant"));
    assert!(shown.contains(&"graph auth"));
    assert!(
        !shown.contains(&"graph secret command"),
        "the secret command is app-only"
    );

    form.graph_auth = GraphAuth::AppOnly;
    assert!(labels(&form).contains(&"graph secret command"));
}

fn cycle_at_focus(form: &mut AccountFormState, code: KeyCode) {
    if let Access::Cycle(cycle) = form.spec(form.focus).access {
        cycle_key(form, code, cycle);
    }
}

#[test]
fn left_right_and_space_cycle_the_type_row() {
    let mut form = filled_form();
    form.focus = type_row(&form);
    cycle_at_focus(&mut form, KeyCode::Right);
    assert_eq!(form.account_type, AccountType::Microsoft);
    cycle_at_focus(&mut form, KeyCode::Char(' '));
    assert_eq!(form.account_type, AccountType::Google);
    cycle_at_focus(&mut form, KeyCode::Right);
    assert_eq!(form.account_type, AccountType::Imap);
    cycle_at_focus(&mut form, KeyCode::Left);
    assert_eq!(form.account_type, AccountType::Google);
}

#[test]
fn the_provider_follows_the_type() {
    let mut form = filled_form();
    assert_eq!(form.provider(), None);
    form.account_type = AccountType::Microsoft;
    assert_eq!(form.provider(), Some(OauthProvider::Microsoft));
    form.account_type = AccountType::Google;
    assert_eq!(form.provider(), Some(OauthProvider::Google));
}

#[test]
fn typing_on_a_cycle_row_changes_nothing() {
    let mut form = filled_form();
    let row = type_row(&form);
    form.focus = row;
    assert!(form.field_mut().is_none());
    assert_eq!(form.field_value(row), "IMAP");
}

#[test]
fn infer_type_reads_the_oauth_and_graph_tables() {
    use antiphon_config::{Graph, Oauth};

    let mut form = filled_form();
    let mut account = minimal_account();
    account.oauth = Some(Oauth {
        provider: OauthProvider::Microsoft,
        client_id: Some("app-1".to_string()),
    });
    account.graph = Some(Graph {
        send: true,
        tenant: Some("tenant-1".to_string()),
        client_id: None,
        auth: GraphAuth::AppOnly,
        secret_cmd: Some("pass show graph".to_string()),
    });
    form.infer_type(&account);
    assert_eq!(form.account_type, AccountType::Microsoft);
    assert_eq!(form.client_id, "app-1");
    assert!(form.graph_send);
    assert_eq!(form.graph_auth, GraphAuth::AppOnly);
    assert_eq!(form.tenant, "tenant-1");
    assert_eq!(form.graph_secret_cmd, "pass show graph");
}

#[test]
fn infer_type_reads_a_google_account() {
    use antiphon_config::Oauth;

    let mut form = filled_form();
    let mut account = minimal_account();
    account.oauth = Some(Oauth {
        provider: OauthProvider::Google,
        client_id: None,
    });
    form.infer_type(&account);
    assert_eq!(form.account_type, AccountType::Google);
    assert!(form.client_id.is_empty());
}

pub(in super::super) fn minimal_account() -> antiphon_config::AccountFile
{
    use antiphon_config::{Account, Imap};

    antiphon_config::AccountFile {
        account: Account {
            name: "work".to_string(),
            maildir: None,
            archive: None,
            trash: None,
        },
        imap: Imap {
            host: "imap.example.com".to_string(),
            port: None,
            user: "quin".to_string(),
            password_cmd: None,
        },
        smtp: None,
        identities: Vec::new(),
        rules: Vec::new(),
        oauth: None,
        graph: None,
        folder_names: Default::default(),
        folder_order: Vec::new(),
        folders_hidden: Vec::new(),
        folders_unsynced: Vec::new(),
    }
}

#[test]
fn esc_closes_the_form_without_writing_anything() {
    use ratatui::crossterm::event::KeyModifiers as Mods;

    let mut app = super::super::testkit::app_with_messages(1);
    app.open_account_form_add();
    feed(&mut app, KeyEvent::new(KeyCode::Esc, Mods::NONE));
    assert!(app.account_form.is_none());
}
