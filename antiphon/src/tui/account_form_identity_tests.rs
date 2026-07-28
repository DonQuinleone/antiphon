use super::*;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn form_with_identity() -> AccountFormState {
    AccountFormState {
        address: "quin@example.com".to_string(),
        identities: vec![FormIdentity::seed(
            "Quin",
            "quin@example.com",
        )],
        identity_ui: Some(IdentityUi::List { selected: 0 }),
        ..AccountFormState::default()
    }
}

#[test]
fn adding_appends_an_identity_defaulting_its_address() {
    let mut form = form_with_identity();
    list_key(&mut form, KeyCode::Char('a'));
    assert!(matches!(form.identity_ui, Some(IdentityUi::Edit(_))));
    editor_key(&mut form, key(KeyCode::Char('X')));
    commit(&mut form);
    assert_eq!(form.identities.len(), 2);
    assert_eq!(form.identities[1].from_name, "X");
    assert_eq!(form.identities[1].address, "quin@example.com");
    assert!(matches!(
        form.identity_ui,
        Some(IdentityUi::List { selected: 1 })
    ));
}

#[test]
fn editing_updates_the_selected_identity_in_place() {
    let mut form = form_with_identity();
    list_key(&mut form, KeyCode::Char('e'));
    for ch in " Two".chars() {
        editor_key(&mut form, key(KeyCode::Char(ch)));
    }
    commit(&mut form);
    assert_eq!(form.identities.len(), 1);
    assert_eq!(form.identities[0].from_name, "Quin Two");
}

#[test]
fn removing_drops_the_selected_identity() {
    let mut form = form_with_identity();
    form.identities
        .push(FormIdentity::seed("Side", "side@example.com"));
    list_key(&mut form, KeyCode::Down);
    list_key(&mut form, KeyCode::Char('d'));
    assert_eq!(form.identities.len(), 1);
    assert_eq!(form.identities[0].from_name, "Quin");
}

#[test]
fn the_last_identity_cannot_be_removed() {
    let mut form = form_with_identity();
    list_key(&mut form, KeyCode::Char('d'));
    assert_eq!(form.identities.len(), 1);
}

#[test]
fn the_auto_sign_toggle_flips_with_space() {
    let mut form = form_with_identity();
    list_key(&mut form, KeyCode::Char('e'));
    for _ in 0..3 {
        editor_key(&mut form, key(KeyCode::Tab));
    }
    editor_key(&mut form, key(KeyCode::Char(' ')));
    commit(&mut form);
    assert!(form.identities[0].pgp_sign);
}

#[test]
fn esc_backs_out_of_the_editor_then_the_list() {
    let mut form = form_with_identity();
    list_key(&mut form, KeyCode::Char('e'));
    editor_key(&mut form, key(KeyCode::Esc));
    assert!(matches!(form.identity_ui, Some(IdentityUi::List { .. })));
    list_key(&mut form, KeyCode::Esc);
    assert!(form.identity_ui.is_none());
}

#[test]
fn to_config_parses_comma_separated_matches() {
    let mut identity = FormIdentity::seed("Quin", "quin@example.com");
    identity.matches = "a@example.com, b@example.com".to_string();
    let config = identity.to_config("acct@example.com");
    assert_eq!(config.address, "quin@example.com");
    assert_eq!(
        config.matches,
        vec!["a@example.com".to_string(), "b@example.com".to_string()]
    );
}

#[test]
fn to_config_defaults_an_empty_address_and_match() {
    let config = FormIdentity::default().to_config("acct@example.com");
    assert_eq!(config.address, "acct@example.com");
    assert_eq!(config.matches, vec!["acct@example.com".to_string()]);
    assert!(config.name.is_none());
    assert!(!config.pgp_sign);
}

#[test]
fn from_config_round_trips_every_field() {
    let config = antiphon_config::Identity {
        address: "quin@example.com".to_string(),
        name: Some("Quin".to_string()),
        signature: Some("~/.sig".to_string()),
        matches: vec![
            "a@example.com".to_string(),
            "b@example.com".to_string(),
        ],
        pgp_sign: true,
        pgp_key: Some("0xAB".to_string()),
    };
    let form = FormIdentity::from_config(&config);
    assert_eq!(form.from_name, "Quin");
    assert_eq!(form.signature, "~/.sig");
    assert_eq!(form.matches, "a@example.com, b@example.com");
    assert!(form.pgp_sign);
    assert_eq!(form.pgp_key, "0xAB");
}
