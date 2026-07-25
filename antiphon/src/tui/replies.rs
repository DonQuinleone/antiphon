use antiphon_core::Action;
use antiphon_store::MessageSummary;
use antiphon_store::contacts::address_entries;

use super::actions::account_of;
use super::app::App;
use super::compose::{ComposeState, bare_address};
use super::dispatch::{CONVENTION_REPLY, body_text, state_for};
use super::identity::{ComposeContext, ComposeIdentity};
use super::lists;
use super::message_list;
use super::prefill::{self, ReplySource};

type Request = fn(&mut App, &ComposeContext) -> Option<ComposeState>;

/// The reply-family dispatch table: one row per action, so
/// dispatch() stays a lookup.
const REQUESTS: [(Action, Request); 4] = [
    (Action::Reply, reply_request),
    (Action::ReplyAll, reply_all_request),
    (Action::ReplyList, list_reply_request),
    (Action::Forward, forward_request),
];

pub(super) fn request(action: Action) -> Option<Request> {
    REQUESTS
        .iter()
        .find(|(candidate, _)| *candidate == action)
        .map(|(_, request)| *request)
}

/// Everything both reply flavours need before recipients are
/// decided: the message, its raw bytes, the delivered
/// addresses, and the identity the reply sends from.
pub(super) struct ReplyBasis {
    pub(super) message: MessageSummary,
    pub(super) raw: Vec<u8>,
    pub(super) delivered: Vec<String>,
    pub(super) account: String,
    pub(super) identity: ComposeIdentity,
}

pub(super) fn reply_basis(
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
/// Reply-all answers the author (Reply-To respected) and
/// keeps every other original recipient in Cc, minus the
/// replying identity itself.
fn reply_all_request(
    app: &mut App,
    context: &ComposeContext,
) -> Option<ComposeState> {
    let basis = reply_basis(app, context)?;
    let headers = antiphon_render::all_headers(&basis.raw);
    let value = |name: &str| {
        headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case(name))
            .map(|header| header.value.clone())
            .unwrap_or_default()
    };
    let reply_to = value("Reply-To");
    let to = if reply_to.trim().is_empty() {
        bare_address(&basis.message.from)
    } else {
        bare_address(&reply_to)
    };
    let mut cc: Vec<String> = Vec::new();
    for field in [value("To"), value("Cc")] {
        for (address, _) in address_entries(&field) {
            let lowered = address.to_lowercase();
            let skip = lowered == to.to_lowercase()
                || lowered == basis.identity.address.to_lowercase()
                || app.is_own(&address)
                || cc.iter().any(|kept| kept.to_lowercase() == lowered);
            if !skip {
                cc.push(address);
            }
        }
    }
    let cc = cc.join(", ");
    finish_reply(app, context, basis, &to, &cc, None)
}

/// A forward opens with empty recipients and the original
/// inline; the passed tag lands when the forward is sent,
/// not before.
fn forward_request(
    app: &mut App,
    context: &ComposeContext,
) -> Option<ComposeState> {
    let basis = reply_basis(app, context)?;
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
    let fields = prefill::forward_fields(&source);
    let account = basis.account.clone();
    let identity = basis.identity.clone();
    let mut state =
        state_for(app, context, &account, &identity, fields);
    state.forwarded_of = Some((account, basis.message.id.clone()));
    Some(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_fields_wrap_the_original_between_rules() {
        let source = ReplySource {
            from: "Alba Voss <alba@example.com>",
            subject: "Programme draft",
            message_id: "m1@example.com",
            date: "Sat, 25 Jul 2026 at 10:00",
            body: "First draft attached.\n",
        };
        let fields = prefill::forward_fields(&source);
        assert_eq!(fields.subject, "Fwd: Programme draft");
        assert!(fields.to.is_empty());
        assert!(
            fields
                .body
                .starts_with("----- Forwarded message from Alba Voss")
        );
        assert!(fields.body.contains("First draft attached."));
        assert!(
            fields
                .body
                .trim_end()
                .ends_with("----- End forwarded message -----")
        );

        let fwd = ReplySource {
            subject: "Fwd: Programme draft",
            ..source
        };
        assert_eq!(
            prefill::forward_fields(&fwd).subject,
            "Fwd: Programme draft",
            "no double prefix"
        );
    }
}
