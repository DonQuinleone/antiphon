use antiphon_core::Action;

use super::actions::account_of;
use super::app::{App, View};
use super::compose::{self, ReplySource};
use super::crypto::ComposeCrypto;
use super::decrypt;
use super::draw;
use super::identity::{ComposeContext, ComposeIdentity};

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
            app.open_pager(opened.body, opened.signature);
        }
        Err(error) => {
            app.open_pager(
                format!("cannot read {}: {error}", path.display()),
                antiphon_pgp::Signature::none(),
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

fn reply_request(
    app: &mut App,
    context: &ComposeContext,
) -> Option<EditorRequest> {
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
    let source = ReplySource {
        from: &message.from,
        subject: &message.subject,
        message_id: &message.id,
        date: &draw::format_date(
            message.date_unix,
            compose::ATTRIBUTION_DATE_FORMAT,
        ),
        body: &body_text(&raw),
    };
    let text = compose::reply_draft(
        &identity,
        &source,
        context.template(CONVENTION_REPLY).as_deref(),
    );
    Some(EditorRequest {
        account,
        text,
        crypto: compose_crypto(app, &identity),
    })
}
