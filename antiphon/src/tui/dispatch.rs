use antiphon_core::Action;
use antiphon_store::MessageSummary;

use super::actions::account_of;
use super::app::{App, View};
use super::compose::{ComposeState, IdentityChoice, bare_address};
use super::decrypt;
use super::identity::{ComposeContext, ComposeIdentity};
use super::lists;
use super::message_list;
use super::prefill::{self, DraftFields, ReplySource};

const CONVENTION_NEW: &str = "new";
const CONVENTION_REPLY: &str = "reply";

fn now_attribution() -> String {
    chrono::Local::now()
        .format(prefill::ATTRIBUTION_DATE_FORMAT)
        .to_string()
}

pub(super) fn pending_template_request(
    app: &mut App,
    context: &ComposeContext,
) -> Option<ComposeState> {
    let name = app.pending_template.take()?;
    let Some(template) = context.template(&name) else {
        app.notice = Some(format!("no template named {name}"));
        return None;
    };
    let first = app.accounts.first().cloned().unwrap_or_default();
    let (account, identity) = context.identity_for(&first)?;
    let fields = prefill::fresh_fields(
        identity,
        Some(&template),
        &now_attribution(),
    );
    Some(state_for(app, context, account, identity, fields))
}

pub(super) fn pending_unsubscribe_request(
    app: &mut App,
    context: &ComposeContext,
) -> Option<ComposeState> {
    let (account, mailto) = app.pending_unsubscribe.take()?;
    let (account, identity) = context.identity_for(&account)?;
    let fields = prefill::unsubscribe_fields(identity, &mailto);
    Some(state_for(app, context, account, identity, fields))
}

/// :resume reopens a saved draft on the fields stage; the
/// saved plan wins over any armed toggles, and a From no
/// longer configured still cycles as a one-off choice.
pub(super) fn pending_resume_request(
    app: &mut App,
    context: &ComposeContext,
) -> Option<ComposeState> {
    let path = app.pending_resume.take()?;
    let draft = match super::drafts::load(&path) {
        Ok(draft) => draft,
        Err(error) => {
            app.notice = Some(format!("resume: {error}"));
            return None;
        }
    };
    let identity = ComposeIdentity {
        name: draft.from_name.clone(),
        address: draft.from.clone(),
        signature: None,
        pgp_sign: false,
        pgp_key: None,
    };
    let matched = context
        .choices()
        .iter()
        .find(|(account, choice)| {
            *account == draft.account
                && choice.address == identity.address
        })
        .map(|(_, choice)| choice.clone());
    let identity = matched.unwrap_or(identity);
    let mut state = state_for(
        app,
        context,
        &draft.account,
        &identity,
        draft.fields,
    );
    state.sign_override = draft.sign;
    state.encrypt_override = draft.encrypt;
    for path in &draft.attachments {
        match super::attach::load(&path.to_string_lossy()) {
            Ok(attachment) => state.add_attachment(attachment),
            Err(error) => {
                app.notice = Some(format!("resume: {error}"));
            }
        }
    }
    Some(state)
}

/// A compose ready for the fields stage: every configured
/// identity to cycle through, the resolved default selected,
/// and any armed :sign/:encrypt overrides consumed.
fn state_for(
    app: &mut App,
    context: &ComposeContext,
    account: &str,
    identity: &ComposeIdentity,
    fields: DraftFields,
) -> ComposeState {
    let mut choices: Vec<IdentityChoice> = context
        .choices()
        .iter()
        .map(|(account, identity)| IdentityChoice {
            account: account.clone(),
            identity: identity.clone(),
        })
        .collect();
    let position = choices.iter().position(|choice| {
        choice.account == account
            && choice.identity.address == identity.address
    });
    let chosen = position.unwrap_or_else(|| {
        choices.insert(
            0,
            IdentityChoice {
                account: account.to_string(),
                identity: identity.clone(),
            },
        );
        0
    });
    let overrides =
        (app.pending_sign.take(), app.pending_encrypt.take());
    ComposeState::new(choices, chosen, fields, overrides)
}

pub(super) fn dispatch(
    app: &mut App,
    action: Action,
    context: &ComposeContext,
) -> Option<ComposeState> {
    if action == Action::Compose && app.view == View::List {
        app.notice = None;
        return fresh_request(app, context);
    }
    if action == Action::Reply {
        app.notice = None;
        return reply_request(app, context);
    }
    if action == Action::ReplyList {
        app.notice = None;
        return list_reply_request(app, context);
    }
    if action == Action::Help {
        app.help = true;
        return None;
    }
    if action == Action::ToggleHtml {
        match app.view {
            View::Pager => toggle_html(app),
            View::List => toggle_preview_html(app),
            _ => {}
        }
        return None;
    }
    let opening = action == Action::Open && app.view == View::List;
    if !opening {
        app.apply(action);
        return None;
    }
    let path = app.selected_message()?.path.clone();
    match std::fs::read(&path) {
        Ok(raw) => {
            let opened =
                decrypt::read_message(&raw, &app.keyring, None);
            app.pager_raw = raw;
            app.pager_html = false;
            app.open_message(opened);
        }
        Err(error) => {
            app.pager_raw = Vec::new();
            app.open_pager(
                format!("cannot read {}: {error}", path.display()),
                antiphon_pgp::Signature::none(),
                Vec::new(),
            );
        }
    }
    None
}

fn body_text(raw: &[u8]) -> String {
    antiphon_render::body_text(raw).text
}

fn fresh_request(
    app: &mut App,
    context: &ComposeContext,
) -> Option<ComposeState> {
    let first = app.accounts.first().cloned().unwrap_or_default();
    let Some((account, identity)) = context.identity_for(&first) else {
        app.notice = Some("no compose identity configured".into());
        return None;
    };
    let fields = prefill::fresh_fields(
        identity,
        context.template(CONVENTION_NEW).as_deref(),
        &now_attribution(),
    );
    Some(state_for(app, context, account, identity, fields))
}

/// Everything both reply flavours need before recipients are
/// decided: the message, its raw bytes, the delivered
/// addresses, and the identity the reply sends from.
struct ReplyBasis {
    message: MessageSummary,
    raw: Vec<u8>,
    delivered: Vec<String>,
    account: String,
    identity: ComposeIdentity,
}

fn reply_basis(
    app: &mut App,
    context: &ComposeContext,
) -> Option<ReplyBasis> {
    let Some(message) = app.selected_message().cloned() else {
        app.notice = Some("no message selected".into());
        return None;
    };
    let raw = match std::fs::read(&message.path) {
        Ok(raw) => raw,
        Err(error) => {
            app.notice = Some(format!(
                "cannot read {}: {error}",
                message.path.display()
            ));
            return None;
        }
    };
    let delivered = antiphon_render::delivered_addresses(&raw);
    let Some((account, identity)) = context
        .reply_identity_for(&account_of(&message.path), &delivered)
    else {
        app.notice = Some("no compose identity configured".into());
        return None;
    };
    Some(ReplyBasis {
        message,
        raw,
        delivered,
        account,
        identity,
    })
}

fn reply_request(
    app: &mut App,
    context: &ComposeContext,
) -> Option<ComposeState> {
    let basis = reply_basis(app, context)?;
    let to = bare_address(&basis.message.from);
    finish_reply(app, context, basis, &to, "", None)
}

fn list_reply_request(
    app: &mut App,
    context: &ComposeContext,
) -> Option<ComposeState> {
    let basis = reply_basis(app, context)?;
    let headers = antiphon_render::list_headers(&basis.raw);
    let plan = lists::list_recipients(
        &headers,
        &basis.message.from,
        &basis.delivered,
        &basis.identity.address,
    );
    let plan = match plan {
        Ok(plan) => plan,
        Err(notice) => {
            app.notice = Some(notice);
            return None;
        }
    };
    let to = plan.to.join(", ");
    let cc = plan.cc.join(", ");
    finish_reply(app, context, basis, &to, &cc, plan.warning)
}

fn finish_reply(
    app: &mut App,
    context: &ComposeContext,
    basis: ReplyBasis,
    to: &str,
    cc: &str,
    warning: Option<String>,
) -> Option<ComposeState> {
    let source = ReplySource {
        from: &basis.message.from,
        subject: &basis.message.subject,
        message_id: &basis.message.id,
        date: &message_list::format_date(
            basis.message.date_unix,
            prefill::ATTRIBUTION_DATE_FORMAT,
        ),
        body: &body_text(&basis.raw),
    };
    let fields = prefill::reply_fields(
        &basis.identity,
        &source,
        to,
        cc,
        context.template(CONVENTION_REPLY).as_deref(),
    );
    app.notice = warning;
    Some(state_for(
        app,
        context,
        &basis.account,
        &basis.identity,
        fields,
    ))
}

/// Re-renders the open message with the other body part; the
/// raw bytes stay in hand so no file or agent round-trip
/// repeats beyond the render itself.
fn toggle_html(app: &mut App) {
    if !antiphon_render::has_html_part(&app.pager_raw) {
        app.notice = Some("this message has no html part".to_string());
        return;
    }
    app.pager_html = !app.pager_html;
    let preference = if app.pager_html {
        antiphon_render::BodyPreference::Html
    } else {
        antiphon_render::BodyPreference::Plain
    };
    let opened = decrypt::read_message_preferring(
        &app.pager_raw,
        &app.keyring,
        None,
        preference,
    );
    let scroll = app.pager_scroll;
    app.open_message(opened);
    app.pager_scroll = scroll;
    app.notice = Some(
        if app.pager_html {
            "showing the html part"
        } else {
            "showing the plain part"
        }
        .to_string(),
    );
}

/// The pane preview flips parts too; the flag resets when the
/// selection moves so each message starts on its plain part.
fn toggle_preview_html(app: &mut App) {
    let Some(message) = app.selected_message() else {
        return;
    };
    let Ok(raw) = std::fs::read(&message.path) else {
        return;
    };
    if !antiphon_render::has_html_part(&raw) {
        app.notice = Some("this message has no html part".to_string());
        return;
    }
    app.preview_html = !app.preview_html;
    app.preview = None;
    app.notice = Some(
        if app.preview_html {
            "pane: html part"
        } else {
            "pane: plain part"
        }
        .to_string(),
    );
}
