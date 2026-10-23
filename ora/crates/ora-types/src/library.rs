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

/// A row in a track list: a playlist's items or Liked Songs.
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
    /// The album, or the show for an episode.
    pub album: String,
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

/// What a click in a list starts playing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum PlayRequest {
    /// A playlist (`spotify:playlist:…`), from the row at `position`, or from the start.
    Context { uri: String, position: Option<u32> },
    /// Tracks or episodes in this order. Liked Songs has no context Spotify lets apps play.
    Items { uris: Vec<String> },
}
