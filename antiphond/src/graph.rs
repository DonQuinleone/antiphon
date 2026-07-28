use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

const GRAPH_BASE: &str = "https://graph.microsoft.com/v1.0";
const MIME_CONTENT_TYPE: &str = "text/plain";
const SEND_TIMEOUT: Duration = Duration::from_secs(60);

/// Delegated tokens send as the signed-in user; app-only
/// tokens have no user and must name the sending mailbox.
pub(crate) fn sendmail_url(app_only_sender: Option<&str>) -> String {
    match app_only_sender {
        None => format!("{GRAPH_BASE}/me/sendMail"),
        Some(sender) => {
            format!("{GRAPH_BASE}/users/{sender}/sendMail")
        }
    }
}

use crate::mailflow::ShipError;

/// Ships an assembled RFC 5322 message through Microsoft
/// Graph: the raw MIME goes base64-encoded to /me/sendMail
/// and only 202 Accepted counts as sent. Graph files the
/// Sent Items copy itself. Only a 400 (malformed message) is
/// permanent; auth and transport failures retry.
pub(crate) fn send_raw(
    token: &str,
    url: &str,
    raw: &[u8],
) -> Result<(), ShipError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(SEND_TIMEOUT)
        .build()
        .map_err(ShipError::transient)?;
    let response = client
        .post(url)
        .bearer_auth(token)
        .header("Content-Type", MIME_CONTENT_TYPE)
        .body(mime_body(raw))
        .send()
        .map_err(ShipError::transient)?;
    let status = response.status();
    if status != reqwest::StatusCode::ACCEPTED {
        let detail = response.text().unwrap_or_default();
        return Err(ShipError {
            detail: format!("graph answered {status}: {detail}"),
            permanent: status == reqwest::StatusCode::BAD_REQUEST,
        });
    }
    Ok(())
}

/// Graph resolves recipients from the MIME headers alone, and
/// the stored message keeps Bcc out of them by design, so the
/// upload splices the envelope's undisclosed recipients back
/// in as a Bcc header; Exchange consumes that header at
/// submission instead of delivering it.
pub(crate) fn with_envelope_bcc(
    raw: &[u8],
    recipients: &[String],
) -> Vec<u8> {
    let disclosed = header_text(raw).to_lowercase();
    let hidden: Vec<&str> = recipients
        .iter()
        .map(String::as_str)
        .filter(|address| {
            !disclosed.contains(address.to_lowercase().as_str())
        })
        .collect();
    if hidden.is_empty() {
        return raw.to_vec();
    }
    let mut upload =
        format!("Bcc: {}\r\n", hidden.join(", ")).into_bytes();
    upload.extend_from_slice(raw);
    upload
}

fn header_text(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let end = text
        .find("\r\n\r\n")
        .or_else(|| text.find("\n\n"))
        .unwrap_or(text.len());
    text[..end].to_string()
}

fn mime_body(raw: &[u8]) -> String {
    STANDARD.encode(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW: &[u8] = b"From: quin@example.com\r\n\
        To: mara@example.com\r\n\
        Cc: bram@example.com\r\n\
        Subject: hello\r\n\
        \r\n\
        body\r\n";

    #[test]
    fn undisclosed_recipients_are_spliced_in_as_bcc() {
        let recipients = [
            "mara@example.com".to_string(),
            "bram@example.com".to_string(),
            "hidden@example.com".to_string(),
        ];
        let upload = with_envelope_bcc(RAW, &recipients);
        let text = String::from_utf8(upload).unwrap();
        assert!(text.starts_with("Bcc: hidden@example.com\r\n"));
        assert!(text.ends_with("body\r\n"));
    }

    #[test]
    fn disclosed_recipients_leave_the_message_untouched() {
        let recipients = [
            "mara@example.com".to_string(),
            "bram@example.com".to_string(),
        ];
        assert_eq!(with_envelope_bcc(RAW, &recipients), RAW);
    }

    #[test]
    fn a_bcc_matching_a_body_address_still_splices() {
        let raw = b"From: quin@example.com\r\n\
            To: mara@example.com\r\n\
            \r\n\
            write to hidden@example.com\r\n";
        let recipients = ["hidden@example.com".to_string()];
        let upload = with_envelope_bcc(raw, &recipients);
        assert!(upload.starts_with(b"Bcc: hidden@example.com\r\n"));
    }

    #[test]
    fn the_body_is_plain_base64_of_the_raw_message() {
        let raw = b"Subject: hi\r\n\r\nbody\r\n";
        let body = mime_body(raw);
        assert_eq!(
            STANDARD.decode(body.as_bytes()).unwrap(),
            raw.to_vec()
        );
        assert!(!body.contains('\n'), "single unwrapped token");
    }
}
