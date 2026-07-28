use antiphon_config::{GraphAuth, OauthProvider};
use antiphon_ui::AccountAccent;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::account_form_fields::{
    Access, AccountType, FIELDS, Field, FieldSpec,
};
use super::app::App;
use super::headers::byte_index;
use super::settings::wrapped;
use crate::account_wizard::AccountAnswers;

/// The in-TUI replacement for the setup wizard's terminal Q&A:
/// one field per row, `editing` naming the account file an
/// edit overwrites (and possibly renames) rather than an add.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct AccountFormState {
    pub(super) account_type: AccountType,
    pub(super) address: String,
    pub(super) name: String,
    pub(super) from_name: String,
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
            from_name: answers.from_name.clone(),
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

    /// Fields follow the type: an OAuth type has fixed servers
    /// and signs in with a grant, so it hides the imap/smtp
    /// rows and the password rows and shows a client id (and,
    /// for Microsoft, the graph rows); the Keychain field is
    /// macOS only.
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
            Field::ImapHost
            | Field::ImapUser
            | Field::SmtpHost
            | Field::PasswordCmd => {
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
