//! Melora's Spotify logic without any UI or Tauri dependency: configuration, sign-in, the Spotify
//! Web API client, playback and feature availability. The app wires it in through `ora-tauri`.

pub mod api;
pub mod browser;
pub mod cache;
pub mod config;
pub mod fsio;
pub mod http;
pub mod library;
pub mod link;
pub mod loopback;
pub mod oauth;
pub mod pkce;
pub mod playback;
pub mod search;
pub mod secret;
pub mod session;
pub mod store;

pub use ora_types as types;
pub use session::{Options, Ora};

use ora_types::Feature;

/// The OAuth scopes needed for a set of features, sorted and without duplicates, ready to be
/// joined into an authorisation request. Scopes are requested per slice, never all at once.
pub fn scopes_for(features: &[Feature]) -> Vec<&'static str> {
    let mut scopes: Vec<&'static str> = features.iter().flat_map(|f| f.scopes()).copied().collect();
    scopes.sort_unstable();
    scopes.dedup();
    scopes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_are_merged_sorted_and_unique() {
        assert_eq!(scopes_for(&[]), Vec::<&str>::new());
        assert_eq!(
            scopes_for(&[Feature::SignIn, Feature::Search]),
            Vec::<&str>::new()
        );
        assert_eq!(
            scopes_for(&[Feature::NowPlaying, Feature::DevicePicker, Feature::Queue]),
            ["user-read-currently-playing", "user-read-playback-state"]
        );
        assert_eq!(
            scopes_for(&[Feature::Profile, Feature::Playlists]),
            [
                "playlist-read-collaborative",
                "playlist-read-private",
                "user-read-private"
            ]
        );
    }
}
