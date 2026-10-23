//! The single list of user-facing features. Melora's UI maps each variant, so adding a variant
//! here fails `npm run typecheck` until the UI handles it.

use serde::{Deserialize, Serialize};

/// A user-facing capability. Deliberately exhaustive (never `#[non_exhaustive]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum Feature {
    SignIn,
    Profile,
    EmbeddedPlayback,
    ConnectControl,
    PlayPause,
    SkipNextPrevious,
    Seek,
    Volume,
    Shuffle,
    Repeat,
    NowPlaying,
    DevicePicker,
    TransferPlayback,
    Queue,
    AddToQueue,
    Search,
    SavedTracks,
    SavedAlbums,
    SavedShowsEpisodes,
    LibrarySaveRemove,
    Playlists,
    PlaylistDetail,
    PlaylistEdit,
    CreatePlaylist,
    RecentlyPlayed,
    OpenInSpotify,
    MediaKeys,
}

const MODIFY_PLAYBACK: &[&str] = &["user-modify-playback-state"];
const READ_PLAYLISTS: &[&str] = &["playlist-read-private", "playlist-read-collaborative"];
const MODIFY_PLAYLISTS: &[&str] = &["playlist-modify-public", "playlist-modify-private"];

impl Feature {
    /// Every variant, in declaration order.
    pub const ALL: [Feature; 27] = [
        Feature::SignIn,
        Feature::Profile,
        Feature::EmbeddedPlayback,
        Feature::ConnectControl,
        Feature::PlayPause,
        Feature::SkipNextPrevious,
        Feature::Seek,
        Feature::Volume,
        Feature::Shuffle,
        Feature::Repeat,
        Feature::NowPlaying,
        Feature::DevicePicker,
        Feature::TransferPlayback,
        Feature::Queue,
        Feature::AddToQueue,
        Feature::Search,
        Feature::SavedTracks,
        Feature::SavedAlbums,
        Feature::SavedShowsEpisodes,
        Feature::LibrarySaveRemove,
        Feature::Playlists,
        Feature::PlaylistDetail,
        Feature::PlaylistEdit,
        Feature::CreatePlaylist,
        Feature::RecentlyPlayed,
        Feature::OpenInSpotify,
        Feature::MediaKeys,
    ];

    /// OAuth scopes the feature's endpoints declare in Spotify's OpenAPI `security` blocks.
    pub const fn scopes(self) -> &'static [&'static str] {
        match self {
            Feature::SignIn | Feature::Search | Feature::OpenInSpotify => &[],
            Feature::Profile => &["user-read-private"],
            Feature::EmbeddedPlayback => &[
                "streaming",
                "user-read-playback-state",
                "user-modify-playback-state",
                "user-read-currently-playing",
            ],
            Feature::ConnectControl => &["user-read-playback-state", "user-modify-playback-state"],
            Feature::PlayPause
            | Feature::SkipNextPrevious
            | Feature::Seek
            | Feature::Volume
            | Feature::Shuffle
            | Feature::Repeat
            | Feature::TransferPlayback
            | Feature::AddToQueue
            | Feature::MediaKeys => MODIFY_PLAYBACK,
            Feature::NowPlaying | Feature::Queue => {
                &["user-read-currently-playing", "user-read-playback-state"]
            }
            Feature::DevicePicker => &["user-read-playback-state"],
            Feature::SavedTracks | Feature::SavedAlbums => &["user-library-read"],
            Feature::SavedShowsEpisodes => &["user-library-read", "user-read-playback-position"],
            Feature::LibrarySaveRemove => &["user-library-read", "user-library-modify"],
            Feature::Playlists | Feature::PlaylistDetail => READ_PLAYLISTS,
            Feature::PlaylistEdit | Feature::CreatePlaylist => MODIFY_PLAYLISTS,
            Feature::RecentlyPlayed => &["user-read-recently-played"],
        }
    }

    /// Spotify only accepts these from Premium accounts.
    pub const fn needs_premium(self) -> bool {
        matches!(
            self,
            Feature::EmbeddedPlayback
                | Feature::ConnectControl
                | Feature::PlayPause
                | Feature::SkipNextPrevious
                | Feature::Seek
                | Feature::Volume
                | Feature::Shuffle
                | Feature::Repeat
                | Feature::TransferPlayback
                | Feature::AddToQueue
                | Feature::MediaKeys
        )
    }

    /// Needs audio playing inside the app itself (the Web Playback SDK).
    pub const fn needs_tier_a(self) -> bool {
        matches!(self, Feature::EmbeddedPlayback)
    }

    /// Development Mode limits worth showing to the user, if any.
    pub const fn dev_mode_note(self) -> Option<&'static str> {
        match self {
            Feature::SignIn => Some(
                "Each Spotify account must be on the app's User Management allowlist (up to 5).",
            ),
            Feature::PlaylistDetail => {
                Some("Spotify only lists the contents of playlists you own or collaborate on.")
            }
            Feature::Queue => Some("Spotify doesn't allow reordering the queue from other apps."),
            Feature::Search => Some("Spotify returns at most 10 results per type per page."),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use ts_rs::TS;

    #[test]
    fn all_lists_every_variant_once_in_order() {
        // The generated union names every variant, so a variant missing from ALL is caught here.
        let union = Feature::inline(&ts_rs::Config::from_env());
        let variants: Vec<&str> = union
            .split('|')
            .map(|v| v.trim().trim_matches('"'))
            .collect();
        assert_eq!(variants.len(), Feature::ALL.len());
        for (name, feature) in variants.iter().zip(Feature::ALL) {
            assert_eq!(serde_json::to_value(feature).ok(), Some((*name).into()));
        }
        assert_eq!(
            Feature::ALL.iter().collect::<BTreeSet<_>>().len(),
            Feature::ALL.len()
        );
    }

    #[test]
    fn scopes_are_known_and_unique() {
        let known = [
            "streaming",
            "user-read-private",
            "user-read-playback-state",
            "user-modify-playback-state",
            "user-read-currently-playing",
            "user-read-playback-position",
            "user-read-recently-played",
            "user-library-read",
            "user-library-modify",
            "playlist-read-private",
            "playlist-read-collaborative",
            "playlist-modify-public",
            "playlist-modify-private",
        ];
        for feature in Feature::ALL {
            let scopes = feature.scopes();
            assert!(scopes.iter().all(|s| known.contains(s)), "{feature:?}");
            assert_eq!(scopes.iter().collect::<BTreeSet<_>>().len(), scopes.len());
        }
    }

    #[test]
    fn premium_and_embedded_rules_are_consistent() {
        for feature in Feature::ALL {
            if feature.needs_tier_a() {
                assert!(feature.needs_premium(), "{feature:?}");
                assert!(feature.scopes().contains(&"streaming"));
            }
            // Controlling playback is Premium-only, so every playback-modifying feature says so.
            if feature.scopes().contains(&"user-modify-playback-state") {
                assert!(feature.needs_premium(), "{feature:?}");
            }
        }
    }
}
