//! The account's playlists and Liked Songs, read through the Web API and cached briefly.
//!
//! Rules:
//! - Each list needs its feature's scopes; `NeedsScope` asks the UI to request them.
//! - Pages are cached per sign-in and account (`cache`), playlists for 5 minutes and Liked
//!   Songs for 1, and identical reads in flight are shared.
//! - Liking or unliking a track forgets the cached Liked Songs.

use crate::{
    api, browser::Browser, cache::Cache, http::HttpClient, link::is_base62_id,
    store::CredentialStore, Ora,
};
use ora_types::{Feature, OraError, PlaylistPage, TrackPage};
use std::time::Duration;

const PLAYLIST_TTL: Duration = Duration::from_secs(5 * 60);
const LIKED_TTL: Duration = Duration::from_secs(60);
/// Spotify's largest documented offset for these lists.
const MAX_OFFSET: u32 = 100_000;

pub struct Library {
    playlists: Cache<PlaylistPage>,
    items: Cache<TrackPage>,
    liked: Cache<TrackPage>,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            playlists: Cache::new(PLAYLIST_TTL),
            items: Cache::new(PLAYLIST_TTL),
            liked: Cache::new(LIKED_TTL),
        }
    }
}

impl Library {
    /// After Liked Songs changed.
    pub(crate) fn forget_liked(&self) {
        self.liked.clear();
    }
}

/// Offsets come from a previous page (a short page can end anywhere); anything past Spotify's
/// limit is refused before a request.
fn page_offset(offset: u32) -> Result<u32, OraError> {
    if offset <= MAX_OFFSET {
        Ok(offset)
    } else {
        Err(OraError::Unsupported)
    }
}

impl<H: HttpClient, S: CredentialStore, B: Browser> Ora<H, S, B> {
    /// One page of the account's playlists, 50 at a time.
    pub async fn playlists(&self, offset: u32) -> Result<PlaylistPage, OraError> {
        self.require(Feature::Playlists)?;
        let offset = page_offset(offset)?;
        let me = self.account();
        let (http, me) = (self.http(), me.as_deref());
        self.library()
            .playlists
            .get(&self.cache_scope(), &offset.to_string(), || {
                self.authorised(|access| async move {
                    api::my_playlists(http, &access, offset, me).await
                })
            })
            .await
    }

    /// One page of a playlist's tracks and episodes.
    pub async fn playlist_tracks(&self, id: &str, offset: u32) -> Result<TrackPage, OraError> {
        self.require(Feature::PlaylistDetail)?;
        let offset = page_offset(offset)?;
        if !is_base62_id(id) {
            return Err(OraError::Unsupported);
        }
        let http = self.http();
        self.library()
            .items
            .get(&self.cache_scope(), &format!("{id}:{offset}"), || {
                self.authorised(|access| async move {
                    api::playlist_items(http, &access, id, offset).await
                })
            })
            .await
    }

    /// One page of Liked Songs, newest first.
    pub async fn liked_tracks(&self, offset: u32) -> Result<TrackPage, OraError> {
        self.require(Feature::SavedTracks)?;
        let offset = page_offset(offset)?;
        let http = self.http();
        self.library()
            .liked
            .get(&self.cache_scope(), &offset.to_string(), || {
                self.authorised(
                    |access| async move { api::saved_tracks(http, &access, offset).await },
                )
            })
            .await
    }
}
