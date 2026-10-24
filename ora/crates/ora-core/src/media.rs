//! The system's media controls: media keys, Windows' media overlay and Linux's MPRIS. This
//! module decides what they show and what each key does; `ora-tauri` wires them to the OS.
//!
//! Rules:
//! - A key does what the player's own button would: the same `PlayerCommand`, so it reaches
//!   this app's SDK or the active remote device in the same way.
//! - While an episode plays, Next and Previous skip 15 seconds, like the player's buttons.
//! - A control Spotify disallows right now does nothing.

use crate::link::{self, Kind};
use ora_types::{PlaybackStatus, PlayerCommand};

/// How far Next and Previous skip in an episode (the player's −15 s / +15 s).
pub const SKIP_MS: u64 = 15_000;

/// A key or button press from the system's media controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKey {
    PlayPause,
    Play,
    Pause,
    Stop,
    Next,
    Previous,
    /// Relative, in milliseconds (MPRIS `Seek`).
    SeekBy(i64),
    /// Absolute, in milliseconds (MPRIS `SetPosition`).
    SetPosition(u64),
}

/// What the system's media controls show for the item playing now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaInfo {
    /// The item's base62 Spotify ID, when its link is known.
    pub id: Option<String>,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    /// Album artwork, already limited to the image host the app allows.
    pub image_url: Option<String>,
    pub episode: bool,
    pub playing: bool,
    pub position_ms: u64,
    pub updated_at_ms: u64,
    pub duration_ms: u64,
    pub can_play: bool,
    pub can_pause: bool,
    pub can_next: bool,
    pub can_previous: bool,
    pub can_seek: bool,
}

impl MediaInfo {
    /// The position at `now_ms`, moving on from the last report while playing.
    pub fn position_at(&self, now_ms: u64) -> u64 {
        let position = if self.playing {
            self.position_ms
                .saturating_add(now_ms.saturating_sub(self.updated_at_ms))
        } else {
            self.position_ms
        };
        if self.duration_ms > 0 {
            position.min(self.duration_ms)
        } else {
            position
        }
    }

    /// Whether `next` jumped within the same item, rather than playing on (MPRIS `Seeked`).
    pub fn jumped_from(&self, before: &MediaInfo) -> bool {
        const TOLERANCE_MS: u64 = 1_500;
        let expected = before.position_at(self.updated_at_ms);
        self.id == before.id && self.position_ms.abs_diff(expected) > TOLERANCE_MS
    }
}

/// What to show, or `None` when nothing plays on any device.
pub fn media_info(status: &PlaybackStatus) -> Option<MediaInfo> {
    let now = status.now_playing.as_ref()?;
    let item = now.spotify_url.as_deref().and_then(link::parse);
    let episode = item.as_ref().is_some_and(|item| item.kind == Kind::Episode);
    let disallows = status.disallows;
    let can_seek = !disallows.seek && status.duration_ms > 0;
    Some(MediaInfo {
        id: item.map(|item| item.id().to_owned()),
        title: now.name.clone(),
        artists: now.artists.clone(),
        album: now.album.clone(),
        image_url: now.image_url.clone(),
        episode,
        playing: !status.paused,
        position_ms: status.position_ms,
        updated_at_ms: status.updated_at_ms,
        duration_ms: status.duration_ms,
        can_play: !disallows.resume,
        can_pause: !disallows.pause,
        can_next: if episode { can_seek } else { !disallows.next },
        can_previous: if episode {
            can_seek
        } else {
            !disallows.previous
        },
        can_seek,
    })
}

/// The command a key sends at `now_ms`, or `None` when it should do nothing.
pub fn command(status: &PlaybackStatus, key: MediaKey, now_ms: u64) -> Option<PlayerCommand> {
    let info = media_info(status)?;
    let position = info.position_at(now_ms);
    let seek = |to: u64| {
        info.can_seek.then(|| PlayerCommand::Seek {
            position_ms: to.min(info.duration_ms),
        })
    };
    match key {
        MediaKey::PlayPause if info.playing => info.can_pause.then_some(PlayerCommand::TogglePlay),
        MediaKey::PlayPause | MediaKey::Play => {
            (!info.playing && info.can_play).then_some(PlayerCommand::TogglePlay)
        }
        MediaKey::Pause | MediaKey::Stop => {
            (info.playing && info.can_pause).then_some(PlayerCommand::TogglePlay)
        }
        MediaKey::Next if info.episode => seek(position.saturating_add(SKIP_MS)),
        MediaKey::Previous if info.episode => seek(position.saturating_sub(SKIP_MS)),
        MediaKey::Next => info.can_next.then_some(PlayerCommand::Next),
        MediaKey::Previous => info.can_previous.then_some(PlayerCommand::Previous),
        MediaKey::SeekBy(offset) => seek(position.saturating_add_signed(offset)),
        MediaKey::SetPosition(to) => seek(to),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ora_types::{Disallows, NowPlaying};

    const ID: &str = "AbCd0123456789xyzXYZ01";

    fn playing(kind: &str, paused: bool) -> PlaybackStatus {
        PlaybackStatus {
            now_playing: Some(NowPlaying {
                name: "Song".into(),
                artists: vec!["One".into(), "Two".into()],
                album: "Album".into(),
                spotify_url: Some(format!("https://open.spotify.com/{kind}/{ID}")),
                image_url: None,
                liked: None,
            }),
            paused,
            position_ms: 60_000,
            duration_ms: 200_000,
            updated_at_ms: 1_000,
            ..PlaybackStatus::default()
        }
    }

    #[test]
    fn nothing_playing_shows_nothing_and_keys_do_nothing() {
        let status = PlaybackStatus::default();
        assert_eq!(media_info(&status), None);
        assert_eq!(command(&status, MediaKey::PlayPause, 0), None);
    }

    #[test]
    fn a_track_shows_what_plays_and_keys_press_the_players_buttons() {
        let status = playing("track", false);
        let info = media_info(&status).unwrap();
        assert_eq!(info.id.as_deref(), Some(ID));
        assert_eq!(info.artists, ["One", "Two"]);
        assert!(info.playing && !info.episode && info.can_next && info.can_seek);
        assert_eq!(info.position_at(3_000), 62_000);
        assert_eq!(info.position_at(10_000_000), 200_000);
        let key = |key| command(&status, key, 3_000);
        assert_eq!(key(MediaKey::PlayPause), Some(PlayerCommand::TogglePlay));
        assert_eq!(key(MediaKey::Pause), Some(PlayerCommand::TogglePlay));
        assert_eq!(key(MediaKey::Stop), Some(PlayerCommand::TogglePlay));
        assert_eq!(key(MediaKey::Play), None, "already playing");
        assert_eq!(key(MediaKey::Next), Some(PlayerCommand::Next));
        assert_eq!(key(MediaKey::Previous), Some(PlayerCommand::Previous));
        assert_eq!(
            key(MediaKey::SeekBy(-70_000)),
            Some(PlayerCommand::Seek { position_ms: 0 })
        );
        assert_eq!(
            key(MediaKey::SetPosition(500_000)),
            Some(PlayerCommand::Seek {
                position_ms: 200_000
            })
        );
        let paused = playing("track", true);
        assert_eq!(
            command(&paused, MediaKey::Play, 3_000),
            Some(PlayerCommand::TogglePlay)
        );
        assert_eq!(command(&paused, MediaKey::Pause, 3_000), None);
        assert_eq!(media_info(&paused).unwrap().position_at(9_000), 60_000);
    }

    #[test]
    fn controls_spotify_disallows_do_nothing() {
        let mut status = playing("track", false);
        status.disallows = Disallows {
            pause: true,
            resume: false,
            next: true,
            previous: true,
            seek: true,
        };
        let info = media_info(&status).unwrap();
        assert!(!info.can_pause && !info.can_next && !info.can_previous && !info.can_seek);
        for key in [
            MediaKey::PlayPause,
            MediaKey::Pause,
            MediaKey::Next,
            MediaKey::Previous,
            MediaKey::SeekBy(5_000),
            MediaKey::SetPosition(0),
        ] {
            assert_eq!(command(&status, key, 1_000), None, "{key:?}");
        }
    }

    #[test]
    fn in_an_episode_next_and_previous_skip_15_seconds() {
        let status = playing("episode", true);
        let info = media_info(&status).unwrap();
        assert!(info.episode && info.can_next && info.can_previous);
        assert_eq!(
            command(&status, MediaKey::Next, 5_000),
            Some(PlayerCommand::Seek {
                position_ms: 75_000
            })
        );
        assert_eq!(
            command(&status, MediaKey::Previous, 5_000),
            Some(PlayerCommand::Seek {
                position_ms: 45_000
            })
        );
        let mut near_end = playing("episode", false);
        near_end.position_ms = 195_000;
        assert_eq!(
            command(&near_end, MediaKey::Next, 1_000),
            Some(PlayerCommand::Seek {
                position_ms: 200_000
            })
        );
        near_end.position_ms = 5_000;
        assert_eq!(
            command(&near_end, MediaKey::Previous, 1_000),
            Some(PlayerCommand::Seek { position_ms: 0 })
        );
        // Skipping is seeking: without it, Next and Previous do nothing, even if allowed.
        near_end.disallows.seek = true;
        assert!(!media_info(&near_end).unwrap().can_next);
        assert_eq!(command(&near_end, MediaKey::Next, 1_000), None);
    }

    #[test]
    fn a_seek_is_a_jump_but_playing_on_or_a_new_item_is_not() {
        let before = media_info(&playing("track", false)).unwrap();
        let mut on = before.clone();
        on.updated_at_ms = 11_000;
        on.position_ms = 70_200;
        assert!(!on.jumped_from(&before));
        on.position_ms = 120_000;
        assert!(on.jumped_from(&before));
        on.id = Some("ZbCd0123456789xyzXYZ01".into());
        assert!(!on.jumped_from(&before));
    }
}
