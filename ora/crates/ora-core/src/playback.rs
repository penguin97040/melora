//! Embedded playback (Tier A): the state the shared SDK bridge reports, and the commands that
//! start playback on this device.
//!
//! Rules:
//! - Everything from the bridge is untrusted input: it is validated, trimmed and reduced to the
//!   fields `ora-types` defines, and anything malformed is dropped.
//! - The access token reaches the bridge only through `sdk_token`, for the SDK's
//!   `getOAuthToken`, and only once the sign-in covers `EmbeddedPlayback`.
//! - Progress is never polled: the UI interpolates `positionMs` from `updatedAtMs`.

use crate::{
    api::{self, is_allowed_image},
    browser::Browser,
    http::HttpClient,
    link::{self, Kind},
    secret::Secret,
    store::CredentialStore,
    Ora,
};
use ora_types::{
    Feature, NowPlaying, OraError, PlaybackIssue, PlaybackStatus, PlayerCommand, Probe,
    SdkErrorKind, SdkEvent, SdkState, Tier,
};
use std::{
    sync::{Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::watch;

const MAX_TEXT: usize = 300;
const MAX_ARTISTS: usize = 20;
const MAX_CSP_BLOCKS: usize = 10;
/// Longer than any track or episode, so a larger position is nonsense.
const MAX_POSITION_MS: u64 = 24 * 60 * 60 * 1000;

pub struct Playback {
    /// The SDK's device ID while Spotify knows this app as a device.
    device: Mutex<Option<String>>,
    status: watch::Sender<PlaybackStatus>,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            device: Mutex::new(None),
            status: watch::Sender::new(PlaybackStatus::default()),
        }
    }
}

impl Playback {
    pub fn status(&self) -> PlaybackStatus {
        self.status.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<PlaybackStatus> {
        self.status.subscribe()
    }

    fn device(&self) -> MutexGuard<'_, Option<String>> {
        self.device
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn device_id(&self) -> Option<String> {
        self.device().clone()
    }

    /// Whether the last probe found a DRM system, so the SDK may load (and its frame navigate).
    pub fn drm_available(&self) -> bool {
        self.status
            .borrow()
            .probe
            .as_ref()
            .is_some_and(Probe::any_drm)
    }

    /// Back to "not enabled", for example after sign-out or a disconnect.
    pub fn reset(&self) {
        *self.device() = None;
        self.status.send_modify(|status| {
            *status = PlaybackStatus {
                probe: status.probe.take(),
                csp_blocks: std::mem::take(&mut status.csp_blocks),
                ..PlaybackStatus::default()
            };
        });
    }

    /// Applies one bridge report. Malformed reports change nothing.
    pub fn apply(&self, event: SdkEvent) {
        let mut device = self.device();
        self.status.send_if_modified(|status| match event {
            SdkEvent::Probe { probe } => {
                let probe = clean_probe(probe);
                status.tier = if probe.any_drm() {
                    Tier::Connecting
                } else {
                    Tier::Unsupported
                };
                status.probe = Some(probe);
                status.issue = None;
                true
            }
            SdkEvent::Ready { device_id } if is_device_id(&device_id) => {
                *device = Some(device_id);
                status.tier = Tier::Ready;
                status.issue = None;
                true
            }
            SdkEvent::NotReady { device_id } if device.as_deref() == Some(&device_id) => {
                *device = None;
                status.tier = Tier::Connecting;
                true
            }
            SdkEvent::State { state: None } if device.is_some() => {
                status.tier = Tier::Ready;
                status.now_playing = None;
                status.paused = true;
                true
            }
            SdkEvent::State { state: Some(state) } if device.is_some() => {
                apply_state(status, state);
                true
            }
            SdkEvent::Error { error, message } => {
                if error == SdkErrorKind::Initialization {
                    *device = None;
                    status.tier = Tier::Unsupported;
                }
                status.issue = Some(PlaybackIssue {
                    kind: error,
                    message: clip(&message, MAX_TEXT),
                });
                true
            }
            SdkEvent::CspViolation { directive, origin } => {
                let entry = format!("{} {}", clip(&directive, 40), csp_origin(&origin));
                if status.csp_blocks.len() >= MAX_CSP_BLOCKS || status.csp_blocks.contains(&entry) {
                    return false;
                }
                status.csp_blocks.push(entry);
                true
            }
            _ => false,
        });
    }

    /// A UI command for the SDK, validated. Only `Disconnect` works without a ready device.
    pub fn command(&self, command: PlayerCommand) -> Result<PlayerCommand, OraError> {
        if command == PlayerCommand::Disconnect {
            self.reset();
            return Ok(command);
        }
        if self.device().is_none() {
            return Err(OraError::NoActiveDevice);
        }
        match command {
            PlayerCommand::Seek { position_ms } => {
                let duration = self.status.borrow().duration_ms;
                let limit = if duration > 0 {
                    duration
                } else {
                    MAX_POSITION_MS
                };
                Ok(PlayerCommand::Seek {
                    position_ms: position_ms.min(limit),
                })
            }
            PlayerCommand::SetVolume { volume } if volume.is_finite() => {
                Ok(PlayerCommand::SetVolume {
                    volume: volume.clamp(0.0, 1.0),
                })
            }
            PlayerCommand::SetVolume { .. } => Err(OraError::Unsupported),
            other => Ok(other),
        }
    }
}

impl<H: HttpClient, S: CredentialStore, B: Browser> Ora<H, S, B> {
    /// The access token for the SDK's `getOAuthToken`, and for nothing else.
    pub async fn sdk_token(&self) -> Result<Secret, OraError> {
        self.require(Feature::EmbeddedPlayback)?;
        self.fresh_access().await
    }

    /// A player command from the UI: shuffle and repeat run here through the Web API (the SDK
    /// has no methods for them); everything else is returned for the SDK to run.
    pub async fn player_command(
        &self,
        command: PlayerCommand,
    ) -> Result<Option<PlayerCommand>, OraError> {
        let command = self.playback().command(command)?;
        if !matches!(
            command,
            PlayerCommand::SetShuffle { .. } | PlayerCommand::SetRepeat { .. }
        ) {
            return Ok(Some(command));
        }
        self.require(Feature::EmbeddedPlayback)?;
        let device = self
            .playback()
            .device_id()
            .ok_or(OraError::NoActiveDevice)?;
        let device = device.as_str();
        let http = self.http();
        match command {
            PlayerCommand::SetShuffle { state } => {
                self.authorised(
                    |access| async move { api::shuffle(http, &access, device, state).await },
                )
                .await?
            }
            PlayerCommand::SetRepeat { mode } => {
                self.authorised(
                    |access| async move { api::repeat(http, &access, device, mode).await },
                )
                .await?
            }
            _ => {}
        }
        Ok(None)
    }

    /// Starts playback on this app's SDK device: the pasted link when there is one, otherwise
    /// whatever the account was last playing (transferred here).
    pub async fn play_here(&self, link: Option<&str>) -> Result<(), OraError> {
        self.require(Feature::EmbeddedPlayback)?;
        let target = match link.map(str::trim).filter(|link| !link.is_empty()) {
            Some(link) => Some(link::parse(link).ok_or(OraError::InvalidSpotifyLink)?),
            None => None,
        };
        let device = self
            .playback()
            .device_id()
            .ok_or(OraError::NoActiveDevice)?;
        let device = device.as_str();
        let http = self.http();
        match &target {
            // Playing with `device_id` targets the device directly; no transfer needed.
            Some(target) => {
                self.authorised(
                    |access| async move { api::play(http, &access, device, target).await },
                )
                .await
            }
            None => {
                self.authorised(
                    |access| async move { api::transfer(http, &access, device, true).await },
                )
                .await
            }
        }
    }
}

fn apply_state(status: &mut PlaybackStatus, state: SdkState) {
    status.tier = Tier::Active;
    status.paused = state.paused;
    status.duration_ms = state.duration_ms.min(MAX_POSITION_MS);
    status.position_ms = state.position_ms.min(status.duration_ms);
    status.updated_at_ms = now_ms();
    status.shuffle = state.shuffle;
    status.repeat = state.repeat;
    status.disallows = state.disallows;
    status.now_playing = state.track.and_then(|track| {
        let item = link::parse(&track.uri)?;
        if !matches!(item.kind, Kind::Track | Kind::Episode) {
            return None;
        }
        Some(NowPlaying {
            name: clip(&track.name, MAX_TEXT),
            artists: track
                .artists
                .iter()
                .take(MAX_ARTISTS)
                .map(|artist| clip(artist, MAX_TEXT))
                .filter(|artist| !artist.is_empty())
                .collect(),
            album: clip(&track.album, MAX_TEXT),
            spotify_url: Some(item.page()),
            image_url: track
                .image_url
                .filter(|url| url.len() <= 512 && is_allowed_image(url)),
        })
    });
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// At most `max` characters, without control characters.
fn clip(text: &str, max: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(max)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// SDK device IDs are hexadecimal; anything outside letters and digits is refused.
fn is_device_id(id: &str) -> bool {
    (1..=128).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn clean_probe(probe: Probe) -> Probe {
    let version = |value: Option<String>| value.map(|v| clip(&v, 64)).filter(|v| !v.is_empty());
    Probe {
        user_agent: clip(&probe.user_agent, 512),
        platform_version: version(probe.platform_version),
        webview_version: version(probe.webview_version),
        ..probe
    }
}

/// Only the origin of a blocked URL (no path or query), or a CSP keyword such as `inline`.
fn csp_origin(blocked: &str) -> String {
    match url::Url::parse(blocked) {
        Ok(url) if url.has_host() => url.origin().ascii_serialization(),
        Ok(url) => format!("{}:", url.scheme()),
        Err(_) => clip(blocked, 20)
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ora_types::{Disallows, RepeatMode, SdkTrack};

    fn id() -> String {
        "AbCd0123456789".chars().chain("xyzXYZ01".chars()).collect()
    }

    fn ready(playback: &Playback) {
        playback.apply(SdkEvent::Probe {
            probe: Probe {
                secure: true,
                widevine: true,
                ..Probe::default()
            },
        });
        playback.apply(SdkEvent::Ready {
            device_id: "device1".into(),
        });
    }

    fn state(uri: String, name: &str) -> SdkEvent {
        SdkEvent::State {
            state: Some(SdkState {
                paused: false,
                position_ms: 1_000,
                duration_ms: 180_000,
                track: Some(SdkTrack {
                    uri,
                    name: name.into(),
                    artists: vec!["Artist".into(), "  ".into()],
                    album: "Album".into(),
                    image_url: Some("https://i.scdn.co/image/album".into()),
                }),
                shuffle: true,
                repeat: RepeatMode::Context,
                disallows: Disallows {
                    seek: true,
                    ..Disallows::default()
                },
            }),
        }
    }

    #[test]
    fn the_probe_decides_whether_the_sdk_loads() {
        let playback = Playback::default();
        playback.apply(SdkEvent::Probe {
            probe: Probe {
                secure: true,
                user_agent: "x".repeat(2_000),
                webview_version: Some(" ".into()),
                ..Probe::default()
            },
        });
        let status = playback.status();
        assert_eq!(status.tier, Tier::Unsupported);
        assert!(!playback.drm_available());
        let probe = status.probe.unwrap();
        assert_eq!(probe.user_agent.len(), 512);
        assert_eq!(probe.webview_version, None);

        ready(&playback);
        assert!(playback.drm_available());
        assert_eq!(playback.status().tier, Tier::Ready);
        assert_eq!(playback.device_id().as_deref(), Some("device1"));
    }

    #[test]
    fn states_become_now_playing_and_another_device_ends_it() {
        let playback = Playback::default();
        // Before the device is ready, state reports are ignored.
        playback.apply(state(format!("spotify:track:{}", id()), "Song"));
        assert_eq!(playback.status().tier, Tier::Off);

        ready(&playback);
        playback.apply(state(format!("spotify:track:{}", id()), "Song\u{7}"));
        let status = playback.status();
        assert_eq!(status.tier, Tier::Active);
        assert!(!status.paused);
        assert_eq!(status.position_ms, 1_000);
        assert!(status.updated_at_ms > 1_700_000_000_000);
        let now = status.now_playing.unwrap();
        assert_eq!(now.name, "Song");
        assert_eq!(now.artists, ["Artist"]);
        assert_eq!(
            now.spotify_url,
            Some(format!("https://open.spotify.com/track/{}", id()))
        );
        assert_eq!(
            now.image_url.as_deref(),
            Some("https://i.scdn.co/image/album")
        );
        assert!(status.shuffle);
        assert_eq!(status.repeat, RepeatMode::Context);
        assert!(status.disallows.seek && !status.disallows.next);

        playback.apply(SdkEvent::State { state: None });
        let status = playback.status();
        assert_eq!(status.tier, Tier::Ready);
        assert_eq!(status.now_playing, None);
    }

    #[test]
    fn malformed_reports_are_dropped() {
        let playback = Playback::default();
        ready(&playback);
        let before = playback.status();
        playback.apply(SdkEvent::Ready {
            device_id: "bad id/..".into(),
        });
        playback.apply(SdkEvent::NotReady {
            device_id: "someone-else".into(),
        });
        assert_eq!(playback.status(), before);
        assert_eq!(playback.device_id().as_deref(), Some("device1"));

        playback.apply(state("https://evil.example/track".into(), "Song"));
        assert_eq!(playback.status().now_playing, None);
        playback.apply(state(format!("spotify:album:{}", id()), "Song"));
        assert_eq!(playback.status().now_playing, None);
    }

    #[test]
    fn errors_and_csp_blocks_are_kept_short() {
        let playback = Playback::default();
        ready(&playback);
        playback.apply(SdkEvent::Error {
            error: SdkErrorKind::Initialization,
            message: "Failed to initialize player\n".repeat(50),
        });
        let status = playback.status();
        assert_eq!(status.tier, Tier::Unsupported);
        assert_eq!(playback.device_id(), None);
        assert!(status.issue.unwrap().message.chars().count() <= MAX_TEXT);

        for _ in 0..3 {
            playback.apply(SdkEvent::CspViolation {
                directive: "script-src-elem".into(),
                origin: "https://cdn.example/path/x.js?token=1".into(),
            });
        }
        playback.apply(SdkEvent::CspViolation {
            directive: "script-src".into(),
            origin: "inline".into(),
        });
        assert_eq!(
            playback.status().csp_blocks,
            ["script-src-elem https://cdn.example", "script-src inline"]
        );
    }

    #[test]
    fn artwork_is_kept_only_from_the_allowed_image_host() {
        let playback = Playback::default();
        ready(&playback);
        for url in [
            "https://mosaic.scdn.co/640/x",
            "http://i.scdn.co/image/x",
            "https://i.scdn.co.example/image/x",
        ] {
            let mut event = state(format!("spotify:track:{}", id()), "Song");
            if let SdkEvent::State {
                state: Some(state), ..
            } = &mut event
            {
                if let Some(track) = &mut state.track {
                    track.image_url = Some(url.into());
                }
            }
            playback.apply(event);
            let now = playback.status().now_playing.unwrap();
            assert_eq!(now.image_url, None, "{url}");
        }
    }

    #[test]
    fn commands_need_a_device_and_are_bounded() {
        let playback = Playback::default();
        assert_eq!(
            playback.command(PlayerCommand::TogglePlay).err(),
            Some(OraError::NoActiveDevice)
        );
        ready(&playback);
        playback.apply(state(format!("spotify:track:{}", id()), "Song"));
        assert_eq!(
            playback.command(PlayerCommand::Seek {
                position_ms: u64::MAX
            }),
            Ok(PlayerCommand::Seek {
                position_ms: 180_000
            })
        );
        assert_eq!(
            playback.command(PlayerCommand::SetVolume { volume: 7.0 }),
            Ok(PlayerCommand::SetVolume { volume: 1.0 })
        );
        assert_eq!(
            playback
                .command(PlayerCommand::SetVolume { volume: f64::NAN })
                .err(),
            Some(OraError::Unsupported)
        );
        assert_eq!(
            playback.command(PlayerCommand::Disconnect),
            Ok(PlayerCommand::Disconnect)
        );
        let status = playback.status();
        assert_eq!(status.tier, Tier::Off);
        assert!(status.probe.is_some());
        assert_eq!(playback.device_id(), None);
    }
}
