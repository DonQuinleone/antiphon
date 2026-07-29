//! The autoconfig endpoints to try, in Thunderbird's precedence:
//! a provider that publishes its own config (on the mail domain
//! or under `.well-known`) is authoritative, so those come before
//! the central Mozilla ISPDB.

const ISPDB_BASE: &str = "https://autoconfig.thunderbird.net/v1.1/";

pub(crate) fn urls(email: &str, domain: &str) -> Vec<String> {
    vec![
        format!(
            "https://autoconfig.{domain}/mail/config-v1.1.xml\
             ?emailaddress={email}"
        ),
        format!(
            "https://{domain}/.well-known/autoconfig/mail/\
             config-v1.1.xml?emailaddress={email}"
        ),
        format!("{ISPDB_BASE}{domain}"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_endpoints_precede_the_ispdb() {
        let urls = urls("ada@example.com", "example.com");
        assert_eq!(urls.len(), 3);
        assert!(urls[0].starts_with("https://autoconfig.example.com/"));
        assert!(urls[1].contains("/.well-known/autoconfig/"));
        assert_eq!(
            urls[2],
            "https://autoconfig.thunderbird.net/v1.1/example.com"
        );
    }

    #[test]
    fn the_address_rides_along_for_provider_lookups() {
        let urls = urls("ada@example.com", "example.com");
        assert!(urls[0].ends_with("emailaddress=ada@example.com"));
        assert!(urls[1].ends_with("emailaddress=ada@example.com"));
    }
}
