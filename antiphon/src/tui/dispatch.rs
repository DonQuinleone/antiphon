use antiphon_core::Action;
use antiphon_store::MessageSummary;

use super::actions::account_of;
use super::app::{App, View};
use super::compose::{self, ReplySource};
use super::crypto::ComposeCrypto;
use super::decrypt;
use super::identity::{ComposeContext, ComposeIdentity};
use super::lists;
use super::message_list;

const CONVENTION_NEW: &str = "new";
const CONVENTION_REPLY: &str = "reply";

fn now_attribution() -> String {
    chrono::Local::now()
        .format(compose::ATTRIBUTION_DATE_FORMAT)
        .to_string()
}

pub(super) fn pending_template_request(
    app: &mut App,
    context: &ComposeContext,
) -> Option<EditorRequest> {
    let name = app.pending_template.take()?;
    let Some(template) = context.template(&name) else {
        app.notice = Some(format!("no template named {name}"));
        return None;
    };
    let first = app.accounts.first().cloned().unwrap_or_default();
    let (account, identity) = context.identity_for(&first)?;
    Some(EditorRequest {
        account: account.to_string(),
        text: compose::fresh_draft(
            identity,
            Some(&template),
            &now_attribution(),
        ),
        crypto: compose_crypto(app, identity),
    })
}

pub(super) fn pending_unsubscribe_request(
    app: &mut App,
    context: &ComposeContext,
) -> Option<EditorRequest> {
    let (account, mailto) = app.pending_unsubscribe.take()?;
    let (account, identity) = context.identity_for(&account)?;
    Some(EditorRequest {
        account: account.to_string(),
        text: compose::unsubscribe_draft(identity, &mailto),
        crypto: compose_crypto(app, identity),
    })
}

/// A draft ready for the user's editor; the event loop owns
/// the terminal hand-off, so app state never touches it.
pub(super) struct EditorRequest {
    pub(super) account: String,
    pub(super) text: String,
    pub(super) crypto: ComposeCrypto,
}

/// The seal settings a compose starts with: the identity's
/// defaults with any armed per-message overrides consumed.
fn compose_crypto(
    app: &mut App,
    identity: &ComposeIdentity,
) -> ComposeCrypto {
    ComposeCrypto {
        plan: app.take_pgp_plan(identity.pgp_sign),
        key: identity.pgp_key.clone(),
        address: identity.address.clone(),
    }
}

pub(super) fn dispatch(
    app: &mut App,
    action: Action,
    context: &ComposeContext,
) -> Option<EditorRequest> {
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
            app.open_pager(
                opened.body,
                opened.signature,
                opened.invite,
            );
        }
        Err(error) => {
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
) -> Option<EditorRequest> {
    let first = app.accounts.first().cloned().unwrap_or_default();
    let Some((account, identity)) = context.identity_for(&first) else {
        app.notice = Some("no compose identity configured".into());
        return None;
    };
    Some(EditorRequest {
        account: account.to_string(),
        text: compose::fresh_draft(
            identity,
            context.template(CONVENTION_NEW).as_deref(),
            &now_attribution(),
        ),
        crypto: compose_crypto(app, identity),
    })
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
) -> Option<EditorRequest> {
    let basis = reply_basis(app, context)?;
    let to = compose::bare_address(&basis.message.from);
    finish_reply(app, context, basis, &to, "", None)
}

fn list_reply_request(
    app: &mut App,
    context: &ComposeContext,
) -> Option<EditorRequest> {
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
) -> Option<EditorRequest> {
    let source = ReplySource {
        from: &basis.message.from,
        subject: &basis.message.subject,
        message_id: &basis.message.id,
        date: &message_list::format_date(
            basis.message.date_unix,
            compose::ATTRIBUTION_DATE_FORMAT,
        ),
        body: &body_text(&basis.raw),
    };
    let text = compose::reply_draft_to(
        &basis.identity,
        &source,
        to,
        cc,
        context.template(CONVENTION_REPLY).as_deref(),
    );
    app.notice = warning;
    Some(EditorRequest {
        account: basis.account,
        text,
        crypto: compose_crypto(app, &basis.identity),
    })
}
