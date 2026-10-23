//! Playback: what the shared Web Playback SDK bridge reports, the commands it runs, the
//! Spotify devices a client can control, and the status every client shows. No tokens or
//! device IDs appear here.

use serde::{Deserialize, Serialize};

/// What the webview supports, checked before the SDK is loaded. Diagnostics only: no personal data.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct Probe {
    pub secure: bool,
    pub widevine: bool,
    pub playready: bool,
    pub fairplay: bool,
    pub user_agent: String,
    /// The OS version the webview reports (`userAgentData` platformVersion), when available.
    pub platform_version: Option<String>,
    /// The webview engine version (for example WebView2's Edge version), when available.
    pub webview_version: Option<String>,
}

impl Probe {
    /// Some DRM system is available, so the SDK is worth loading.
    pub fn any_drm(&self) -> bool {
        self.secure && (self.widevine || self.playready || self.fairplay)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum SdkErrorKind {
    /// `initialization_error`: usually no EME/DRM support.
    Initialization,
    /// `authentication_error`: the token was refused.
    Authentication,
    /// `account_error`: the account isn't Premium.
    Account,
    /// `playback_error`: a track couldn't be played.
    Playback,
    /// `autoplay_failed`: the webview blocked audio until a click.
    Autoplay,
}

/// The current item, reduced from the SDK's `player_state_changed` by the bridge.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct SdkTrack {
    pub uri: String,
    pub name: String,
    pub artists: Vec<String>,
    pub album: String,
    /// The album image the bridge picked (closest to 300 px wide), if any.
    #[serde(default)]
    pub image_url: Option<String>,
}

/// The SDK's `repeat_mode`: 0, 1 and 2.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum RepeatMode {
    #[default]
    Off,
    Context,
    Track,
}

/// The SDK's `disallows`: `true` means Spotify doesn't permit that control right now (for
/// example while an advert plays), so clients disable it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct Disallows {
    pub pause: bool,
    pub resume: bool,
    pub next: bool,
    pub previous: bool,
    pub seek: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct SdkState {
    pub paused: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub track: Option<SdkTrack>,
    #[serde(default)]
    pub shuffle: bool,
    #[serde(default)]
    pub repeat: RepeatMode,
    #[serde(default)]
    pub disallows: Disallows,
}

/// Bridge → Rust (`plugin:ora|sdk_event`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum SdkEvent {
    Probe {
        probe: Probe,
    },
    /// The SDK is connected and Spotify knows this device.
    Ready {
        #[serde(rename = "deviceId")]
        device_id: String,
    },
    NotReady {
        #[serde(rename = "deviceId")]
        device_id: String,
    },
    /// `None` when playback moved to another device.
    State {
        state: Option<SdkState>,
    },
    Error {
        error: SdkErrorKind,
        /// The SDK's own message (client-side text, never a server body or token).
        message: String,
    },
    /// The page's CSP blocked something: only the directive and the blocked origin.
    CspViolation {
        directive: String,
        origin: String,
    },
}

/// UI → Rust (`plugin:ora|playback_command`) and Rust → bridge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum PlayerCommand {
    TogglePlay,
    Seek {
        #[serde(rename = "positionMs")]
        position_ms: u64,
    },
    Next,
    Previous,
    /// 0.0 to 1.0.
    SetVolume {
        volume: f64,
    },
    /// Run through the Web API: the SDK has no shuffle or repeat methods.
    SetShuffle {
        state: bool,
    },
    SetRepeat {
        mode: RepeatMode,
    },
    Disconnect,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum Tier {
    /// Embedded playback hasn't been enabled in this run.
    #[default]
    Off,
    /// The webview has no usable DRM, so the SDK isn't loaded (control another device instead).
    Unsupported,
    /// Loading the SDK or waiting for Spotify to accept the device.
    Connecting,
    /// This app is a Spotify device, but something else is playing (or nothing is).
    Ready,
    /// This app is the active device.
    Active,
}

/// Spotify's device `type`, reduced to what a client shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum DeviceKind {
    Computer,
    Smartphone,
    Tablet,
    Speaker,
    Tv,
    GameConsole,
    Automobile,
    #[default]
    Other,
}

/// A Spotify Connect device. `key` is an opaque handle for choosing it; the real device ID
/// stays in Rust.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct Device {
    pub key: String,
    pub name: String,
    pub kind: DeviceKind,
    pub active: bool,
    /// Spotify accepts no commands for this device from other apps.
    pub restricted: bool,
    /// This app's own Web Playback SDK device.
    pub this_app: bool,
    pub supports_volume: bool,
    pub volume_percent: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct NowPlaying {
    pub name: String,
    pub artists: Vec<String>,
    pub album: String,
    /// `https://open.spotify.com/…` for "Open in Spotify".
    pub spotify_url: Option<String>,
    /// Album artwork, only from the image host clients allow in their CSP.
    pub image_url: Option<String>,
    /// Whether the track is in the account's Liked Songs: `None` until checked, and always for
    /// episodes. Spotify's answer is only held for the current item, never stored.
    pub liked: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct PlaybackIssue {
    pub kind: SdkErrorKind,
    pub message: String,
}

/// Everything the UI needs about playback, pushed on every change. `tier` is this app's own
/// SDK device; `device` is whichever device is active, and the now-playing fields describe it.
/// Progress is interpolated by the UI from `positionMs` at `updatedAtMs` while not paused.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct PlaybackStatus {
    pub tier: Tier,
    /// The active device: this app while the SDK plays, otherwise from the Web API.
    pub device: Option<Device>,
    pub now_playing: Option<NowPlaying>,
    pub paused: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    /// Milliseconds since the Unix epoch when `positionMs` was reported.
    pub updated_at_ms: u64,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub disallows: Disallows,
    pub issue: Option<PlaybackIssue>,
    pub probe: Option<Probe>,
    /// `directive origin` pairs the CSP blocked, at most ten.
    pub csp_blocks: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_messages_use_the_documented_shapes() {
        let event: SdkEvent =
            serde_json::from_value(serde_json::json!({"type": "ready", "deviceId": "abc"}))
                .unwrap();
        assert_eq!(
            event,
            SdkEvent::Ready {
                device_id: "abc".into()
            }
        );
        let command = serde_json::to_value(PlayerCommand::Seek { position_ms: 5 }).unwrap();
        assert_eq!(command, serde_json::json!({"op": "seek", "positionMs": 5}));
        let command = serde_json::to_value(PlayerCommand::TogglePlay).unwrap();
        assert_eq!(command, serde_json::json!({"op": "togglePlay"}));
    }

    #[test]
    fn drm_needs_a_secure_context() {
        let mut probe = Probe {
            widevine: true,
            ..Probe::default()
        };
        assert!(!probe.any_drm());
        probe.secure = true;
        assert!(probe.any_drm());
    }
}
