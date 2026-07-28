use antiphon_core::Action;

use super::app::{App, View};
use super::compose::{ComposeState, IdentityChoice};
use super::decrypt;
use super::identity::{ComposeContext, ComposeIdentity};
use super::prefill::{self, DraftFields};
use super::replies::{self, reply_basis};

const CONVENTION_NEW: &str = "new";
pub(super) const CONVENTION_REPLY: &str = "reply";

pub(super) fn now_attribution() -> String {
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
    let scoped = app.compose_account();
    let (account, identity) = context.identity_for(&scoped)?;
    let fields = prefill::fresh_fields(
        identity,
        Some(&template),
        &now_attribution(),
    );
    Some(state_for(app, context, account, identity, fields))
}

const RSVP_ATTACHMENT_NAME: &str = "invite.ics";
const RSVP_CONTENT_TYPE: &str = "text/calendar; method=REPLY";

/// :accept, :tentative and :decline on an open invite become
/// an ordinary compose to the organiser, the RFC 5546 REPLY
/// riding along as a calendar part; nothing sends before the
/// review screen's confirmation like any other message.
pub(super) fn pending_rsvp_request(
    app: &mut App,
    context: &ComposeContext,
) -> Option<ComposeState> {
    let rsvp = app.pending_rsvp.take()?;
    let basis = reply_basis(app, context)?;
    let now = chrono::Utc::now().timestamp();
    let Some((fields, attachment)) =
        rsvp_parts(&basis.raw, &basis.identity.address, rsvp, now)
    else {
        app.notice = Some("no calendar invite in this message".into());
        return None;
    };
    let account = basis.account.clone();
    let identity = basis.identity.clone();
    let mut state =
        state_for(app, context, &account, &identity, fields);
    state.add_attachment(attachment);
    Some(state)
}

fn rsvp_parts(
    raw: &[u8],
    address: &str,
    rsvp: antiphon_render::Rsvp,
    now_unix: i64,
) -> Option<(DraftFields, super::attach::Attachment)> {
    let reply =
        antiphon_render::itip_reply(raw, address, rsvp, now_unix)?;
    let fields = DraftFields {
        to: reply.organiser,
        subject: reply.subject.clone(),
        body: format!("{}\n", reply.subject),
        ..DraftFields::default()
    };
    let attachment = super::attach::Attachment {
        path: std::path::PathBuf::new(),
        filename: RSVP_ATTACHMENT_NAME.to_string(),
        content_type: RSVP_CONTENT_TYPE,
        bytes: reply.ics.into_bytes(),
    };
    Some((fields, attachment))
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
pub(super) fn state_for(
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
    if let Some(request) = replies::request(action) {
        app.notice = None;
        return request(app, context);
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

pub(super) fn body_text(raw: &[u8]) -> String {
    antiphon_render::body_text(raw).text
}

fn fresh_request(
    app: &mut App,
    context: &ComposeContext,
) -> Option<ComposeState> {
    let scoped = app.compose_account();
    let Some((account, identity)) = context.identity_for(&scoped)
    else {
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

#[cfg(test)]
mod tests {
    use super::*;

    const NOON: i64 = 1_784_800_000;

    fn invite_message() -> Vec<u8> {
        let ics = [
            "BEGIN:VCALENDAR",
            "VERSION:2.0",
            "PRODID:-//Example//Test//EN",
            "METHOD:REQUEST",
            "BEGIN:VEVENT",
            "UID:planning-7@example.com",
            "DTSTAMP:20260724T210000Z",
            "DTSTART:20260801T140000Z",
            "SUMMARY:Planning call",
            "ORGANIZER;CN=Alba:mailto:alba@example.com",
            "ATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:me@example.com",
            "END:VEVENT",
            "END:VCALENDAR",
        ]
        .join("\r\n");
        format!(
            "From: alba@example.com\r\n\
             To: me@example.com\r\n\
             Subject: invite\r\n\
             MIME-Version: 1.0\r\n\
             Content-Type: text/calendar; method=REQUEST\r\n\
             \r\n\
             {ics}\r\n"
        )
        .into_bytes()
    }

    #[test]
    fn an_invite_becomes_a_reply_compose_with_a_calendar_part() {
        let raw = invite_message();
        let (fields, attachment) = rsvp_parts(
            &raw,
            "me@example.com",
            antiphon_render::Rsvp::Accept,
            NOON,
        )
        .unwrap();
        assert_eq!(fields.to, "alba@example.com");
        assert_eq!(fields.subject, "Accepted: Planning call");
        assert!(fields.body.contains("Accepted"));
        assert_eq!(attachment.filename, "invite.ics");
        assert_eq!(
            attachment.content_type,
            "text/calendar; method=REPLY"
        );
        let ics = String::from_utf8(attachment.bytes).unwrap();
        assert!(ics.contains("PARTSTAT=ACCEPTED"), "{ics}");
        assert!(ics.contains("METHOD:REPLY"), "{ics}");
    }

    #[test]
    fn a_plain_message_yields_no_rsvp() {
        let raw = b"Subject: hello\r\n\r\nplain text\r\n";
        assert!(
            rsvp_parts(
                raw,
                "me@example.com",
                antiphon_render::Rsvp::Decline,
                NOON
            )
            .is_none()
        );
    }
}
