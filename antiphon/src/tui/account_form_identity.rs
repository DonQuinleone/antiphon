//! The identity sub-editor: an account carries one or more
//! `[[identity]]` blocks, each a from name and address with its
//! own signature, PGP key and auto-sign toggle and match
//! patterns. The account form shows them as a list (add, edit,
//! remove) that opens this per-identity field editor.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::account_form::{AccountFormState, edit_text};
use super::account_form_fields::on_off;
use super::app::App;
use super::settings::wrapped;

/// One identity as the form holds it: every value a string (the
/// match patterns comma-separated) bar the auto-sign toggle.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct FormIdentity {
    pub(super) from_name: String,
    pub(super) address: String,
    pub(super) pgp_key: String,
    pub(super) pgp_sign: bool,
    pub(super) signature: String,
    pub(super) matches: String,
}

impl FormIdentity {
    pub(super) fn seed(from_name: &str, address: &str) -> FormIdentity {
        FormIdentity {
            from_name: from_name.to_string(),
            address: address.to_string(),
            ..FormIdentity::default()
        }
    }

    pub(super) fn from_config(
        identity: &antiphon_config::Identity,
    ) -> FormIdentity {
        FormIdentity {
            from_name: identity.name.clone().unwrap_or_default(),
            address: identity.address.clone(),
            pgp_key: identity.pgp_key.clone().unwrap_or_default(),
            pgp_sign: identity.pgp_sign,
            signature: identity.signature.clone().unwrap_or_default(),
            matches: identity.matches.join(", "),
        }
    }

    /// The form values as a config identity ready to write. An
    /// empty from address falls back to the account address and
    /// an empty match list to matching that address, so a
    /// minimally-filled identity still routes.
    pub(super) fn to_config(
        &self,
        account_address: &str,
    ) -> antiphon_config::Identity {
        let address = non_empty(&self.address)
            .unwrap_or_else(|| account_address.trim().to_string());
        let matches = parse_matches(&self.matches, &address);
        antiphon_config::Identity {
            address,
            name: non_empty(&self.from_name),
            signature: non_empty(&self.signature),
            matches,
            pgp_sign: self.pgp_sign,
            pgp_key: non_empty(&self.pgp_key),
        }
    }
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn parse_matches(raw: &str, address: &str) -> Vec<String> {
    let parsed: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect();
    if parsed.is_empty() {
        return vec![address.to_string()];
    }
    parsed
}

/// A one-line label for an identity in the list and summary:
/// its from name, or its address, or a placeholder when neither
/// is filled in yet.
pub(super) fn descriptor(identity: &FormIdentity) -> String {
    non_empty(&identity.from_name)
        .or_else(|| non_empty(&identity.address))
        .unwrap_or_else(|| "(unnamed)".to_string())
}

pub(super) fn summary(identities: &[FormIdentity]) -> String {
    let count = identities.len();
    let noun = if count == 1 { "identity" } else { "identities" };
    let names: Vec<String> =
        identities.iter().map(descriptor).collect();
    format!("{count} {noun}: {}", names.join(", "))
}

/// The identity UI layered over the account form: a list of the
/// account's identities, or the field editor for one of them.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum IdentityUi {
    List { selected: usize },
    Edit(IdentityEditor),
}

/// Editing one identity: `target` is the list slot a save
/// overwrites (`None` for a fresh add), `origin` the list
/// selection to restore on cancel.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct IdentityEditor {
    pub(super) target: Option<usize>,
    pub(super) origin: usize,
    pub(super) draft: FormIdentity,
    pub(super) focus: usize,
    pub(super) cursor: usize,
}

pub(super) struct EditorSpec {
    pub(super) label: &'static str,
    pub(super) get: fn(&FormIdentity) -> &str,
    get_mut: Option<fn(&mut FormIdentity) -> &mut String>,
    pub(super) toggle: bool,
}

pub(super) const EDITOR_FIELDS: &[EditorSpec] = &[
    EditorSpec {
        label: "from name",
        get: |identity| &identity.from_name,
        get_mut: Some(|identity| &mut identity.from_name),
        toggle: false,
    },
    EditorSpec {
        label: "from address",
        get: |identity| &identity.address,
        get_mut: Some(|identity| &mut identity.address),
        toggle: false,
    },
    EditorSpec {
        label: "pgp key",
        get: |identity| &identity.pgp_key,
        get_mut: Some(|identity| &mut identity.pgp_key),
        toggle: false,
    },
    EditorSpec {
        label: "auto-sign",
        get: |identity| on_off(identity.pgp_sign),
        get_mut: None,
        toggle: true,
    },
    EditorSpec {
        label: "signature",
        get: |identity| &identity.signature,
        get_mut: Some(|identity| &mut identity.signature),
        toggle: false,
    },
    EditorSpec {
        label: "match patterns",
        get: |identity| &identity.matches,
        get_mut: Some(|identity| &mut identity.matches),
        toggle: false,
    },
];

pub(super) fn open_list(app: &mut App) {
    let Some(form) = app.account_form.as_mut() else {
        return;
    };
    if form.identities.is_empty() {
        form.identities.push(FormIdentity::default());
    }
    form.identity_ui = Some(IdentityUi::List { selected: 0 });
}

pub(super) fn feed(app: &mut App, key: KeyEvent) {
    let Some(form) = app.account_form.as_mut() else {
        return;
    };
    if matches!(form.identity_ui, Some(IdentityUi::Edit(_))) {
        editor_key(form, key);
        return;
    }
    if matches!(form.identity_ui, Some(IdentityUi::List { .. })) {
        list_key(form, key.code);
    }
}

fn list_key(form: &mut AccountFormState, code: KeyCode) {
    let selected = match &form.identity_ui {
        Some(IdentityUi::List { selected }) => *selected,
        _ => return,
    };
    let last = form.identities.len().saturating_sub(1);
    match code {
        KeyCode::Esc => form.identity_ui = None,
        KeyCode::Up | KeyCode::BackTab => {
            set_selected(form, selected.saturating_sub(1))
        }
        KeyCode::Down | KeyCode::Tab => {
            set_selected(form, (selected + 1).min(last))
        }
        KeyCode::Char('a') => open_editor(form, None, selected),
        KeyCode::Char('e') | KeyCode::Enter => {
            open_editor(form, Some(selected), selected)
        }
        KeyCode::Char('d') | KeyCode::Char('x') => {
            remove(form, selected)
        }
        _ => {}
    }
}

fn set_selected(form: &mut AccountFormState, value: usize) {
    if let Some(IdentityUi::List { selected }) = &mut form.identity_ui {
        *selected = value;
    }
}

/// A fresh identity defaults its from address to the account
/// e-mail address, so the common single-address case needs no
/// typing.
fn open_editor(
    form: &mut AccountFormState,
    target: Option<usize>,
    origin: usize,
) {
    let mut draft = match target {
        Some(index) => form.identities[index].clone(),
        None => FormIdentity::default(),
    };
    if draft.address.trim().is_empty() {
        draft.address = form.address.clone();
    }
    let cursor = draft.from_name.chars().count();
    form.identity_ui = Some(IdentityUi::Edit(IdentityEditor {
        target,
        origin,
        draft,
        focus: 0,
        cursor,
    }));
}

/// An account needs at least one identity for its compose From,
/// so removing the last one is refused.
fn remove(form: &mut AccountFormState, selected: usize) {
    if form.identities.len() <= 1 {
        return;
    }
    form.identities.remove(selected);
    let last = form.identities.len() - 1;
    set_selected(form, selected.min(last));
}

fn editor_key(form: &mut AccountFormState, key: KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        if key.code == KeyCode::Char('s') {
            commit(form);
        }
        return;
    }
    match key.code {
        KeyCode::Esc => close_editor(form),
        KeyCode::Tab | KeyCode::Down => editor_step(form, 1),
        KeyCode::BackTab | KeyCode::Up => editor_step(form, -1),
        KeyCode::Enter => editor_enter(form),
        other => editor_field_key(form, other),
    }
}

fn editor(form: &mut AccountFormState) -> Option<&mut IdentityEditor> {
    match &mut form.identity_ui {
        Some(IdentityUi::Edit(editor)) => Some(editor),
        _ => None,
    }
}

fn editor_step(form: &mut AccountFormState, step: i32) {
    let Some(editor) = editor(form) else {
        return;
    };
    editor.focus = wrapped(editor.focus, EDITOR_FIELDS.len(), step);
    editor.cursor = editor_value_len(editor);
}

fn editor_enter(form: &mut AccountFormState) {
    let focus = match editor(form) {
        Some(editor) => editor.focus,
        None => return,
    };
    if focus == EDITOR_FIELDS.len() - 1 {
        commit(form);
    } else {
        editor_step(form, 1);
    }
}

fn editor_field_key(form: &mut AccountFormState, code: KeyCode) {
    let Some(editor) = editor(form) else {
        return;
    };
    let spec = &EDITOR_FIELDS[editor.focus];
    if spec.toggle {
        toggle_sign(editor, code);
        return;
    }
    let Some(get_mut) = spec.get_mut else {
        return;
    };
    let cursor = editor.cursor;
    editor.cursor = edit_text(get_mut(&mut editor.draft), cursor, code);
}

fn toggle_sign(editor: &mut IdentityEditor, code: KeyCode) {
    match code {
        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') => {
            editor.draft.pgp_sign = !editor.draft.pgp_sign;
        }
        _ => {}
    }
}

fn editor_value_len(editor: &IdentityEditor) -> usize {
    (EDITOR_FIELDS[editor.focus].get)(&editor.draft)
        .chars()
        .count()
}

fn commit(form: &mut AccountFormState) {
    let Some(IdentityUi::Edit(editor)) = form.identity_ui.take() else {
        return;
    };
    let selected = match editor.target {
        Some(index) => {
            form.identities[index] = editor.draft;
            index
        }
        None => {
            form.identities.push(editor.draft);
            form.identities.len() - 1
        }
    };
    form.identity_ui = Some(IdentityUi::List { selected });
}

fn close_editor(form: &mut AccountFormState) {
    let origin = match &form.identity_ui {
        Some(IdentityUi::Edit(editor)) => editor.origin,
        _ => return,
    };
    let last = form.identities.len().saturating_sub(1);
    form.identity_ui = Some(IdentityUi::List {
        selected: origin.min(last),
    });
}

#[cfg(test)]
#[path = "account_form_identity_tests.rs"]
mod tests;
