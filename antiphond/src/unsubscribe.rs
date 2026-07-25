use std::time::Duration;

const POST_TIMEOUT: Duration = Duration::from_secs(30);
const ONE_CLICK_BODY: &str = "List-Unsubscribe=One-Click";
const FORM_TYPE: &str = "application/x-www-form-urlencoded";

/// RFC 8058 insists on https; anything else is refused before
/// a connection is even attempted.
pub(crate) fn validate(url: &str) -> Result<(), String> {
    if url.starts_with("https://") {
        return Ok(());
    }
    Err("one-click unsubscribe requires an https url".to_string())
}

/// Fires the POST off the serve loop; the outcome lands in the
/// daemon log either way.
pub(crate) fn spawn_post(url: String) {
    std::thread::spawn(move || match post(&url) {
        Ok(()) => println!("unsubscribed: POST {url}"),
        Err(error) => eprintln!("unsubscribe {url}: {error}"),
    });
}

fn post(url: &str) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(POST_TIMEOUT)
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .post(url)
        .header("Content-Type", FORM_TYPE)
        .body(ONE_CLICK_BODY)
        .send()
        .map_err(|error| error.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("server answered {status}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_urls_pass_validation() {
        assert!(validate("https://lists.example.com/u/7").is_ok());
        for bad in [
            "http://lists.example.com/u/7",
            "mailto:leave@example.com",
            "ftp://example.com",
            "",
        ] {
            assert!(validate(bad).is_err(), "{bad}");
        }
    }
}
