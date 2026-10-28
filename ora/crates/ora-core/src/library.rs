//! The account's library — playlists, Liked Songs, saved albums, shows and episodes — and the
//! albums and shows opened from it, read through the Web API and cached briefly.
//!
//! Rules:
//! - Each list needs its feature's scopes; `NeedsScope` asks the UI to request them. An album's
//!   tracks need only a sign-in.
//! - Pages are cached per sign-in and account (`cache`): playlists and show episodes for 5
//!   minutes, album tracks for 30, saved lists for 1. Identical reads in flight are shared.
//! - Whether items are saved is asked 40 at a time (Spotify's limit) and remembered for a
//!   minute; saving and removing also go 40 at a time. A change forgets the saved lists it
//!   affects and updates the player's `+` when it's the track playing now.
//! - The queue is read on demand (kept 2 seconds, so identical reads share one request, and
//!   forgotten after an addition); recently played is kept 30 seconds.
//! - Playlists the account owns or collaborates on can be changed: items added (100 per
//!   request), removed (every copy of a URI, 100 per request) or moved one at a time, and new
//!   private playlists created. Any change forgets the cached playlists and their items, even
//!   when a later batch fails.

use crate::{
    api,
    browser::Browser,
    cache::Cache,
    http::HttpClient,
    link::{self, is_base62_id, Kind, SpotifyUri},
    store::CredentialStore,
    Ora,
};
use ora_types::{
    CatalogPage, Feature, OraError, PlaylistPage, PlaylistSummary, QueueView, TrackPage,
};
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

const PLAYLIST_TTL: Duration = Duration::from_secs(5 * 60);
const ALBUM_TTL: Duration = Duration::from_secs(30 * 60);
const SAVED_TTL: Duration = Duration::from_secs(60);
const QUEUE_TTL: Duration = Duration::from_secs(2);
const RECENT_TTL: Duration = Duration::from_secs(30);
/// One request from the UI covers at most this many items; Melora splits it into batches.
pub const MAX_ITEMS: usize = 500;
/// More remembered states than this and the memory starts again.
const MAX_STATES: usize = 5_000;

#[derive(Default)]
struct SavedStates {
    scope: String,
    states: HashMap<String, (bool, Instant)>,
    /// Counts saves, removals and forgets. A `contains` answer for an item written after its
    /// request started is out of date and dropped.
    writes: u64,
    written: HashMap<String, u64>,
}

impl SavedStates {
    /// Starts again for another scope, or when full.
    fn enter(&mut self, scope: &str) {
        if self.scope != scope || self.states.len() >= MAX_STATES {
            self.scope = scope.to_owned();
            self.states.clear();
            self.written.clear();
        }
    }

    fn mark_written(&mut self, uri: String) {
        self.writes += 1;
        self.written.insert(uri, self.writes);
    }
}

pub struct Library {
    playlists: Cache<PlaylistPage>,
    items: Cache<TrackPage>,
    liked: Cache<TrackPage>,
    albums: Cache<CatalogPage>,
    album_tracks: Cache<TrackPage>,
    shows: Cache<CatalogPage>,
    show_episodes: Cache<TrackPage>,
    episodes: Cache<TrackPage>,
    queue: Cache<QueueView>,
    recent: Cache<TrackPage>,
    saved: Mutex<SavedStates>,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            playlists: Cache::new(PLAYLIST_TTL),
            items: Cache::new(PLAYLIST_TTL),
            liked: Cache::new(SAVED_TTL),
            albums: Cache::new(SAVED_TTL),
            album_tracks: Cache::new(ALBUM_TTL),
            shows: Cache::new(SAVED_TTL),
            show_episodes: Cache::new(PLAYLIST_TTL),
            episodes: Cache::new(SAVED_TTL),
            queue: Cache::new(QUEUE_TTL),
            recent: Cache::new(RECENT_TTL),
            saved: Mutex::default(),
        }
    }
}

impl Library {
    /// After Liked Songs changed.
    pub(crate) fn forget_liked(&self) {
        self.liked.clear();
    }

    /// After something was added to the queue.
    pub(crate) fn forget_queue(&self) {
        self.queue.clear();
    }

    /// After a playlist was created or changed: the list's counts and its items.
    fn forget_playlists(&self) {
        self.playlists.clear();
        self.items.clear();
    }

    fn states(&self) -> std::sync::MutexGuard<'_, SavedStates> {
        self.saved
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// What's known about `item` in this scope, while it's fresh.
    fn known(&self, scope: &str, item: &SpotifyUri) -> Option<bool> {
        let states = self.states();
        if states.scope != scope {
            return None;
        }
        states
            .states
            .get(&item.uri())
            .filter(|(_, at)| at.elapsed() < SAVED_TTL)
            .map(|(saved, _)| *saved)
    }

    /// After this app saved or removed `item`.
    pub(crate) fn remember_saved(&self, scope: &str, item: &SpotifyUri, saved: bool) {
        let mut states = self.states();
        states.enter(scope);
        states.mark_written(item.uri());
        states.states.insert(item.uri(), (saved, Instant::now()));
    }

    /// Where the writes stand before a `contains` request, for `remember_answer`.
    pub(crate) fn write_mark(&self) -> u64 {
        self.states().writes
    }

    /// Spotify's `contains` answer for `item`, asked at `mark`. Returns what to show: the
    /// answer, or the newer state if `item` was saved, removed or forgotten meanwhile.
    pub(crate) fn remember_answer(
        &self,
        scope: &str,
        item: &SpotifyUri,
        saved: bool,
        mark: u64,
    ) -> bool {
        let mut states = self.states();
        states.enter(scope);
        let uri = item.uri();
        if states.written.get(&uri).is_some_and(|at| *at > mark) {
            return states.states.get(&uri).map_or(saved, |(newer, _)| *newer);
        }
        states.states.insert(uri, (saved, Instant::now()));
        saved
    }

    /// After a failed save or removal, when nobody knows which way it went: ask Spotify again.
    fn forget_saved(&self, items: &[SpotifyUri]) {
        let mut states = self.states();
        for item in items {
            states.states.remove(&item.uri());
            states.mark_written(item.uri());
        }
    }

    /// After items of these kinds were saved or removed.
    fn forget_lists(&self, kinds: impl IntoIterator<Item = Kind>) {
        for kind in kinds {
            match kind {
                Kind::Track => self.liked.clear(),
                Kind::Album => self.albums.clear(),
                Kind::Show => self.shows.clear(),
                Kind::Episode => self.episodes.clear(),
                Kind::Artist | Kind::Playlist => {}
            }
        }
    }
}

/// Offsets come from a previous page (a short page can end anywhere); anything past Spotify's
/// limit is refused before a request.
fn page_offset(offset: u32) -> Result<u32, OraError> {
    if offset <= api::MAX_OFFSET {
        Ok(offset)
    } else {
        Err(OraError::Unsupported)
    }
}

fn catalogue_id(id: &str) -> Result<&str, OraError> {
    if is_base62_id(id) {
        Ok(id)
    } else {
        Err(OraError::Unsupported)
    }
}

/// Tracks, albums, shows and episodes, the kinds the library holds; duplicates go.
fn library_items(uris: &[String]) -> Result<Vec<SpotifyUri>, OraError> {
    if uris.is_empty() || uris.len() > MAX_ITEMS {
        return Err(OraError::Unsupported);
    }
    let mut items: Vec<SpotifyUri> = Vec::with_capacity(uris.len());
    for uri in uris {
        let item = link::parse(uri)
            .filter(|item| {
                matches!(
                    item.kind,
                    Kind::Track | Kind::Album | Kind::Show | Kind::Episode
                )
            })
            .ok_or(OraError::InvalidSpotifyLink)?;
        if !items.contains(&item) {
            items.push(item);
        }
    }
    Ok(items)
}

/// Spotify's limit for a playlist's name is undocumented; Melora keeps names to this.
pub const MAX_NAME: usize = 100;

/// A new playlist's name: trimmed, not empty, no control characters.
fn playlist_name(name: &str) -> Result<&str, OraError> {
    let name = name.trim();
    let count = name.chars().count();
    if (1..=MAX_NAME).contains(&count) && !name.chars().any(char::is_control) {
        Ok(name)
    } else {
        Err(OraError::Unsupported)
    }
}

/// Tracks and episodes, the kinds a playlist holds, in the order given.
fn playlist_entries(uris: &[String]) -> Result<Vec<SpotifyUri>, OraError> {
    if uris.is_empty() || uris.len() > MAX_ITEMS {
        return Err(OraError::Unsupported);
    }
    uris.iter()
        .map(|uri| {
            link::parse(uri)
                .filter(|item| matches!(item.kind, Kind::Track | Kind::Episode))
                .ok_or(OraError::InvalidSpotifyLink)
        })
        .collect()
}

/// Spotify's `insert_before` for moving the item at `from` so it ends up at `to`.
fn insert_before(from: u32, to: u32) -> u32 {
    if to > from {
        to + 1
    } else {
        to
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
        let id = catalogue_id(id)?;
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

    /// Creates a private playlist; it appears first in the account's list.
    pub async fn create_playlist(&self, name: &str) -> Result<PlaylistSummary, OraError> {
        self.require(Feature::CreatePlaylist)?;
        let name = playlist_name(name)?;
        let me = self.account();
        let (http, me) = (self.http(), me.as_deref());
        let result = self
            .authorised(|access| async move { api::create_playlist(http, &access, name, me).await })
            .await;
        self.library().forget_playlists();
        result
    }

    /// Adds tracks and episodes to the end of a playlist, in order (duplicates too).
    pub async fn add_to_playlist(&self, id: &str, uris: &[String]) -> Result<(), OraError> {
        self.require(Feature::PlaylistEdit)?;
        let id = catalogue_id(id)?;
        let items = playlist_entries(uris)?;
        let http = self.http();
        for batch in items.chunks(api::PLAYLIST_BATCH) {
            let result = self
                .authorised(|access| async move {
                    api::add_playlist_items(http, &access, id, batch).await
                })
                .await;
            self.library().forget_playlists();
            result?;
        }
        Ok(())
    }

    /// Removes every copy of these tracks and episodes from a playlist.
    pub async fn remove_from_playlist(&self, id: &str, uris: &[String]) -> Result<(), OraError> {
        self.require(Feature::PlaylistEdit)?;
        let id = catalogue_id(id)?;
        let mut items = playlist_entries(uris)?;
        let mut seen = std::collections::HashSet::new();
        items.retain(|item| seen.insert(item.uri()));
        let http = self.http();
        for batch in items.chunks(api::PLAYLIST_BATCH) {
            let result = self
                .authorised(|access| async move {
                    api::remove_playlist_items(http, &access, id, batch).await
                })
                .await;
            self.library().forget_playlists();
            result?;
        }
        Ok(())
    }

    /// Moves the item at position `from` so it ends up at position `to`.
    pub async fn move_in_playlist(&self, id: &str, from: u32, to: u32) -> Result<(), OraError> {
        self.require(Feature::PlaylistEdit)?;
        let id = catalogue_id(id)?;
        let (from, to) = (page_offset(from)?, page_offset(to)?);
        if from == to {
            return Ok(());
        }
        let http = self.http();
        let before = insert_before(from, to);
        let result = self
            .authorised(|access| async move {
                api::move_playlist_item(http, &access, id, from, before).await
            })
            .await;
        self.library().forget_playlists();
        result
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

    /// One page of saved albums, newest first.
    pub async fn saved_albums(&self, offset: u32) -> Result<CatalogPage, OraError> {
        self.require(Feature::SavedAlbums)?;
        let offset = page_offset(offset)?;
        let http = self.http();
        self.library()
            .albums
            .get(&self.cache_scope(), &offset.to_string(), || {
                self.authorised(
                    |access| async move { api::saved_albums(http, &access, offset).await },
                )
            })
            .await
    }

    /// One page of saved shows (podcasts).
    pub async fn saved_shows(&self, offset: u32) -> Result<CatalogPage, OraError> {
        self.require(Feature::SavedShowsEpisodes)?;
        let offset = page_offset(offset)?;
        let http = self.http();
        self.library()
            .shows
            .get(&self.cache_scope(), &offset.to_string(), || {
                self.authorised(
                    |access| async move { api::saved_shows(http, &access, offset).await },
                )
            })
            .await
    }

    /// One page of saved episodes, newest first.
    pub async fn saved_episodes(&self, offset: u32) -> Result<TrackPage, OraError> {
        self.require(Feature::SavedShowsEpisodes)?;
        let offset = page_offset(offset)?;
        let http = self.http();
        self.library()
            .episodes
            .get(&self.cache_scope(), &offset.to_string(), || {
                self.authorised(
                    |access| async move { api::saved_episodes(http, &access, offset).await },
                )
            })
            .await
    }

    /// One page of an album's tracks. Catalogue reads need only a sign-in.
    pub async fn album_tracks(&self, id: &str, offset: u32) -> Result<TrackPage, OraError> {
        self.require(Feature::SignIn)?;
        let offset = page_offset(offset)?;
        let id = catalogue_id(id)?;
        let http = self.http();
        self.library()
            .album_tracks
            .get(&self.cache_scope(), &format!("{id}:{offset}"), || {
                self.authorised(|access| async move {
                    api::album_tracks(http, &access, id, offset).await
                })
            })
            .await
    }

    /// One page of a show's episodes, newest first. Spotify asks for the playback-position
    /// scope, which comes with saved shows and episodes.
    pub async fn show_episodes(&self, id: &str, offset: u32) -> Result<TrackPage, OraError> {
        self.require(Feature::SavedShowsEpisodes)?;
        let offset = page_offset(offset)?;
        let id = catalogue_id(id)?;
        let http = self.http();
        self.library()
            .show_episodes
            .get(&self.cache_scope(), &format!("{id}:{offset}"), || {
                self.authorised(|access| async move {
                    api::show_episodes(http, &access, id, offset).await
                })
            })
            .await
    }

    /// What plays now and next on the active device.
    pub async fn queue(&self) -> Result<QueueView, OraError> {
        self.require(Feature::Queue)?;
        let http = self.http();
        self.library()
            .queue
            .get(&self.cache_scope(), "queue", || {
                self.authorised(|access| async move { api::queue(http, &access).await })
            })
            .await
    }

    /// The tracks played most recently, newest first.
    pub async fn recently_played(&self) -> Result<TrackPage, OraError> {
        self.require(Feature::RecentlyPlayed)?;
        let http = self.http();
        self.library()
            .recent
            .get(&self.cache_scope(), "recent", || {
                self.authorised(|access| async move { api::recently_played(http, &access).await })
            })
            .await
    }

    /// Whether each item is saved (a track: in Liked Songs), in the order asked.
    pub async fn saved_states(&self, uris: &[String]) -> Result<Vec<bool>, OraError> {
        self.require(Feature::LibrarySaveRemove)?;
        let items = library_items(uris)?;
        let scope = self.cache_scope();
        let library = self.library();
        let mut answers: HashMap<String, bool> = HashMap::new();
        let mut missing = Vec::new();
        for item in items {
            match library.known(&scope, &item) {
                Some(saved) => {
                    answers.insert(item.uri(), saved);
                }
                None => missing.push(item),
            }
        }
        let http = self.http();
        for batch in missing.chunks(api::LIBRARY_BATCH) {
            let mark = library.write_mark();
            let saved = self
                .authorised(|access| async move {
                    api::library_contains_many(http, &access, batch).await
                })
                .await?;
            for (item, saved) in batch.iter().zip(saved) {
                let saved = library.remember_answer(&scope, item, saved, mark);
                self.playback().set_liked(item, saved);
                answers.insert(item.uri(), saved);
            }
        }
        // Every URI parsed in `library_items`, so each has an answer.
        Ok(uris
            .iter()
            .filter_map(|uri| link::parse(uri))
            .map(|item| answers.get(&item.uri()).copied().unwrap_or(false))
            .collect())
    }

    /// Saves the items to the library, or removes them.
    pub async fn set_saved(&self, uris: &[String], saved: bool) -> Result<(), OraError> {
        self.require(Feature::LibrarySaveRemove)?;
        let items = library_items(uris)?;
        let scope = self.cache_scope();
        let http = self.http();
        for batch in items.chunks(api::LIBRARY_BATCH) {
            let result = self
                .authorised(|access| async move {
                    api::library_set_many(http, &access, batch, saved).await
                })
                .await;
            // Batches already sent changed the library even if a later one fails.
            self.library()
                .forget_lists(batch.iter().map(|item| item.kind));
            if result.is_err() {
                self.library().forget_saved(batch);
            }
            result?;
            for item in batch {
                self.library().remember_saved(&scope, item, saved);
                self.playback().set_liked(item, saved);
            }
        }
        Ok(())
    }
}
