//! The live [`Fetcher`](crate::Fetcher): blocking HTTPS requests
//! with a short timeout. A missing autoconfig subdomain, a 404 or
//! an unreachable network are all "no config here", reported as
//! `Ok(None)` so discovery moves on to the next candidate rather
//! than failing outright.

use std::time::Duration;

use reqwest::blocking::Client;

use crate::{DiscoverError, Fetcher};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

pub struct HttpFetcher {
    client: Client,
}

impl HttpFetcher {
    pub fn new() -> HttpFetcher {
        let client = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap_or_default();
        HttpFetcher { client }
    }
}

impl Default for HttpFetcher {
    fn default() -> HttpFetcher {
        HttpFetcher::new()
    }
}

impl Fetcher for HttpFetcher {
    fn fetch(
        &self,
        url: &str,
    ) -> Result<Option<String>, DiscoverError> {
        let Ok(response) = self.client.get(url).send() else {
            return Ok(None);
        };
        if !response.status().is_success() {
            return Ok(None);
        }
        Ok(response.text().ok())
    }
}
