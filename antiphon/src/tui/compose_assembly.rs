//! Turning a validated compose into bytes on the wire: the
//! parsed `Outgoing` header set, the address helpers, and the
//! one place a message is assembled. All App-free, so it sits
//! apart from `ComposeState`.

use antiphon_render::{Draft, build_message};
use antiphon_store::Envelope;

use super::attach::Attachment;

/// The X-Mailer every outgoing message carries; the version is the
/// single build-time string, never a hardcoded copy.
const MAILER: &str = concat!("Antiphon ", env!("ANTIPHON_VERSION"));

/// A compose validated and ready to assemble: parsed address
/// lists and the exact header values the message will carry.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Outgoing {
    pub from_name: Option<String>,
    pub from: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub body: String,
    pub read_receipt: bool,
}

/// Header lists keep what you typed, display names included,
/// split with the same quote-aware rules the harvester uses;
/// only the envelope reduces entries to bare addresses.
pub(super) fn address_list(value: &str) -> Vec<String> {
    antiphon_store::contacts::address_entries(value)
        .into_iter()
        .map(|(address, name)| match name.is_empty() {
            true => address,
            false => format!("{name} <{address}>"),
        })
        .collect()
}

pub(super) fn bare_address(value: &str) -> String {
    let bracketed = value
        .split_once('<')
        .and_then(|(_, rest)| rest.split_once('>'))
        .map(|(inner, _)| inner);
    bracketed.unwrap_or(value).trim().to_string()
}

/// The one place an outgoing message is assembled; Bcc
/// recipients ride the envelope only, never the headers, and
/// attachments make it multipart/mixed.
pub(super) fn assemble(
    outgoing: &Outgoing,
    attachments: &[Attachment],
    date_unix: i64,
) -> Vec<u8> {
    let (_, domain) = outgoing
        .from
        .rsplit_once('@')
        .expect("validated in outgoing");
    let draft = Draft {
        from_name: outgoing.from_name.as_deref(),
        from: &outgoing.from,
        to: as_strs(&outgoing.to),
        cc: as_strs(&outgoing.cc),
        subject: &outgoing.subject,
        in_reply_to: outgoing.in_reply_to.as_deref(),
        references: as_strs(&outgoing.references),
        body: &outgoing.body,
        signature: None,
        attachments: attachments
            .iter()
            .map(Attachment::as_part)
            .collect(),
        read_receipt: outgoing.read_receipt,
    };
    build_message(&draft, domain, date_unix, MAILER)
}

pub(super) fn envelope(account: &str, outgoing: &Outgoing) -> Envelope {
    Envelope {
        account: account.to_string(),
        from: outgoing.from.clone(),
        recipients: outgoing
            .to
            .iter()
            .chain(&outgoing.cc)
            .chain(&outgoing.bcc)
            .map(|entry| bare_address(entry))
            .collect(),
        send_after: None,
    }
}

fn as_strs(values: &[String]) -> Vec<&str> {
    values.iter().map(String::as_str).collect()
}
