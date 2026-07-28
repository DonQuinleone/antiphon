use antiphon_config::OauthProvider;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::app::App;
use super::headers::byte_index;
use super::settings::wrapped;
use crate::account_wizard::AccountAnswers;

/// Every field the form can show, in display order; which of
/// them are actually visible follows the provider choice (see
/// `AccountFormState::shows`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Field {
    Address,
    Name,
    ImapHost,
    ImapUser,
    SmtpHost,
    Provider,
    ClientId,
    GraphSend,
    Tenant,
    PasswordCmd,
    KeychainSecret,
}

/// How a field reacts to keys: text fields edit a string in
/// place, cycling fields step through fixed choices with
/// left/right.
pub(super) enum Access {
    Edit(fn(&mut AccountFormState) -> &mut String),
    Cycle(fn(&mut AccountFormState, i32)),
}

pub(super) struct FieldSpec {
    pub(super) field: Field,
    label: &'static str,
    masked: bool,
    get: fn(&AccountFormState) -> &str,
    access: Access,
}

macro_rules! field {
    ($field:expr, $label:literal, $name:ident) => {
        FieldSpec {
            field: $field,
            label: $label,
            masked: false,
            get: |state| &state.$name,
            access: Access::Edit(|state| &mut state.$name),
        }
    };
}

const FIELDS: &[FieldSpec] = &[
    field!(Field::Address, "e-mail address", address),
    field!(Field::Name, "account name", name),
    field!(Field::ImapHost, "imap host", imap_host),
    field!(Field::ImapUser, "imap user", imap_user),
    field!(Field::SmtpHost, "smtp host", smtp_host),
    FieldSpec {
        field: Field::Provider,
        label: "oauth provider (\u{2190}/\u{2192})",
        masked: false,
        get: |state| provider_name(state.provider),
        access: Access::Cycle(cycle_provider),
    },
    field!(Field::ClientId, "oauth client id", client_id),
    FieldSpec {
        field: Field::GraphSend,
        label: "graph send (\u{2190}/\u{2192})",
        masked: false,
        get: |state| on_off(state.graph_send),
        access: Access::Cycle(|state, _| {
            state.graph_send = !state.graph_send
        }),
    },
    field!(Field::Tenant, "graph tenant", tenant),
    field!(Field::PasswordCmd, "password command", password_cmd),
    FieldSpec {
        field: Field::KeychainSecret,
        label: "password (stored in Keychain)",
        masked: true,
        get: |state| &state.keychain_secret,
        access: Access::Edit(|state| &mut state.keychain_secret),
    },
];

/// The provider cycle's fixed order under left/right.
const PROVIDERS: [Option<OauthProvider>; 3] = [
    None,
    Some(OauthProvider::Google),
    Some(OauthProvider::Microsoft),
];

pub(super) fn provider_name(
    provider: Option<OauthProvider>,
) -> &'static str {
    match provider {
        None => "none",
        Some(OauthProvider::Google) => "google",
        Some(OauthProvider::Microsoft) => "microsoft",
    }
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn cycle_provider(state: &mut AccountFormState, step: i32) {
    let current = PROVIDERS
        .iter()
        .position(|choice| *choice == state.provider)
        .unwrap_or(0);
    let next = wrapped(current, PROVIDERS.len(), step);
    state.provider = PROVIDERS[next];
}

pub(super) const PASSWORD_HINT: &str =
    "empty = use the Keychain field below";

/// The in-TUI replacement for the setup wizard's terminal Q&A:
/// one field per row, `editing` naming the account file an
/// edit overwrites (and possibly renames) rather than an add.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct AccountFormState {
    pub(super) address: String,
    pub(super) name: String,
    pub(super) imap_host: String,
    pub(super) imap_user: String,
    pub(super) smtp_host: String,
    pub(super) password_cmd: String,
    pub(super) keychain_secret: String,
    pub(super) provider: Option<OauthProvider>,
    pub(super) client_id: String,
    pub(super) graph_send: bool,
    pub(super) tenant: String,
    pub(super) focus: usize,
    pub(super) cursor: usize,
    pub(super) editing: Option<String>,
    pub(super) error: Option<String>,
}

impl AccountFormState {
    fn from_answers(
        answers: &AccountAnswers,
        editing: Option<String>,
    ) -> AccountFormState {
        AccountFormState {
            address: answers.address.clone(),
            name: answers.name.clone(),
            imap_host: answers.imap_host.clone(),
            imap_user: answers.imap_user.clone(),
            smtp_host: answers.smtp_host.clone(),
            password_cmd: answers.password_cmd.clone(),
            cursor: answers.address.chars().count(),
            editing,
            ..AccountFormState::default()
        }
    }

    fn oauth_prefill(
        &mut self,
        account: &antiphon_config::AccountFile,
    ) {
        if let Some(oauth) = &account.oauth {
            self.provider = Some(oauth.provider);
            self.client_id =
                oauth.client_id.clone().unwrap_or_default();
        }
        if let Some(graph) = &account.graph {
            self.graph_send = graph.send;
            self.tenant = graph.tenant.clone().unwrap_or_default();
        }
    }

    /// Password fields make way for the provider's own fields
    /// once one is chosen; the Keychain field is macOS only.
    fn shows(&self, field: Field) -> bool {
        match field {
            Field::ClientId => self.provider.is_some(),
            Field::GraphSend => {
                self.provider == Some(OauthProvider::Microsoft)
            }
            Field::Tenant => {
                self.provider == Some(OauthProvider::Microsoft)
                    && self.graph_send
            }
            Field::PasswordCmd => self.provider.is_none(),
            Field::KeychainSecret => {
                self.provider.is_none() && cfg!(target_os = "macos")
            }
            _ => true,
        }
    }

    fn visible(&self) -> impl Iterator<Item = &'static FieldSpec> {
        FIELDS.iter().filter(|spec| self.shows(spec.field))
    }

    fn spec(&self, index: usize) -> &'static FieldSpec {
        self.visible().nth(index).expect("field index in range")
    }

    pub(super) fn field_count(&self) -> usize {
        self.visible().count()
    }

    pub(super) fn field_id(&self, index: usize) -> Field {
        self.spec(index).field
    }

    pub(super) fn field_label(&self, index: usize) -> &'static str {
        self.spec(index).label
    }

    pub(super) fn field_value(&self, index: usize) -> &str {
        (self.spec(index).get)(self)
    }

    pub(super) fn field_masked(&self, index: usize) -> bool {
        self.spec(index).masked
    }

    fn field(&self) -> &str {
        (self.spec(self.focus).get)(self)
    }

    fn field_mut(&mut self) -> Option<&mut String> {
        match self.spec(self.focus).access {
            Access::Edit(get_mut) => Some(get_mut(self)),
            Access::Cycle(_) => None,
        }
    }
}

impl App {
    pub(super) fn open_account_form_add(&mut self) {
        self.account_form = Some(AccountFormState::default());
    }

    pub(super) fn open_account_form_edit(&mut self, file_stem: &str) {
        let Ok(loaded) = antiphon_config::load(&self.dirs) else {
            self.notice =
                Some(format!("account {file_stem}: could not load"));
            return;
        };
        let Some(named) = loaded
            .accounts
            .iter()
            .find(|entry| entry.file_stem == file_stem)
        else {
            self.notice =
                Some(format!("account {file_stem} not found"));
            return;
        };
        let answers = AccountAnswers::from_existing(&named.account);
        let mut form = AccountFormState::from_answers(
            &answers,
            Some(file_stem.to_string()),
        );
        form.oauth_prefill(&named.account);
        self.account_form = Some(form);
    }
}

/// Keys while the modal is open; everything but esc, tab
/// stepping and ctrl-s edits the focused field in place.
pub(super) fn feed(app: &mut App, key: KeyEvent) {
    if app.account_form.is_none() {
        return;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        if key.code == KeyCode::Char('s') {
            super::account_form_save::save(app);
        }
        return;
    }
    match key.code {
        KeyCode::Esc => app.account_form = None,
        KeyCode::Tab | KeyCode::Down => step_focus(app, 1),
        KeyCode::BackTab | KeyCode::Up => step_focus(app, -1),
        KeyCode::Enter => enter(app),
        other => field_key(app, other),
    }
}

fn enter(app: &mut App) {
    let Some(form) = app.account_form.as_ref() else {
        return;
    };
    if form.focus == form.field_count() - 1 {
        super::account_form_save::save(app);
    } else {
        step_focus(app, 1);
    }
}

fn step_focus(app: &mut App, step: i32) {
    let Some(form) = app.account_form.as_mut() else {
        return;
    };
    form.focus = wrapped(form.focus, form.field_count(), step);
    form.cursor = form.field().chars().count();
}

fn field_key(app: &mut App, code: KeyCode) {
    let Some(form) = app.account_form.as_mut() else {
        return;
    };
    if let Access::Cycle(cycle) = form.spec(form.focus).access {
        cycle_key(form, code, cycle);
        return;
    }
    match code {
        KeyCode::Char(ch) => insert(form, ch),
        KeyCode::Backspace => backspace(form),
        KeyCode::Delete => delete(form),
        KeyCode::Left => form.cursor = form.cursor.saturating_sub(1),
        KeyCode::Right => {
            form.cursor =
                (form.cursor + 1).min(form.field().chars().count())
        }
        KeyCode::Home => form.cursor = 0,
        KeyCode::End => form.cursor = form.field().chars().count(),
        _ => {}
    }
}

/// A cycle may hide fields after the focus (the provider's
/// password rows, say), so the focus is clamped afterwards.
fn cycle_key(
    form: &mut AccountFormState,
    code: KeyCode,
    cycle: fn(&mut AccountFormState, i32),
) {
    match code {
        KeyCode::Left => cycle(form, -1),
        KeyCode::Right => cycle(form, 1),
        _ => return,
    }
    form.focus = form.focus.min(form.field_count() - 1);
    form.cursor = 0;
}

fn insert(form: &mut AccountFormState, ch: char) {
    let cursor = form.cursor;
    let Some(field) = form.field_mut() else {
        return;
    };
    let at = byte_index(field, cursor);
    field.insert(at, ch);
    form.cursor += 1;
}

fn backspace(form: &mut AccountFormState) {
    if form.cursor == 0 {
        return;
    }
    form.cursor -= 1;
    delete(form);
}

fn delete(form: &mut AccountFormState) {
    let cursor = form.cursor;
    let Some(field) = form.field_mut() else {
        return;
    };
    if cursor >= field.chars().count() {
        return;
    }
    let at = byte_index(field, cursor);
    field.remove(at);
}

#[cfg(test)]
pub(super) mod tests {
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

    fn provider_row(form: &AccountFormState) -> usize {
        labels(form)
            .iter()
            .position(|label| label.starts_with("oauth provider"))
            .expect("provider row")
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
                "none",
                "",
            ]
            .contains(&value);
            assert!(belongs, "{index}: {value:?}");
        }
    }

    #[test]
    fn prefill_starts_focused_on_the_first_field_at_its_end() {
        let answers = filled_answers();
        let form = AccountFormState::from_answers(
            &answers,
            Some("work".to_string()),
        );
        assert_eq!(form.editing.as_deref(), Some("work"));
        assert_eq!(form.focus, 0);
        assert_eq!(form.cursor, answers.address.chars().count());
        assert_eq!(form.address, answers.address);
        assert_eq!(form.password_cmd, answers.password_cmd);
        assert!(form.keychain_secret.is_empty());
        assert_eq!(form.provider, None);
    }

    #[test]
    fn a_provider_swaps_the_password_fields_for_oauth_ones() {
        let mut form = filled_form();
        assert!(labels(&form).contains(&"password command"));
        assert!(!labels(&form).contains(&"oauth client id"));

        form.provider = Some(OauthProvider::Google);
        let shown = labels(&form);
        assert!(!shown.contains(&"password command"));
        assert!(shown.contains(&"oauth client id"));
        assert!(!shown.iter().any(|label| label.contains("graph")));

        form.provider = Some(OauthProvider::Microsoft);
        let shown = labels(&form);
        assert!(
            shown.iter().any(|label| label.starts_with("graph send"))
        );
        assert!(
            !shown.contains(&"graph tenant"),
            "tenant hides until graph send is on"
        );
        form.graph_send = true;
        assert!(labels(&form).contains(&"graph tenant"));
    }

    fn cycle_at_focus(form: &mut AccountFormState, code: KeyCode) {
        if let Access::Cycle(cycle) = form.spec(form.focus).access {
            cycle_key(form, code, cycle);
        }
    }

    #[test]
    fn left_and_right_cycle_the_provider_row() {
        let mut form = filled_form();
        form.focus = provider_row(&form);
        cycle_at_focus(&mut form, KeyCode::Right);
        assert_eq!(form.provider, Some(OauthProvider::Google));
        cycle_at_focus(&mut form, KeyCode::Right);
        assert_eq!(form.provider, Some(OauthProvider::Microsoft));
        cycle_at_focus(&mut form, KeyCode::Right);
        assert_eq!(form.provider, None);
        cycle_at_focus(&mut form, KeyCode::Left);
        assert_eq!(form.provider, Some(OauthProvider::Microsoft));
    }

    #[test]
    fn typing_on_a_cycle_row_changes_nothing() {
        let mut form = filled_form();
        let row = provider_row(&form);
        form.focus = row;
        assert!(form.field_mut().is_none());
        assert_eq!(form.field_value(row), "none");
    }

    #[test]
    fn oauth_prefill_reads_the_oauth_and_graph_tables() {
        use antiphon_config::{Graph, GraphAuth, Oauth};

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
            auth: GraphAuth::Delegated,
            secret_cmd: None,
        });
        form.oauth_prefill(&account);
        assert_eq!(form.provider, Some(OauthProvider::Microsoft));
        assert_eq!(form.client_id, "app-1");
        assert!(form.graph_send);
        assert_eq!(form.tenant, "tenant-1");
    }

    pub(in super::super) fn minimal_account()
    -> antiphon_config::AccountFile {
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
}
