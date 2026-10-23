//! The account's playlists and Liked Songs, as the UI lists them, and what it asks to play.
//! Spotify IDs here are public catalogue IDs (they appear in every share link); no tokens,
//! device IDs or raw server data.

use serde::{Deserialize, Serialize};

/// A playlist in the account's list (`GET /me/playlists`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct PlaylistSummary {
    /// The 22-character Spotify ID.
    pub id: String,
    pub name: String,
    /// The owner's display name, or empty when Spotify doesn't give one.
    pub owner: String,
    /// How many tracks and episodes it holds.
    pub total: u32,
    pub image_url: Option<String>,
    /// Its `open.spotify.com` page, for "Open in Spotify".
    pub spotify_url: String,
    /// Spotify lists the items only of playlists the account owns or collaborates on.
    pub readable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct PlaylistPage {
    pub items: Vec<PlaylistSummary>,
    pub total: u32,
    /// Where the next page starts, or `None` after the last one.
    pub next_offset: Option<u32>,
}

/// A row in a track list: a playlist's items, Liked Songs, an album's tracks, a show's episodes
/// or saved episodes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct TrackRow {
    /// The row's place in the whole list, counting rows Spotify left out, so playing it starts
    /// at the right item even when a track appears twice.
    pub position: u32,
    /// `spotify:track:…` or `spotify:episode:…`; `None` for local files and items Spotify no
    /// longer has, which Melora can't play.
    pub uri: Option<String>,
    pub name: String,
    pub artists: Vec<String>,
    /// The album, or the show for an episode; empty in an album's or show's own list.
    pub album: String,
    /// An episode's release date (`YYYY`, `YYYY-MM` or `YYYY-MM-DD`); empty for tracks.
    pub release_date: String,
    pub duration_ms: u64,
    pub spotify_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct TrackPage {
    pub items: Vec<TrackRow>,
    pub total: u32,
    pub next_offset: Option<u32>,
}

/// What plays now and what Spotify plays after it (`GET /me/player/queue`). Spotify lets apps
/// read the queue and add to it, but not reorder or remove from it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct QueueView {
    pub current: Option<TrackRow>,
    /// In the order they'll play; `position` counts from 0 after `current`.
    pub next: Vec<TrackRow>,
}

/// What a click in a list starts playing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum PlayRequest {
    /// A playlist or album, from the row at `position` or from the start, or an artist (their
    /// top tracks, from the start only).
    Context { uri: String, position: Option<u32> },
    /// Tracks or episodes in this order. Liked Songs has no context Spotify lets apps play.
    Items { uris: Vec<String> },
}
