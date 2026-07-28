use antiphon_config::{GraphAuth, OauthProvider};
use antiphon_ui::AccountAccent;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::app::App;
use super::headers::byte_index;
use super::settings::wrapped;
use crate::account_wizard::AccountAnswers;

/// The kind of account, chosen by the segmented toggle at the
/// top of the form; it drives which of the fields below show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum AccountType {
    #[default]
    Imap,
    Microsoft,
    Google,
}

/// The type toggle's fixed order, matching `TYPE_OPTIONS`.
const TYPES: [AccountType; 3] = [
    AccountType::Imap,
    AccountType::Microsoft,
    AccountType::Google,
];

const TYPE_OPTIONS: [&str; 3] = ["IMAP", "Microsoft 365", "Google"];
const GRAPH_SEND_OPTIONS: [&str; 2] = ["off", "on"];
const GRAPH_AUTH_OPTIONS: [&str; 2] = ["delegated", "app-only"];

impl AccountType {
    fn provider(self) -> Option<OauthProvider> {
        match self {
            AccountType::Imap => None,
            AccountType::Microsoft => Some(OauthProvider::Microsoft),
            AccountType::Google => Some(OauthProvider::Google),
        }
    }

    fn accent(self) -> AccountAccent {
        match self {
            AccountType::Imap => AccountAccent::Imap,
            AccountType::Microsoft => AccountAccent::Microsoft,
            AccountType::Google => AccountAccent::Google,
        }
    }

    fn from_provider(provider: OauthProvider) -> AccountType {
        match provider {
            OauthProvider::Microsoft => AccountType::Microsoft,
            OauthProvider::Google => AccountType::Google,
        }
    }
}

/// Every field the form can show, in display order; which of
/// them are actually visible follows the account type (see
/// `AccountFormState::shows`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Field {
    AccountType,
    Address,
    Name,
    ImapHost,
    ImapUser,
    SmtpHost,
    ClientId,
    GraphSend,
    GraphAuth,
    Tenant,
    GraphSecretCmd,
    PasswordCmd,
    KeychainSecret,
}

/// How a field reacts to keys: text fields edit a string in
/// place, cycling fields step through fixed choices with
/// left/right (and space) and draw as a segmented toggle.
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
    /// `Some` draws the value as a segmented toggle over these
    /// options; `None` draws the plain string `get` returns.
    segments: Option<&'static [&'static str]>,
    selected: fn(&AccountFormState) -> usize,
}

macro_rules! field {
    ($field:expr, $label:literal, $name:ident) => {
        FieldSpec {
            field: $field,
            label: $label,
            masked: false,
            get: |state| &state.$name,
            access: Access::Edit(|state| &mut state.$name),
            segments: None,
            selected: |_| 0,
        }
    };
}

const FIELDS: &[FieldSpec] = &[
    FieldSpec {
        field: Field::AccountType,
        label: "account type",
        masked: false,
        get: |state| type_name(state.account_type),
        access: Access::Cycle(cycle_type),
        segments: Some(&TYPE_OPTIONS),
        selected: type_index,
    },
    field!(Field::Address, "e-mail address", address),
    field!(Field::Name, "account name", name),
    field!(Field::ImapHost, "imap host", imap_host),
    field!(Field::ImapUser, "imap user", imap_user),
    field!(Field::SmtpHost, "smtp host", smtp_host),
    field!(Field::ClientId, "oauth client id", client_id),
    FieldSpec {
        field: Field::GraphSend,
        label: "graph send",
        masked: false,
        get: |state| on_off(state.graph_send),
        access: Access::Cycle(|state, _| {
            state.graph_send = !state.graph_send
        }),
        segments: Some(&GRAPH_SEND_OPTIONS),
        selected: |state| usize::from(state.graph_send),
    },
    FieldSpec {
        field: Field::GraphAuth,
        label: "graph auth",
        masked: false,
        get: |state| graph_auth_name(state.graph_auth),
        access: Access::Cycle(cycle_graph_auth),
        segments: Some(&GRAPH_AUTH_OPTIONS),
        selected: graph_auth_index,
    },
    field!(Field::Tenant, "graph tenant", tenant),
    field!(
        Field::GraphSecretCmd,
        "graph secret command",
        graph_secret_cmd
    ),
    field!(Field::PasswordCmd, "password command", password_cmd),
    FieldSpec {
        field: Field::KeychainSecret,
        label: "password (stored in Keychain)",
        masked: true,
        get: |state| &state.keychain_secret,
        access: Access::Edit(|state| &mut state.keychain_secret),
        segments: None,
        selected: |_| 0,
    },
];

fn type_name(account_type: AccountType) -> &'static str {
    TYPE_OPTIONS[type_index_of(account_type)]
}

fn type_index_of(account_type: AccountType) -> usize {
    TYPES
        .iter()
        .position(|candidate| *candidate == account_type)
        .unwrap_or(0)
}

fn type_index(state: &AccountFormState) -> usize {
    type_index_of(state.account_type)
}

pub(super) fn provider_name(
    provider: Option<OauthProvider>,
) -> &'static str {
    match provider {
        None => "none",
        Some(OauthProvider::Google) => "google",
        Some(OauthProvider::Microsoft) => "microsoft",
    }
}

pub(super) fn graph_auth_name(auth: GraphAuth) -> &'static str {
    match auth {
        GraphAuth::Delegated => "delegated",
        GraphAuth::AppOnly => "app-only",
    }
}

/// The `[graph] auth` value as the config parser spells it,
/// distinct from the toggle's display label (`app-only`).
pub(super) fn graph_auth_toml(auth: GraphAuth) -> &'static str {
    match auth {
        GraphAuth::Delegated => "delegated",
        GraphAuth::AppOnly => "app_only",
    }
}

fn graph_auth_index(state: &AccountFormState) -> usize {
    usize::from(state.graph_auth == GraphAuth::AppOnly)
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn cycle_type(state: &mut AccountFormState, step: i32) {
    let current = type_index(state);
    let next = wrapped(current, TYPES.len(), step);
    state.account_type = TYPES[next];
}

fn cycle_graph_auth(state: &mut AccountFormState, _step: i32) {
    state.graph_auth = match state.graph_auth {
        GraphAuth::Delegated => GraphAuth::AppOnly,
        GraphAuth::AppOnly => GraphAuth::Delegated,
    };
}

pub(super) const PASSWORD_HINT: &str =
    "empty = use the Keychain field below";

/// The env var overriding the account file's client id, per
/// provider, surfaced as the client-id field's hint.
pub(super) fn client_id_env_hint(
    account_type: AccountType,
) -> Option<&'static str> {
    match account_type {
        AccountType::Microsoft => Some("or set ANTIPHON_MS_CLIENT_ID"),
        AccountType::Google => Some("or set ANTIPHON_GOOGLE_CLIENT_ID"),
        AccountType::Imap => None,
    }
}

/// The in-TUI replacement for the setup wizard's terminal Q&A:
/// one field per row, `editing` naming the account file an
/// edit overwrites (and possibly renames) rather than an add.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct AccountFormState {
    pub(super) account_type: AccountType,
    pub(super) address: String,
    pub(super) name: String,
    pub(super) imap_host: String,
    pub(super) imap_user: String,
    pub(super) smtp_host: String,
    pub(super) password_cmd: String,
    pub(super) keychain_secret: String,
    pub(super) client_id: String,
    pub(super) graph_send: bool,
    pub(super) graph_auth: GraphAuth,
    pub(super) tenant: String,
    pub(super) graph_secret_cmd: String,
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
            editing,
            ..AccountFormState::default()
        }
    }

    /// The account type is inferred from its [oauth]/[graph]
    /// config, so editing an existing account lands on the
    /// right toggle and keeps its graph settings.
    fn infer_type(&mut self, account: &antiphon_config::AccountFile) {
        if let Some(oauth) = &account.oauth {
            self.account_type =
                AccountType::from_provider(oauth.provider);
            self.client_id =
                oauth.client_id.clone().unwrap_or_default();
        }
        if let Some(graph) = &account.graph {
            self.graph_send = graph.send;
            self.graph_auth = graph.auth;
            self.tenant = graph.tenant.clone().unwrap_or_default();
            self.graph_secret_cmd =
                graph.secret_cmd.clone().unwrap_or_default();
        }
    }

    pub(super) fn provider(&self) -> Option<OauthProvider> {
        self.account_type.provider()
    }

    pub(super) fn type_accent(&self) -> AccountAccent {
        self.account_type.accent()
    }

    /// Fields follow the type: an OAuth type swaps the password
    /// rows for a client id (and, for Microsoft, the graph
    /// rows); the Keychain field is macOS only.
    fn shows(&self, field: Field) -> bool {
        let microsoft = self.account_type == AccountType::Microsoft;
        match field {
            Field::ClientId => self.provider().is_some(),
            Field::GraphSend => microsoft,
            Field::GraphAuth | Field::Tenant => {
                microsoft && self.graph_send
            }
            Field::GraphSecretCmd => {
                microsoft
                    && self.graph_send
                    && self.graph_auth == GraphAuth::AppOnly
            }
            Field::PasswordCmd => {
                self.account_type == AccountType::Imap
            }
            Field::KeychainSecret => {
                self.account_type == AccountType::Imap
                    && cfg!(target_os = "macos")
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

    pub(super) fn field_segments(
        &self,
        index: usize,
    ) -> Option<&'static [&'static str]> {
        self.spec(index).segments
    }

    pub(super) fn field_selected(&self, index: usize) -> usize {
        (self.spec(index).selected)(self)
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
        form.infer_type(&named.account);
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

/// A cycle may hide fields after the focus (the password rows,
/// the graph rows), so the focus is clamped afterwards. Space
/// steps forward like the reference's segmented control.
fn cycle_key(
    form: &mut AccountFormState,
    code: KeyCode,
    cycle: fn(&mut AccountFormState, i32),
) {
    match code {
        KeyCode::Left => cycle(form, -1),
        KeyCode::Right | KeyCode::Char(' ') => cycle(form, 1),
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
#[path = "account_form_tests.rs"]
pub(super) mod tests;
