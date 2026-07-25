use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

const SENDMAIL_URL: &str =
    "https://graph.microsoft.com/v1.0/me/sendMail";
const MIME_CONTENT_TYPE: &str = "text/plain";
const SEND_TIMEOUT: Duration = Duration::from_secs(60);

/// Ships an assembled RFC 5322 message through Microsoft
/// Graph: the raw MIME goes base64-encoded to /me/sendMail
/// and only 202 Accepted counts as sent. Graph files the
/// Sent Items copy itself.
pub(crate) fn send_raw(token: &str, raw: &[u8]) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(SEND_TIMEOUT)
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .post(SENDMAIL_URL)
        .bearer_auth(token)
        .header("Content-Type", MIME_CONTENT_TYPE)
        .body(mime_body(raw))
        .send()
        .map_err(|error| error.to_string())?;
    let status = response.status();
    if status != reqwest::StatusCode::ACCEPTED {
        let detail = response.text().unwrap_or_default();
        return Err(format!("graph answered {status}: {detail}"));
    }
    Ok(())
}

fn mime_body(raw: &[u8]) -> String {
    STANDARD.encode(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

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
