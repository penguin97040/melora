//! Playback: embedded playback (Tier A, the state the SDK bridge reports) and control
//! of other Spotify Connect devices (Tier B, through the Web API).
//!
//! Rules:
//! - Everything from the bridge or the Web API is untrusted input: it is validated, trimmed and
//!   reduced to the fields `ora-types` defines, and anything malformed is dropped.
//! - The access token reaches the bridge only through `sdk_token`, for the SDK's
//!   `getOAuthToken`, and only once the sign-in covers `EmbeddedPlayback`.
//! - Device IDs stay here; the UI gets opaque keys.
//! - While this app's SDK device plays, its reports win over the Web API (DO_NEXT §4.5).
//! - Progress is never polled: the UI interpolates `positionMs` from `updatedAtMs`, and the
//!   Web API state is fetched only on events, on focus and at a low rate.

use crate::{
    api::{self, is_allowed_image, ApiDevice, PlayStart, PlayerState},
    browser::Browser,
    http::HttpClient,
    link::{self, Kind, SpotifyUri},
    secret::Secret,
    store::CredentialStore,
    Ora,
};
use ora_types::{
    Device, DeviceKind, Feature, ForbiddenHint, NowPlaying, OraError, PlayRequest, PlaybackIssue,
    PlaybackStatus, PlayerCommand, Probe, SdkErrorKind, SdkEvent, SdkState, Tier,
};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::watch;

pub(crate) const MAX_TEXT: usize = 300;
pub(crate) const MAX_ARTISTS: usize = 20;
const MAX_CSP_BLOCKS: usize = 10;
const MAX_DEVICE_NAME: usize = 100;
/// Spotify runs player commands asynchronously, so the follow-up read waits this long.
const RECONCILE_DELAY: std::time::Duration = std::time::Duration::from_millis(600);
/// Longer than any track or episode, so a larger position is nonsense.
const MAX_POSITION_MS: u64 = 24 * 60 * 60 * 1000;

/// Locks are always taken in the order `device`, `remote`, then the status, and `remote` is
/// never locked while the status is borrowed.
pub struct Playback {
    /// The SDK's device ID while Spotify knows this app as a device.
    device: Mutex<Option<String>>,
    remote: Mutex<Remote>,
    status: watch::Sender<PlaybackStatus>,
}

#[derive(Default)]
struct Remote {
    /// The active device's real ID while it isn't this app.
    active: Option<String>,
    /// Keys handed to the UI, to the real ID and whether Spotify restricts the device.
    known: HashMap<String, (String, bool)>,
}

/// The active remote device a command goes to.
struct RemoteTarget {
    id: String,
    restricted: bool,
    supports_volume: bool,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            device: Mutex::new(None),
            remote: Mutex::new(Remote::default()),
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

    fn remote(&self) -> MutexGuard<'_, Remote> {
        self.remote
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
        *self.remote() = Remote::default();
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
        let mut remote = self.remote();
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
                forget_this_app(status);
                true
            }
            // Playback moved to another device; a Web API refresh shows which.
            SdkEvent::State { state: None } if device.is_some() => {
                status.tier = Tier::Ready;
                status.now_playing = None;
                status.paused = true;
                forget_this_app(status);
                true
            }
            SdkEvent::State { state: Some(state) } => match device.as_deref() {
                Some(id) => {
                    remote.active = None;
                    status.tier = Tier::Active;
                    status.device = Some(this_app(id));
                    fill(status, state);
                    true
                }
                None => false,
            },
            SdkEvent::Error { error, message } => {
                if error == SdkErrorKind::Initialization {
                    *device = None;
                    status.tier = Tier::Unsupported;
                    forget_this_app(status);
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

    /// Mirrors the Web API's playback state. While this app plays, its SDK reports win.
    pub(crate) fn apply_remote(&self, reported: Option<PlayerState>) {
        let device = self.device();
        let mut remote = self.remote();
        self.status.send_if_modified(|status| {
            if status.tier == Tier::Active {
                remote.active = None;
                return false;
            }
            match reported {
                // Spotify says this app is active; the SDK's own report follows.
                Some(reported) if device.as_deref() == Some(reported.device.id.as_str()) => {
                    remote.active = None;
                    false
                }
                Some(PlayerState { device, state }) => {
                    let key = device_key(&device.id);
                    remote
                        .known
                        .insert(key.clone(), (device.id.clone(), device.restricted));
                    remote.active = Some(device.id.clone());
                    status.device = Some(public_device(&device, key, false));
                    fill(status, state);
                    true
                }
                None => {
                    remote.active = None;
                    let changed = status.device.is_some() || status.now_playing.is_some();
                    status.device = None;
                    status.now_playing = None;
                    status.paused = true;
                    status.position_ms = 0;
                    status.duration_ms = 0;
                    changed
                }
            }
        });
    }

    /// Keys the device list for the UI and remembers which real ID each key means.
    pub(crate) fn remember_devices(&self, devices: Vec<ApiDevice>) -> Vec<Device> {
        let sdk = self.device().clone();
        let mut remote = self.remote();
        remote.known.clear();
        devices
            .into_iter()
            .map(|device| {
                let key = device_key(&device.id);
                let this_app = sdk.as_deref() == Some(device.id.as_str());
                remote
                    .known
                    .insert(key.clone(), (device.id.clone(), device.restricted));
                public_device(&device, key, this_app)
            })
            .collect()
    }

    /// The real ID behind a key from the last device list (or this app's own device).
    fn device_for(&self, key: &str) -> Result<String, OraError> {
        let sdk = self.device().clone();
        if let Some(sdk) = sdk.filter(|id| device_key(id) == key) {
            return Ok(sdk);
        }
        match self.remote().known.get(key) {
            Some((_, true)) => Err(OraError::Forbidden {
                hint: ForbiddenHint::Restricted,
            }),
            Some((id, false)) => Ok(id.clone()),
            None => Err(OraError::DeviceUnavailable),
        }
    }

    /// The active remote device, unless this app is the one playing.
    fn remote_target(&self) -> Option<RemoteTarget> {
        let id = self.remote().active.clone()?;
        let status = self.status.borrow();
        if status.tier == Tier::Active {
            return None;
        }
        let device = status.device.as_ref()?;
        Some(RemoteTarget {
            id,
            restricted: device.restricted,
            supports_volume: device.supports_volume,
        })
    }

    /// Shows a remote command's expected result until the next refresh confirms it.
    fn assume(&self, command: &PlayerCommand) {
        self.status.send_modify(|status| {
            let now = now_ms();
            match *command {
                PlayerCommand::TogglePlay => {
                    if !status.paused {
                        let played = now.saturating_sub(status.updated_at_ms);
                        status.position_ms = status
                            .position_ms
                            .saturating_add(played)
                            .min(status.duration_ms);
                    }
                    status.paused = !status.paused;
                    status.updated_at_ms = now;
                }
                PlayerCommand::Seek { position_ms } => {
                    status.position_ms = position_ms;
                    status.updated_at_ms = now;
                }
                PlayerCommand::SetShuffle { state } => status.shuffle = state,
                PlayerCommand::SetRepeat { mode } => status.repeat = mode,
                PlayerCommand::SetVolume { volume } => {
                    if let Some(device) = &mut status.device {
                        device.volume_percent = Some(percent(volume));
                    }
                }
                _ => {}
            }
        });
    }

    /// The item playing now and whether it's liked, if known.
    fn current_item(&self) -> Option<(SpotifyUri, Option<bool>)> {
        let status = self.status.borrow();
        let now = status.now_playing.as_ref()?;
        Some((link::parse(now.spotify_url.as_deref()?)?, now.liked))
    }

    /// Records Spotify's answer for `item`, unless another item is playing by now.
    fn set_liked(&self, item: &SpotifyUri, liked: bool) {
        let page = item.page();
        self.status
            .send_if_modified(|status| match &mut status.now_playing {
                Some(now) if now.spotify_url.as_deref() == Some(page.as_str()) => {
                    let changed = now.liked != Some(liked);
                    now.liked = Some(liked);
                    changed
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
        self.bound(command)
    }

    /// Keeps a seek within the item and the volume within 0 to 1.
    fn bound(&self, command: PlayerCommand) -> Result<PlayerCommand, OraError> {
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

    /// A player command from the UI. While another device is active, it runs here through the
    /// Web API. While this app plays, shuffle and repeat run here (the SDK has no methods for
    /// them) and everything else is returned for the SDK to run. `None` means it ran here.
    pub async fn player_command(
        &self,
        command: PlayerCommand,
    ) -> Result<Option<PlayerCommand>, OraError> {
        if command != PlayerCommand::Disconnect {
            if let Some(target) = self.playback().remote_target() {
                self.remote_command(target, command).await?;
                return Ok(None);
            }
        }
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

    async fn remote_command(
        &self,
        target: RemoteTarget,
        command: PlayerCommand,
    ) -> Result<(), OraError> {
        self.require(Feature::ConnectControl)?;
        if target.restricted {
            return Err(OraError::Forbidden {
                hint: ForbiddenHint::Restricted,
            });
        }
        if matches!(command, PlayerCommand::SetVolume { .. }) && !target.supports_volume {
            return Err(OraError::Unsupported);
        }
        let command = self.playback().bound(command)?;
        let paused = self.playback().status().paused;
        let (http, id, command_ref) = (self.http(), target.id.as_str(), &command);
        self.authorised(|access| async move {
            match *command_ref {
                PlayerCommand::TogglePlay if paused => api::resume(http, &access, id).await,
                PlayerCommand::TogglePlay => api::pause(http, &access, id).await,
                PlayerCommand::Seek { position_ms } => {
                    api::seek(http, &access, id, position_ms).await
                }
                PlayerCommand::Next => api::next(http, &access, id).await,
                PlayerCommand::Previous => api::previous(http, &access, id).await,
                PlayerCommand::SetVolume { volume } => {
                    api::volume(http, &access, id, percent(volume)).await
                }
                PlayerCommand::SetShuffle { state } => api::shuffle(http, &access, id, state).await,
                PlayerCommand::SetRepeat { mode } => api::repeat(http, &access, id, mode).await,
                PlayerCommand::Disconnect => Ok(()),
            }
        })
        .await?;
        self.playback().assume(&command);
        Ok(())
    }

    /// Fetches the Web API's playback state and mirrors it, unless this app is the one playing.
    pub async fn refresh_playback(&self) -> Result<(), OraError> {
        self.require(Feature::ConnectControl)?;
        let http = self.http();
        let state = self
            .authorised(|access| async move { api::player_state(http, &access).await })
            .await?;
        self.playback().apply_remote(state);
        Ok(())
    }

    /// A refresh shortly after a command or a device change, to confirm what Spotify did.
    /// Failures are left for the next refresh: this one is only a follow-up.
    pub async fn reconcile_playback(&self) {
        tokio::time::sleep(RECONCILE_DELAY).await;
        let _ = self.refresh_playback().await;
    }

    /// The account's Spotify Connect devices, with opaque keys for `transfer_playback`.
    pub async fn list_devices(&self) -> Result<Vec<Device>, OraError> {
        self.require(Feature::DevicePicker)?;
        let http = self.http();
        let devices = self
            .authorised(|access| async move { api::devices(http, &access).await })
            .await?;
        Ok(self.playback().remember_devices(devices))
    }

    /// Moves playback to the device behind `key` and keeps it playing.
    pub async fn transfer_playback(&self, key: &str) -> Result<(), OraError> {
        self.require(Feature::TransferPlayback)?;
        let id = self.playback().device_for(key)?;
        let (http, id) = (self.http(), id.as_str());
        self.authorised(|access| async move { api::transfer(http, &access, id, true).await })
            .await
    }

    /// Asks Spotify whether the track playing now is in Liked Songs. Episodes aren't checked.
    pub async fn refresh_liked(&self) -> Result<(), OraError> {
        self.require(Feature::LibrarySaveRemove)?;
        let Some((item, _)) = self.playback().current_item() else {
            return Ok(());
        };
        if item.kind != Kind::Track {
            return Ok(());
        }
        let liked = self.library_contains(&item).await?;
        self.playback().set_liked(&item, liked);
        Ok(())
    }

    /// The `+` control: adds the track playing now to Liked Songs, or removes it from there.
    /// Returns whether it's liked afterwards, for the UI's "Added to Liked Songs" message.
    pub async fn toggle_liked(&self) -> Result<bool, OraError> {
        self.require(Feature::LibrarySaveRemove)?;
        let (item, liked) = self
            .playback()
            .current_item()
            .ok_or(OraError::NoActiveDevice)?;
        if item.kind != Kind::Track {
            return Err(OraError::Unsupported);
        }
        let liked = match liked {
            Some(liked) => liked,
            None => self.library_contains(&item).await?,
        };
        let (http, item_ref) = (self.http(), &item);
        self.authorised(
            |access| async move { api::library_set(http, &access, item_ref, !liked).await },
        )
        .await?;
        self.playback().set_liked(&item, !liked);
        self.library().forget_liked();
        Ok(!liked)
    }

    async fn library_contains(&self, item: &SpotifyUri) -> Result<bool, OraError> {
        let http = self.http();
        self.authorised(|access| async move { api::library_contains(http, &access, item).await })
            .await
    }

    /// Plays from a list: on the active device, or on this app's own device when nothing else
    /// is playing. With neither, the UI asks for a device to be chosen.
    pub async fn play(&self, request: PlayRequest) -> Result<(), OraError> {
        let start = play_start(request)?;
        let device = match self.playback().remote_target() {
            Some(target) => {
                self.require(Feature::ConnectControl)?;
                if target.restricted {
                    return Err(OraError::Forbidden {
                        hint: ForbiddenHint::Restricted,
                    });
                }
                target.id
            }
            None => {
                self.require(Feature::EmbeddedPlayback)?;
                self.playback()
                    .device_id()
                    .ok_or(OraError::NoActiveDevice)?
            }
        };
        let (http, device, start) = (self.http(), device.as_str(), &start);
        self.authorised(|access| async move { api::play_from(http, &access, device, start).await })
            .await
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

/// Spotify accepts long `uris` lists, but a click never needs more than a page or two.
const MAX_PLAY_ITEMS: usize = 100;

/// Checks a `PlayRequest` from the UI: a playlist, album or artist, or tracks and episodes.
/// Spotify starts only playlists and albums at a position.
fn play_start(request: PlayRequest) -> Result<PlayStart, OraError> {
    match request {
        PlayRequest::Context { uri, position } => {
            let context = link::parse(&uri)
                .filter(|uri| matches!(uri.kind, Kind::Playlist | Kind::Album | Kind::Artist))
                .ok_or(OraError::InvalidSpotifyLink)?;
            if context.kind == Kind::Artist && position.is_some() {
                return Err(OraError::Unsupported);
            }
            Ok(PlayStart::Context { context, position })
        }
        PlayRequest::Items { uris } => {
            if uris.is_empty() || uris.len() > MAX_PLAY_ITEMS {
                return Err(OraError::Unsupported);
            }
            uris.iter()
                .map(|uri| link::parse(uri).filter(|uri| !uri.is_context()))
                .collect::<Option<Vec<_>>>()
                .map(PlayStart::Items)
                .ok_or(OraError::InvalidSpotifyLink)
        }
    }
}

/// Copies a validated state report (from the SDK or the Web API) into the status.
fn fill(status: &mut PlaybackStatus, state: SdkState) {
    status.paused = state.paused;
    status.duration_ms = state.duration_ms.min(MAX_POSITION_MS);
    status.position_ms = state.position_ms.min(status.duration_ms);
    status.updated_at_ms = now_ms();
    status.shuffle = state.shuffle;
    status.repeat = state.repeat;
    status.disallows = state.disallows;
    let previous = status.now_playing.take();
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
            liked: None,
        })
    });
    // Reports repeat while an item plays; Spotify's liked answer holds until the item changes.
    if let (Some(now), Some(previous)) = (&mut status.now_playing, previous) {
        if now.spotify_url == previous.spotify_url {
            now.liked = previous.liked;
        }
    }
}

/// A stable, opaque key for a device ID: it can't be turned back into the ID.
fn device_key(id: &str) -> String {
    Sha256::digest(id.as_bytes())[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn public_device(device: &ApiDevice, key: String, this_app: bool) -> Device {
    Device {
        key,
        name: clip(&device.name, MAX_DEVICE_NAME),
        kind: device.kind,
        active: device.active,
        restricted: device.restricted,
        this_app,
        supports_volume: device.supports_volume,
        volume_percent: device.volume_percent,
    }
}

/// This app's SDK device while it plays. The UI shows its own name for it.
fn this_app(id: &str) -> Device {
    Device {
        key: device_key(id),
        name: String::new(),
        kind: DeviceKind::Computer,
        active: true,
        restricted: false,
        this_app: true,
        supports_volume: true,
        volume_percent: None,
    }
}

fn forget_this_app(status: &mut PlaybackStatus) {
    if status.device.as_ref().is_some_and(|device| device.this_app) {
        status.device = None;
    }
}

/// 0.0–1.0 to Spotify's whole percent.
fn percent(volume: f64) -> u8 {
    (volume.clamp(0.0, 1.0) * 100.0).round() as u8
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// At most `max` characters, without control characters.
pub(crate) fn clip(text: &str, max: usize) -> String {
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
    fn the_liked_answer_lasts_while_the_item_plays() {
        let playback = Playback::default();
        ready(&playback);
        playback.apply(state(format!("spotify:track:{}", id()), "Song"));
        let (item, liked) = playback.current_item().unwrap();
        assert_eq!(liked, None);
        playback.set_liked(&item, true);
        playback.apply(state(format!("spotify:track:{}", id()), "Song"));
        assert_eq!(playback.status().now_playing.unwrap().liked, Some(true));

        // Another item is unknown again, and a late answer about the old one is ignored.
        let other: String = id().chars().rev().collect();
        playback.apply(state(format!("spotify:track:{other}"), "Next"));
        assert_eq!(playback.status().now_playing.unwrap().liked, None);
        playback.set_liked(&item, false);
        assert_eq!(playback.status().now_playing.unwrap().liked, None);
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

    fn phone(id: &str, restricted: bool) -> ApiDevice {
        ApiDevice {
            id: id.into(),
            name: "Phone\u{7}".into(),
            kind: DeviceKind::Smartphone,
            active: true,
            restricted,
            supports_volume: false,
            volume_percent: None,
        }
    }

    fn remote_state(device: ApiDevice) -> PlayerState {
        let SdkEvent::State { state: Some(state) } =
            state(format!("spotify:track:{}", id()), "Remote song")
        else {
            unreachable!()
        };
        PlayerState { device, state }
    }

    #[test]
    fn another_device_is_mirrored_until_this_app_plays() {
        let playback = Playback::default();
        ready(&playback);
        playback.apply_remote(Some(remote_state(phone("phone1", false))));
        let status = playback.status();
        assert_eq!(status.tier, Tier::Ready);
        let device = status.device.unwrap();
        assert_eq!(device.name, "Phone");
        assert!(!device.this_app && device.active);
        assert_eq!(device.key.len(), 16);
        assert!(!device.key.contains("phone1"));
        assert_eq!(status.now_playing.unwrap().name, "Remote song");
        assert!(playback.remote_target().is_some());

        // This app starts playing: the SDK's reports win from now on.
        playback.apply(state(format!("spotify:track:{}", id()), "Here"));
        playback.apply_remote(Some(remote_state(phone("phone1", false))));
        let status = playback.status();
        assert_eq!(status.tier, Tier::Active);
        assert!(status.device.unwrap().this_app);
        assert_eq!(status.now_playing.unwrap().name, "Here");
        assert!(playback.remote_target().is_none());

        // Playback moves away, and Spotify reports nothing active.
        playback.apply(SdkEvent::State { state: None });
        assert_eq!(playback.status().device, None);
        playback.apply_remote(Some(remote_state(phone("phone1", false))));
        assert!(playback.status().device.is_some());
        playback.apply_remote(None);
        let status = playback.status();
        assert_eq!((status.device, status.now_playing), (None, None));
        assert!(playback.remote_target().is_none());
    }

    #[test]
    fn a_web_api_report_of_this_app_waits_for_the_sdk() {
        let playback = Playback::default();
        ready(&playback);
        let before = playback.status();
        playback.apply_remote(Some(remote_state(phone("device1", false))));
        assert_eq!(playback.status(), before);
        assert!(playback.remote_target().is_none());
    }

    #[test]
    fn device_keys_are_opaque_and_checked() {
        let playback = Playback::default();
        ready(&playback);
        let mut speaker = phone("speaker1", true);
        speaker.active = false;
        let listed = playback.remember_devices(vec![
            phone("device1", false),
            phone("phone1", false),
            speaker,
        ]);
        assert!(listed[0].this_app && !listed[1].this_app);
        assert_ne!(listed[0].key, listed[1].key);
        assert_eq!(playback.device_for(&listed[1].key).unwrap(), "phone1");
        assert_eq!(playback.device_for(&listed[0].key).unwrap(), "device1");
        assert_eq!(
            playback.device_for(&listed[2].key).err(),
            Some(OraError::Forbidden {
                hint: ForbiddenHint::Restricted
            })
        );
        assert_eq!(
            playback.device_for("0123456789abcdef").err(),
            Some(OraError::DeviceUnavailable)
        );
        // A fresh list forgets devices that have gone.
        playback.remember_devices(vec![phone("phone2", false)]);
        assert_eq!(
            playback.device_for(&listed[1].key).err(),
            Some(OraError::DeviceUnavailable)
        );
    }

    #[test]
    fn remote_commands_show_their_result_until_confirmed() {
        let playback = Playback::default();
        playback.apply_remote(Some(remote_state(phone("phone1", false))));
        playback.assume(&PlayerCommand::TogglePlay);
        assert!(playback.status().paused);
        playback.assume(&PlayerCommand::Seek { position_ms: 9_000 });
        assert_eq!(playback.status().position_ms, 9_000);
        playback.assume(&PlayerCommand::SetShuffle { state: false });
        playback.assume(&PlayerCommand::SetRepeat {
            mode: RepeatMode::Track,
        });
        playback.assume(&PlayerCommand::SetVolume { volume: 0.456 });
        let status = playback.status();
        assert!(!status.shuffle);
        assert_eq!(status.repeat, RepeatMode::Track);
        assert_eq!(status.device.unwrap().volume_percent, Some(46));
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
