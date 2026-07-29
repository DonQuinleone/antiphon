use crate::stub::MapFetcher;
use crate::{
    DiscoverError, Security, discover, domain_of,
};

const ADDRESS: &str = "ada@example.com";
const ISPDB_URL: &str =
    "https://autoconfig.thunderbird.net/v1.1/example.com";
const PROVIDER_URL: &str = "https://autoconfig.example.com/mail/\
     config-v1.1.xml?emailaddress=ada@example.com";

fn config(imap_host: &str, smtp_host: &str) -> String {
    format!(
        r#"<?xml version="1.0"?>
<clientConfig version="1.1">
  <emailProvider id="example.com">
    <domain>example.com</domain>
    <displayName>Example Mail</displayName>
    <incomingServer type="imap">
      <hostname>{imap_host}</hostname>
      <port>993</port>
      <socketType>SSL</socketType>
      <username>%EMAILADDRESS%</username>
    </incomingServer>
    <incomingServer type="pop3">
      <hostname>pop.example.com</hostname>
      <port>995</port>
      <socketType>SSL</socketType>
    </incomingServer>
    <outgoingServer type="smtp">
      <hostname>{smtp_host}</hostname>
      <port>587</port>
      <socketType>STARTTLS</socketType>
      <username>%EMAILLOCALPART%</username>
    </outgoingServer>
  </emailProvider>
</clientConfig>"#
    )
}

#[test]
fn the_ispdb_answers_when_the_provider_has_no_config() {
    let fetcher = MapFetcher::new().with(
        ISPDB_URL,
        &config("imap.example.com", "smtp.example.com"),
    );
    let found = discover(ADDRESS, &fetcher).unwrap().unwrap();
    let imap = found.imap.unwrap();
    assert_eq!(imap.host, "imap.example.com");
    assert_eq!(imap.port, 993);
    assert_eq!(imap.security, Security::Ssl);
    assert_eq!(imap.username, ADDRESS);
    let smtp = found.smtp.unwrap();
    assert_eq!(smtp.host, "smtp.example.com");
    assert_eq!(smtp.port, 587);
    assert_eq!(smtp.security, Security::Starttls);
    assert_eq!(smtp.username, "ada");
    assert_eq!(found.provider.as_deref(), Some("Example Mail"));
}

#[test]
fn the_provider_config_beats_the_ispdb() {
    let fetcher = MapFetcher::new()
        .with(
            PROVIDER_URL,
            &config("imap.own.example", "smtp.own.example"),
        )
        .with(ISPDB_URL, &config("imap.db.example", "smtp.db.example"));
    let found = discover(ADDRESS, &fetcher).unwrap().unwrap();
    assert_eq!(found.imap.unwrap().host, "imap.own.example");
}

#[test]
fn pop3_blocks_are_ignored_in_favour_of_imap() {
    let fetcher = MapFetcher::new().with(
        ISPDB_URL,
        &config("imap.example.com", "smtp.example.com"),
    );
    let found = discover(ADDRESS, &fetcher).unwrap().unwrap();
    assert_eq!(found.imap.unwrap().host, "imap.example.com");
}

#[test]
fn a_missing_port_falls_back_to_the_security_default() {
    let body = r#"<clientConfig version="1.1">
  <emailProvider id="example.com">
    <incomingServer type="imap">
      <hostname>imap.example.com</hostname>
      <socketType>STARTTLS</socketType>
    </incomingServer>
  </emailProvider>
</clientConfig>"#;
    let fetcher = MapFetcher::new().with(ISPDB_URL, body);
    let found = discover(ADDRESS, &fetcher).unwrap().unwrap();
    assert_eq!(found.imap.unwrap().port, 143);
}

#[test]
fn nothing_anywhere_is_a_clean_not_found() {
    let fetcher = MapFetcher::new();
    assert_eq!(discover(ADDRESS, &fetcher).unwrap(), None);
}

#[test]
fn an_addressless_input_is_rejected() {
    let fetcher = MapFetcher::new();
    assert_eq!(
        discover("not-an-address", &fetcher),
        Err(DiscoverError::BadAddress)
    );
}

#[test]
fn the_domain_is_the_part_after_the_at() {
    assert_eq!(domain_of("ada@example.com"), Some("example.com"));
    assert_eq!(domain_of("bare"), None);
}
