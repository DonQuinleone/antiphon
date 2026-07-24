use lettre::address::{Address, Envelope};
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::{SmtpTransport, Transport};

use crate::error::SyncError;

const AUTH_MECHANISMS: [Mechanism; 2] =
    [Mechanism::Plain, Mechanism::Login];

#[derive(Clone, Debug)]
pub struct SmtpAccount {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
}

/// Submits a fully formed RFC 5322 message over SMTP with
/// mandatory STARTTLS on the configured port. The envelope is
/// derived from the message's own From, To, Cc and Bcc headers.
pub fn send(
    account: &SmtpAccount,
    raw_message: &[u8],
) -> Result<(), SyncError> {
    let envelope = envelope_for(raw_message)?;
    let transport = SmtpTransport::starttls_relay(&account.host)
        .map_err(SyncError::smtp(&account.host))?
        .port(account.port)
        .credentials(Credentials::new(
            account.user.clone(),
            account.password.clone(),
        ))
        .authentication(AUTH_MECHANISMS.to_vec())
        .build();
    transport
        .send_raw(&envelope, raw_message)
        .map_err(SyncError::smtp(&account.host))?;
    Ok(())
}

fn envelope_for(raw: &[u8]) -> Result<Envelope, SyncError> {
    let mut sender = None;
    let mut recipients = Vec::new();
    for (name, value) in header_fields(raw) {
        if name.eq_ignore_ascii_case("from") {
            let mut addresses = parse_addresses(&value)?;
            if sender.is_none() && !addresses.is_empty() {
                sender = Some(addresses.remove(0));
            }
        } else if is_recipient_header(&name) {
            recipients.extend(parse_addresses(&value)?);
        }
    }
    Envelope::new(sender, recipients).map_err(|source| {
        SyncError::SmtpMessage {
            detail: source.to_string(),
        }
    })
}

fn is_recipient_header(name: &str) -> bool {
    ["to", "cc", "bcc"]
        .iter()
        .any(|header| name.eq_ignore_ascii_case(header))
}

/// Returns the unfolded header fields of the message, stopping
/// at the blank line that opens the body.
fn header_fields(raw: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(raw);
    let mut fields: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            break;
        }
        if line.starts_with([' ', '\t']) {
            if let Some((_, value)) = fields.last_mut() {
                value.push(' ');
                value.push_str(line.trim_start());
            }
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        fields.push((name.trim().to_owned(), value.trim().to_owned()));
    }
    fields
}

fn parse_addresses(value: &str) -> Result<Vec<Address>, SyncError> {
    split_mailboxes(value)
        .iter()
        .filter(|mailbox| !mailbox.is_empty())
        .map(|mailbox| {
            let spec = addr_spec(mailbox);
            spec.parse().map_err(|_| SyncError::SmtpMessage {
                detail: format!("invalid address `{spec}`"),
            })
        })
        .collect()
}

/// Splits an address header into individual mailboxes on the
/// commas that sit outside quoted strings and angle brackets,
/// so display names like "Doe, Jane" survive intact.
fn split_mailboxes(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut bracketed = false;
    for ch in value.chars() {
        match ch {
            '"' => quoted = !quoted,
            '<' if !quoted => bracketed = true,
            '>' if !quoted => bracketed = false,
            ',' if !quoted && !bracketed => {
                parts.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(ch);
    }
    parts.push(current);
    parts
        .into_iter()
        .map(|part| part.trim().to_owned())
        .collect()
}

fn addr_spec(mailbox: &str) -> &str {
    let Some(start) = mailbox.rfind('<') else {
        return mailbox.trim();
    };
    let after = &mailbox[start + 1..];
    match after.find('>') {
        Some(end) => after[..end].trim(),
        None => after.trim(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipient_strings(envelope: &Envelope) -> Vec<String> {
        envelope
            .to()
            .iter()
            .map(|address| address.to_string())
            .collect()
    }

    #[test]
    fn envelope_reads_from_and_all_recipient_headers() {
        let message = b"From: A <a@example.com>\r\n\
            To: b@example.com, C <c@example.com>\r\n\
            Cc: d@example.com\r\n\
            Bcc: e@example.com\r\n\
            Subject: hello\r\n\
            \r\n\
            body\r\n";
        let envelope = envelope_for(message).unwrap();
        assert_eq!(
            envelope.from().unwrap().to_string(),
            "a@example.com"
        );
        assert_eq!(
            recipient_strings(&envelope),
            [
                "b@example.com",
                "c@example.com",
                "d@example.com",
                "e@example.com",
            ]
        );
    }

    #[test]
    fn quoted_display_name_commas_do_not_split() {
        let mailboxes = split_mailboxes(
            "\"Doe, Jane\" <jane@example.com>, b@example.com",
        );
        assert_eq!(
            mailboxes,
            ["\"Doe, Jane\" <jane@example.com>", "b@example.com"]
        );
    }

    #[test]
    fn folded_recipient_headers_unfold() {
        let message = b"From: a@example.com\r\n\
            To: b@example.com,\r\n\
            \tc@example.com\r\n\
            \r\n\
            body\r\n";
        let envelope = envelope_for(message).unwrap();
        assert_eq!(
            recipient_strings(&envelope),
            ["b@example.com", "c@example.com"]
        );
    }

    #[test]
    fn body_lines_never_reach_the_envelope() {
        let message = b"From: a@example.com\r\n\
            To: b@example.com\r\n\
            \r\n\
            To: smuggled@example.com\r\n";
        let envelope = envelope_for(message).unwrap();
        assert_eq!(recipient_strings(&envelope), ["b@example.com"]);
    }

    #[test]
    fn a_message_without_recipients_is_rejected() {
        let message = b"From: a@example.com\r\n\
            Subject: nothing\r\n\
            \r\n\
            body\r\n";
        let error = envelope_for(message).unwrap_err();
        assert!(matches!(error, SyncError::SmtpMessage { .. }));
    }

    #[test]
    fn an_unparseable_address_is_rejected() {
        let message = b"From: a@example.com\r\n\
            To: not-an-address\r\n\
            \r\n\
            body\r\n";
        let error = envelope_for(message).unwrap_err();
        assert!(matches!(error, SyncError::SmtpMessage { .. }));
    }

    #[test]
    fn bare_angle_addresses_extract_the_spec() {
        assert_eq!(
            addr_spec("Jane <jane@example.com>"),
            "jane@example.com"
        );
        assert_eq!(addr_spec("jane@example.com"), "jane@example.com");
    }
}
