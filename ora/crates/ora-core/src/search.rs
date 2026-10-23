//! Spotify search: the first page of each kind for a query, then further pages of one kind.
//!
//! Rules:
//! - Queries are trimmed, at most 200 characters and without control characters; the cache
//!   compares them without case, as Spotify does.
//! - A newer search cancels an older one still waiting for Spotify: its request is dropped and
//!   it returns `Cancelled`, which the UI ignores. Token refreshes are never cancelled.
//! - Results are cached per sign-in and account for 3 minutes, and identical reads in flight
//!   are shared.
//! - Paging stops at Spotify's limit of 1,000 results per kind.

use crate::{
    api::{self, ApiError},
    browser::Browser,
    cache::Cache,
    http::HttpClient,
    store::CredentialStore,
    Ora,
};
use ora_types::{Feature, OraError, SearchKind, SearchPage, SearchResults};
use std::time::Duration;
use tokio::sync::watch;

const SEARCH_TTL: Duration = Duration::from_secs(3 * 60);
pub const MAX_QUERY_CHARS: usize = 200;

pub struct Search {
    results: Cache<SearchResults>,
    pages: Cache<SearchPage>,
    /// Bumped by every new search, so older ones still waiting give up.
    latest: watch::Sender<u64>,
}

impl Default for Search {
    fn default() -> Self {
        Self {
            results: Cache::new(SEARCH_TTL),
            pages: Cache::new(SEARCH_TTL),
            latest: watch::Sender::new(0),
        }
    }
}

fn clean(query: &str) -> Result<&str, OraError> {
    let query = query.trim();
    if query.is_empty()
        || query.chars().count() > MAX_QUERY_CHARS
        || query.chars().any(char::is_control)
    {
        return Err(OraError::Unsupported);
    }
    Ok(query)
}

impl<H: HttpClient, S: CredentialStore, B: Browser> Ora<H, S, B> {
    /// The first page of each kind. A newer search returns `Cancelled` from this one.
    pub async fn search(&self, query: &str) -> Result<SearchResults, OraError> {
        self.require(Feature::Search)?;
        let query = clean(query)?;
        let latest = &self.search_state().latest;
        let mut mine = 0;
        latest.send_modify(|value| {
            *value += 1;
            mine = *value;
        });
        let newer = latest.subscribe();
        self.first_pages(query, Some((newer, mine))).await
    }

    /// One page of one kind; offset 0 is the overview's page.
    pub async fn search_page(
        &self,
        query: &str,
        kind: SearchKind,
        offset: u32,
    ) -> Result<SearchPage, OraError> {
        self.require(Feature::Search)?;
        let query = clean(query)?;
        if offset.saturating_add(api::SEARCH_PAGE) > api::SEARCH_LIMIT {
            return Err(OraError::Unsupported);
        }
        if offset == 0 {
            return Ok(self.first_pages(query, None).await?.page(kind).clone());
        }
        let http = self.http();
        let key = format!("{}:{offset}:{}", kind.name(), query.to_lowercase());
        self.search_state()
            .pages
            .get(&self.cache_scope(), &key, || {
                self.authorised(|access| async move {
                    api::search_more(http, &access, query, kind, offset).await
                })
            })
            .await
    }

    /// `until` is a search's place in line: a newer search drops its request.
    async fn first_pages(
        &self,
        query: &str,
        until: Option<(watch::Receiver<u64>, u64)>,
    ) -> Result<SearchResults, OraError> {
        let http = self.http();
        self.search_state()
            .results
            .get(&self.cache_scope(), &query.to_lowercase(), || {
                self.authorised(|access| {
                    let until = until.clone();
                    async move {
                        let request = api::search(http, &access, query);
                        let Some((mut newer, mine)) = until else {
                            return request.await;
                        };
                        tokio::select! {
                            result = request => result,
                            _ = newer.wait_for(|value| *value != mine) => {
                                Err(ApiError::Other(OraError::Cancelled))
                            }
                        }
                    }
                })
            })
            .await
    }
}
