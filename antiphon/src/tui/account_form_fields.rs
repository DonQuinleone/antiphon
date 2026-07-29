//! The account form's field table: every row the modal can
//! show, in display order, as data rather than a draw-time
//! branch. The identity row is a launcher into the identity
//! sub-editor (see `account_form_identity`); every other row is
//! a text field or a segmented toggle.

use antiphon_config::{GraphAuth, OauthProvider};
use antiphon_ui::AccountAccent;

use super::account_form::AccountFormState;
use super::account_form_identity::FormIdentity;
use crate::tui::settings::wrapped;

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

pub(super) const TYPE_OPTIONS: [&str; 3] =
    ["IMAP", "Microsoft 365", "Google"];
const GRAPH_SEND_OPTIONS: [&str; 2] = ["off", "on"];
const GRAPH_AUTH_OPTIONS: [&str; 2] = ["delegated", "app-only"];

pub(super) const ON_OFF_OPTIONS: [&str; 2] = ["off", "on"];

impl AccountType {
    pub(super) fn provider(self) -> Option<OauthProvider> {
        match self {
            AccountType::Imap => None,
            AccountType::Microsoft => Some(OauthProvider::Microsoft),
            AccountType::Google => Some(OauthProvider::Google),
        }
    }

    pub(super) fn accent(self) -> AccountAccent {
        match self {
            AccountType::Imap => AccountAccent::Imap,
            AccountType::Microsoft => AccountAccent::Microsoft,
            AccountType::Google => AccountAccent::Google,
        }
    }

    pub(super) fn from_provider(
        provider: OauthProvider,
    ) -> AccountType {
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
    FromName,
    FromAddress,
    Identities,
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
/// left/right (and space) and draw as a segmented toggle. The
/// identity row edits nothing here; enter opens its sub-editor.
pub(super) enum Access {
    Edit(fn(&mut AccountFormState) -> &mut String),
    Cycle(fn(&mut AccountFormState, i32)),
    Launch,
}

pub(super) struct FieldSpec {
    pub(super) field: Field,
    pub(super) label: &'static str,
    pub(super) masked: bool,
    pub(super) get: fn(&AccountFormState) -> &str,
    pub(super) access: Access,
    /// `Some` draws the value as a segmented toggle over these
    /// options; `None` draws the plain string `get` returns.
    pub(super) segments: Option<&'static [&'static str]>,
    pub(super) selected: fn(&AccountFormState) -> usize,
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

pub(super) const FIELDS: &[FieldSpec] = &[
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
    FieldSpec {
        field: Field::FromName,
        label: "from name",
        masked: false,
        get: |state| first_from(state, |identity| &identity.from_name),
        access: Access::Edit(|state| {
            &mut first_identity_mut(state).from_name
        }),
        segments: None,
        selected: |_| 0,
    },
    FieldSpec {
        field: Field::FromAddress,
        label: "from address",
        masked: false,
        get: |state| first_from(state, |identity| &identity.address),
        access: Access::Edit(|state| {
            &mut first_identity_mut(state).address
        }),
        segments: None,
        selected: |_| 0,
    },
    FieldSpec {
        field: Field::Identities,
        label: "identities",
        masked: false,
        get: |_| "",
        access: Access::Launch,
        segments: None,
        selected: |_| 0,
    },
    field!(Field::ImapHost, "imap host", imap_host),
    field!(Field::ImapUser, "imap user", imap_user),
    field!(Field::SmtpHost, "smtp host", smtp_host),
    field!(Field::ClientId, "oauth client id", client_id),
    FieldSpec {
        field: Field::GraphSend,
        label: "graph mode",
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
        label: "auth type",
        masked: false,
        get: |state| graph_auth_name(state.graph_auth),
        access: Access::Cycle(cycle_graph_auth),
        segments: Some(&GRAPH_AUTH_OPTIONS),
        selected: graph_auth_index,
    },
    field!(Field::Tenant, "tenant id", tenant),
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

pub(super) fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

/// The new-account flow edits the account's single identity
/// inline, so the from-name and from-address rows read and write
/// `identities[0]`. Reading an empty list yields the empty
/// string; writing seeds a default identity first.
fn first_from<'a>(
    state: &'a AccountFormState,
    field: impl Fn(&'a FormIdentity) -> &'a String,
) -> &'a str {
    state
        .identities
        .first()
        .map(|identity| field(identity).as_str())
        .unwrap_or("")
}

fn first_identity_mut(
    state: &mut AccountFormState,
) -> &mut FormIdentity {
    if state.identities.is_empty() {
        state.identities.push(FormIdentity::default());
    }
    &mut state.identities[0]
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

pub(super) const CLIENT_ID_MS_HINT: &str = "blank for Thunderbird's";

pub(super) const FROM_ADDRESS_HINT: &str = "blank = e-mail address";
