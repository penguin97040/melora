//! Spotify search results as the UI lists them. Spotify IDs here are public catalogue IDs; no
//! tokens or raw server data.

use serde::{Deserialize, Serialize};

/// The kinds of result Melora searches for. Shows and audiobooks wait for views to open them in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum SearchKind {
    Track,
    Artist,
    Album,
    Playlist,
    Episode,
}

impl SearchKind {
    pub const ALL: [SearchKind; 5] = [
        SearchKind::Track,
        SearchKind::Artist,
        SearchKind::Album,
        SearchKind::Playlist,
        SearchKind::Episode,
    ];

    /// The Web API's `type` value.
    pub fn name(self) -> &'static str {
        match self {
            SearchKind::Track => "track",
            SearchKind::Artist => "artist",
            SearchKind::Album => "album",
            SearchKind::Playlist => "playlist",
            SearchKind::Episode => "episode",
        }
    }
}

/// One result. Fields a kind doesn't have are empty.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct SearchItem {
    /// The result's place in all of Spotify's results for its kind.
    pub position: u32,
    /// `spotify:<kind>:<id>`.
    pub uri: String,
    pub name: String,
    /// A track's or album's artists, or a playlist's owner.
    pub by: Vec<String>,
    /// A track's album.
    pub album: String,
    /// An album's or episode's release date: `YYYY`, `YYYY-MM` or `YYYY-MM-DD`.
    pub release_date: String,
    /// An album's tracks, or a playlist's tracks and episodes.
    pub total: Option<u32>,
    pub duration_ms: Option<u64>,
    /// `false` when Spotify says it can't be played in the account's country.
    pub playable: bool,
    pub spotify_url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct SearchPage {
    pub items: Vec<SearchItem>,
    /// How many results Spotify has; Melora pages through the first 1,000.
    pub total: u32,
    pub next_offset: Option<u32>,
}

/// The first page of each kind for one query.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct SearchResults {
    pub tracks: SearchPage,
    pub artists: SearchPage,
    pub albums: SearchPage,
    pub playlists: SearchPage,
    pub episodes: SearchPage,
}

impl SearchResults {
    pub fn page(&self, kind: SearchKind) -> &SearchPage {
        match kind {
            SearchKind::Track => &self.tracks,
            SearchKind::Artist => &self.artists,
            SearchKind::Album => &self.albums,
            SearchKind::Playlist => &self.playlists,
            SearchKind::Episode => &self.episodes,
        }
    }
}
