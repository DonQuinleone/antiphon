//! A [`Fetcher`](crate::Fetcher) that answers from an in-memory
//! table, for tests that must never touch the network. Enabled by
//! the `stub` feature so downstream test suites can drive
//! discovery with canned autoconfig documents.

use std::collections::HashMap;

use crate::{DiscoverError, Fetcher};

#[derive(Default)]
pub struct MapFetcher {
    bodies: HashMap<String, String>,
}

impl MapFetcher {
    pub fn new() -> MapFetcher {
        MapFetcher::default()
    }

    /// Serves `body` when `url` is requested; every other URL
    /// answers `None`, as a real endpoint without config would.
    pub fn with(mut self, url: &str, body: &str) -> MapFetcher {
        self.bodies.insert(url.to_string(), body.to_string());
        self
    }
}

impl Fetcher for MapFetcher {
    fn fetch(
        &self,
        url: &str,
    ) -> Result<Option<String>, DiscoverError> {
        Ok(self.bodies.get(url).cloned())
    }
}
