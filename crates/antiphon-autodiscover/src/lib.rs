//! Thunderbird-style mail autoconfig discovery: given an e-mail
//! address, try the provider's own autoconfig endpoints and the
//! Mozilla ISPDB, parse the returned `clientConfig` XML, and hand
//! back the IMAP and SMTP settings to prefill an account form.
//!
//! Every network call goes through the [`Fetcher`] seam so tests
//! inject canned responses and never touch the network.

mod candidates;
mod fetch;
mod parse;

#[cfg(any(feature = "stub", test))]
pub mod stub;

pub use fetch::HttpFetcher;

/// How a server expects the connection to be secured, mapped from
/// autoconfig's `socketType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Security {
    /// Implicit TLS from the first byte (autoconfig `SSL`).
    Ssl,
    /// Plain connection upgraded with STARTTLS.
    Starttls,
    /// No transport security (autoconfig `plain`).
    Plain,
}

/// One end of the account: the host to reach, the port, how it is
/// secured, and the login name (autoconfig placeholders already
/// resolved against the address).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerSettings {
    pub host: String,
    pub port: u16,
    pub security: Security,
    pub username: String,
}

/// The settings a lookup resolved: the incoming (IMAP) and
/// outgoing (SMTP) servers, plus the provider's display name when
/// the config carried one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Discovered {
    pub provider: Option<String>,
    pub imap: Option<ServerSettings>,
    pub smtp: Option<ServerSettings>,
}

impl Discovered {
    fn is_empty(&self) -> bool {
        self.imap.is_none() && self.smtp.is_none()
    }
}

/// Why a lookup could not even be attempted. A reachable server
/// that simply has no config is not an error: `discover` returns
/// `Ok(None)` for that, distinct from these failures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiscoverError {
    /// The address has no `@`, so there is no domain to query.
    BadAddress,
}

/// Fetches an autoconfig document, returning its body on success
/// and `None` when the endpoint does not serve one (a 404, a
/// missing host, or an unreachable network). The trait is the
/// test seam: the real [`HttpFetcher`] makes HTTPS requests, the
/// stub answers from a table.
pub trait Fetcher {
    fn fetch(&self, url: &str)
    -> Result<Option<String>, DiscoverError>;
}

/// Tries each candidate endpoint in precedence order (the
/// provider's own autoconfig first, the Mozilla ISPDB last),
/// returning the first document that parses into servers. A
/// reachable-but-empty result is `Ok(None)`.
pub fn discover<F: Fetcher>(
    email: &str,
    fetcher: &F,
) -> Result<Option<Discovered>, DiscoverError> {
    let domain = domain_of(email).ok_or(DiscoverError::BadAddress)?;
    for url in candidates::urls(email, domain) {
        let Some(body) = fetcher.fetch(&url)? else {
            continue;
        };
        let found = parse::config(&body, email);
        if !found.is_empty() {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

fn domain_of(email: &str) -> Option<&str> {
    let domain = email.rsplit_once('@')?.1;
    let trimmed = domain.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

#[cfg(test)]
mod tests;
