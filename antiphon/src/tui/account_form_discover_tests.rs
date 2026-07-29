use antiphon_autodiscover::stub::MapFetcher;

use super::*;
use crate::tui::account_form_fields::AccountType;
use crate::tui::testkit::app_with_messages;

const ISPDB_URL: &str =
    "https://autoconfig.thunderbird.net/v1.1/example.com";

const CONFIG: &str = r#"<clientConfig version="1.1">
  <emailProvider id="example.com">
    <displayName>Example Mail</displayName>
    <incomingServer type="imap">
      <hostname>imap.example.com</hostname>
      <port>993</port>
      <socketType>SSL</socketType>
      <username>%EMAILADDRESS%</username>
    </incomingServer>
    <outgoingServer type="smtp">
      <hostname>smtp.example.com</hostname>
      <port>587</port>
      <socketType>STARTTLS</socketType>
      <username>%EMAILADDRESS%</username>
    </outgoingServer>
  </emailProvider>
</clientConfig>"#;

fn app_with_address(address: &str) -> App {
    let mut app = app_with_messages(1);
    app.open_account_form_add();
    let form = app.account_form.as_mut().expect("form open");
    form.address = address.to_string();
    form.error = None;
    app
}

#[test]
fn discovery_fills_the_imap_and_smtp_rows() {
    let mut app = app_with_address("ada@example.com");
    let fetcher = MapFetcher::new().with(ISPDB_URL, CONFIG);
    run_with(&mut app, &fetcher);
    let form = app.account_form.as_ref().unwrap();
    assert_eq!(form.imap_host, "imap.example.com");
    assert_eq!(form.imap_user, "ada@example.com");
    assert_eq!(form.smtp_host, "smtp.example.com");
    assert_eq!(form.error, None);
}

#[test]
fn a_miss_reports_where_it_looked() {
    let mut app = app_with_address("ada@example.com");
    run_with(&mut app, &MapFetcher::new());
    let form = app.account_form.as_ref().unwrap();
    assert!(form.imap_host.is_empty());
    let error = form.error.as_deref().unwrap_or_default();
    assert!(error.contains("example.com"), "{error:?}");
}

#[test]
fn an_addressless_form_is_told_to_fill_the_address() {
    let mut app = app_with_address("nobody");
    run_with(&mut app, &MapFetcher::new().with(ISPDB_URL, CONFIG));
    let form = app.account_form.as_ref().unwrap();
    assert!(form.imap_host.is_empty());
    assert!(form.error.as_deref().unwrap().contains("e-mail address"));
}

#[test]
fn an_oauth_account_is_declined_since_its_servers_are_fixed() {
    let mut app = app_with_address("ada@example.com");
    app.account_form.as_mut().unwrap().account_type =
        AccountType::Google;
    run_with(&mut app, &MapFetcher::new().with(ISPDB_URL, CONFIG));
    let form = app.account_form.as_ref().unwrap();
    assert!(form.imap_host.is_empty());
    assert!(form.error.as_deref().unwrap().contains("fixed servers"));
}
