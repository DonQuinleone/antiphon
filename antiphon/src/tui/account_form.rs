use antiphon_config::Dirs;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::app::App;
use super::headers::byte_index;
use crate::account_wizard::{self, AccountAnswers};
use crate::setup::validate_address;

#[cfg(target_os = "macos")]
pub(super) const FIELD_COUNT: usize = 7;
#[cfg(not(target_os = "macos"))]
pub(super) const FIELD_COUNT: usize = 6;
const LAST_FIELD: usize = FIELD_COUNT - 1;

/// The account form's fields, in the exact order
/// `account_wizard::prompt_account` asks them; `masked` alone
/// tells the draw and the key handling apart, so nothing here
/// branches per field beyond this table.
struct FieldSpec {
    label: &'static str,
    get: fn(&AccountFormState) -> &str,
    get_mut: fn(&mut AccountFormState) -> &mut String,
    masked: bool,
}

macro_rules! field {
    ($label:literal, $name:ident) => {
        FieldSpec {
            label: $label,
            get: |state| &state.$name,
            get_mut: |state| &mut state.$name,
            masked: false,
        }
    };
}

#[cfg(target_os = "macos")]
const FIELDS: [FieldSpec; FIELD_COUNT] = [
    field!("e-mail address", address),
    field!("account name", name),
    field!("imap host", imap_host),
    field!("imap user", imap_user),
    field!("smtp host", smtp_host),
    field!("password command", password_cmd),
    FieldSpec {
        label: "password (stored in Keychain)",
        get: |state| &state.keychain_secret,
        get_mut: |state| &mut state.keychain_secret,
        masked: true,
    },
];

#[cfg(not(target_os = "macos"))]
const FIELDS: [FieldSpec; FIELD_COUNT] = [
    field!("e-mail address", address),
    field!("account name", name),
    field!("imap host", imap_host),
    field!("imap user", imap_user),
    field!("smtp host", smtp_host),
    field!("password command", password_cmd),
];

pub(super) const PASSWORD_HINT: &str =
    "empty = use the Keychain field below";

/// The in-TUI replacement for the setup wizard's terminal Q&A:
/// one field per row, `editing` naming the account file an
/// edit overwrites (and possibly renames) rather than an add.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct AccountFormState {
    address: String,
    name: String,
    imap_host: String,
    imap_user: String,
    smtp_host: String,
    password_cmd: String,
    keychain_secret: String,
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
            keychain_secret: String::new(),
            focus: 0,
            cursor: answers.address.chars().count(),
            editing,
            error: None,
        }
    }

    pub(super) fn field_label(&self, index: usize) -> &'static str {
        FIELDS[index].label
    }

    pub(super) fn field_value(&self, index: usize) -> &str {
        (FIELDS[index].get)(self)
    }

    pub(super) fn field_masked(&self, index: usize) -> bool {
        FIELDS[index].masked
    }

    fn field(&self) -> &str {
        (FIELDS[self.focus].get)(self)
    }

    fn field_mut(&mut self) -> &mut String {
        (FIELDS[self.focus].get_mut)(self)
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
        self.account_form = Some(AccountFormState::from_answers(
            &answers,
            Some(file_stem.to_string()),
        ));
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
            save(app);
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
    if form.focus == LAST_FIELD {
        save(app);
    } else {
        step_focus(app, 1);
    }
}

fn step_focus(app: &mut App, step: i32) {
    let Some(form) = app.account_form.as_mut() else {
        return;
    };
    let count = FIELD_COUNT as i32;
    let next = (form.focus as i32 + step).rem_euclid(count);
    form.focus = next as usize;
    form.cursor = form.field().chars().count();
}

fn field_key(app: &mut App, code: KeyCode) {
    let Some(form) = app.account_form.as_mut() else {
        return;
    };
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

fn insert(form: &mut AccountFormState, ch: char) {
    let cursor = form.cursor;
    let field = form.field_mut();
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
    let field = form.field_mut();
    if cursor >= field.chars().count() {
        return;
    }
    let at = byte_index(field, cursor);
    field.remove(at);
}

fn save(app: &mut App) {
    let Some(form) = app.account_form.as_ref() else {
        return;
    };
    match build_and_write(&app.dirs, form) {
        Ok(name) => {
            app.notice = Some(match super::request_reload() {
                None => format!("account {name} saved; syncing"),
                Some(notice) => {
                    format!("account {name} saved ({notice})")
                }
            });
            app.account_form = None;
            app.refresh_settings_accounts();
        }
        Err(error) => {
            if let Some(form) = app.account_form.as_mut() {
                form.error = Some(error);
            }
        }
    }
}

fn build_and_write(
    dirs: &Dirs,
    form: &AccountFormState,
) -> Result<String, String> {
    validate(form)?;
    let answers = AccountAnswers {
        name: form.name.trim().to_string(),
        address: form.address.trim().to_string(),
        imap_host: form.imap_host.trim().to_string(),
        imap_user: form.imap_user.trim().to_string(),
        smtp_host: form.smtp_host.trim().to_string(),
        password_cmd: resolve_password_cmd(form)?,
    };
    let adding = form.editing.is_none();
    if adding && account_path(dirs, &answers.name).exists() {
        return Err(format!("{} already exists", answers.name));
    }
    account_wizard::write_account_file(
        dirs,
        &answers,
        form.editing.as_deref(),
    )?;
    Ok(answers.name)
}

fn account_path(dirs: &Dirs, name: &str) -> std::path::PathBuf {
    dirs.config.join("accounts").join(format!("{name}.toml"))
}

fn validate(form: &AccountFormState) -> Result<(), String> {
    if form.name.trim().is_empty() {
        return Err("account name is required".to_string());
    }
    validate_address(form.address.trim())?;
    if form.imap_host.trim().is_empty() {
        return Err("imap host is required".to_string());
    }
    if form.imap_user.trim().is_empty() {
        return Err("imap user is required".to_string());
    }
    if form.smtp_host.trim().is_empty() {
        return Err("smtp host is required".to_string());
    }
    Ok(())
}

/// A typed password command wins outright; otherwise, on
/// macOS, the masked field's secret is stored in the Keychain
/// and its lookup command takes the empty field's place.
fn resolve_password_cmd(
    form: &AccountFormState,
) -> Result<String, String> {
    let typed = form.password_cmd.trim();
    if !typed.is_empty() {
        return Ok(typed.to_string());
    }
    if !cfg!(target_os = "macos") {
        return Err(
            "give a password command, e.g. pass show mail/name"
                .to_string(),
        );
    }
    let secret = form.keychain_secret.trim();
    if secret.is_empty() {
        return Err("type the password into the Keychain field, \
                    or give a password command above"
            .to_string());
    }
    account_wizard::store_supplied_secret(
        form.name.trim(),
        form.address.trim(),
        secret,
    )
}

#[cfg(test)]
mod tests {
    use super::super::testkit::TempDir;
    use super::*;

    fn filled_answers() -> AccountAnswers {
        AccountAnswers {
            name: "work".to_string(),
            address: "quin@example.com".to_string(),
            imap_host: "imap.example.com".to_string(),
            imap_user: "quin@example.com".to_string(),
            smtp_host: "smtp.example.com".to_string(),
            password_cmd: "pass show mail/work".to_string(),
        }
    }

    fn dirs_at(root: &std::path::Path) -> Dirs {
        Dirs {
            config: root.to_path_buf(),
            state: root.join("state"),
            cache: root.join("cache"),
            data: root.join("data"),
        }
    }

    #[test]
    fn the_field_table_round_trips_every_answer() {
        let answers = filled_answers();
        let form = AccountFormState::from_answers(&answers, None);
        for index in 0..FIELD_COUNT {
            let value = form.field_value(index);
            let belongs_to_an_answer = [
                answers.name.as_str(),
                answers.address.as_str(),
                answers.imap_host.as_str(),
                answers.imap_user.as_str(),
                answers.smtp_host.as_str(),
                answers.password_cmd.as_str(),
                "",
            ]
            .contains(&value);
            assert!(belongs_to_an_answer, "{index}: {value:?}");
        }
        let rebuilt = AccountAnswers {
            name: form.name.clone(),
            address: form.address.clone(),
            imap_host: form.imap_host.clone(),
            imap_user: form.imap_user.clone(),
            smtp_host: form.smtp_host.clone(),
            password_cmd: form.password_cmd.clone(),
        };
        assert_eq!(rebuilt, answers);
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
    }

    #[test]
    fn a_blank_password_command_fails_validation_off_macos() {
        if cfg!(target_os = "macos") {
            return;
        }
        let mut form =
            AccountFormState::from_answers(&filled_answers(), None);
        form.password_cmd = String::new();
        assert!(resolve_password_cmd(&form).is_err());
    }

    #[test]
    fn saving_an_edit_overwrites_only_the_one_file() {
        let root = TempDir::new();
        let dirs = dirs_at(&root.path);
        account_wizard::write_account_file(
            &dirs,
            &filled_answers(),
            None,
        )
        .expect("seed the account file");

        let mut form =
            AccountFormState::from_answers(&filled_answers(), None);
        form.editing = Some("work".to_string());
        form.imap_host = "imap2.example.com".to_string();
        let name = build_and_write(&dirs, &form).expect("save");
        assert_eq!(name, "work");

        let text = std::fs::read_to_string(
            dirs.config.join("accounts/work.toml"),
        )
        .unwrap();
        assert!(text.contains("imap2.example.com"));
    }

    #[test]
    fn renaming_on_save_removes_the_old_file() {
        let root = TempDir::new();
        let dirs = dirs_at(&root.path);
        account_wizard::write_account_file(
            &dirs,
            &filled_answers(),
            None,
        )
        .expect("seed the account file");

        let mut form =
            AccountFormState::from_answers(&filled_answers(), None);
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
        account_wizard::write_account_file(
            &dirs,
            &filled_answers(),
            None,
        )
        .expect("seed the account file");

        let form =
            AccountFormState::from_answers(&filled_answers(), None);
        assert!(build_and_write(&dirs, &form).is_err());
    }

    #[test]
    fn esc_closes_the_form_without_writing_anything() {
        use ratatui::crossterm::event::KeyModifiers as Mods;

        let mut app = super::super::testkit::app_with_messages(1);
        app.dirs = dirs_at(&TempDir::new().path);
        app.open_account_form_add();
        feed(&mut app, KeyEvent::new(KeyCode::Esc, Mods::NONE));
        assert!(app.account_form.is_none());
    }
}
