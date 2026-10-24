//! MPRIS (`org.mpris.MediaPlayer2`) on the session bus: the media keys and the desktop's media
//! controls (KDE Plasma, GNOME and others). The desktop fetches the artwork itself, from the
//! image host the app allows. No volume is offered: it belongs to the device that plays.

use super::{Controls, Press};
use ora_core::{
    media::{MediaInfo, MediaKey},
    playback::now_ms,
};
use std::{collections::HashMap, sync::Arc};
use zbus::{
    connection, fdo, interface,
    object_server::SignalEmitter,
    zvariant::{ObjectPath, Value},
    Connection,
};

const PATH: &str = "/org/mpris/MediaPlayer2";
const NAME: &str = "org.mpris.MediaPlayer2.melora";
/// MPRIS's track ID when nothing plays.
const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";

type Raise = Arc<dyn Fn() + Send + Sync>;

/// MPRIS counts in microseconds.
fn micros(ms: u64) -> i64 {
    i64::try_from(ms.saturating_mul(1000)).unwrap_or(i64::MAX)
}

/// A D-Bus object path for the item: its base62 ID is already a valid path element.
fn track_id(info: &MediaInfo) -> String {
    info.id.as_ref().map_or_else(
        || NO_TRACK.to_owned(),
        |id| format!("/app/melora/item/{id}"),
    )
}

struct Root {
    raise: Raise,
}

#[interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        (self.raise)();
    }

    /// Closing the window is how Melora quits.
    fn quit(&self) {}

    #[zbus(property)]
    fn can_quit(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn identity(&self) -> &str {
        "Melora"
    }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        Vec::new()
    }
}

struct Player {
    press: Press,
    info: Option<MediaInfo>,
}

impl Player {
    fn can(&self, allowed: impl Fn(&MediaInfo) -> bool) -> bool {
        self.info.as_ref().is_some_and(allowed)
    }
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn next(&self) {
        (self.press)(MediaKey::Next);
    }

    fn previous(&self) {
        (self.press)(MediaKey::Previous);
    }

    fn pause(&self) {
        (self.press)(MediaKey::Pause);
    }

    fn play_pause(&self) {
        (self.press)(MediaKey::PlayPause);
    }

    fn stop(&self) {
        (self.press)(MediaKey::Stop);
    }

    fn play(&self) {
        (self.press)(MediaKey::Play);
    }

    fn seek(&self, offset: i64) {
        (self.press)(MediaKey::SeekBy(offset / 1000));
    }

    /// Ignored unless it names the item playing now, as MPRIS asks.
    fn set_position(&self, track: ObjectPath<'_>, position: i64) {
        let current = self.info.as_ref().map(track_id);
        if let (Some(current), Ok(position)) = (current, u64::try_from(position)) {
            if track.as_str() == current {
                (self.press)(MediaKey::SetPosition(position / 1000));
            }
        }
    }

    fn open_uri(&self, _uri: &str) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported("Melora opens no URIs".into()))
    }

    #[zbus(signal)]
    async fn seeked(emitter: &SignalEmitter<'_>, position: i64) -> zbus::Result<()>;

    #[zbus(property)]
    fn playback_status(&self) -> &str {
        match &self.info {
            None => "Stopped",
            Some(info) if info.playing => "Playing",
            Some(_) => "Paused",
        }
    }

    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<&'static str, Value<'static>> {
        let mut metadata = HashMap::new();
        let Some(info) = &self.info else {
            if let Ok(path) = ObjectPath::try_from(NO_TRACK) {
                metadata.insert("mpris:trackid", Value::from(path));
            }
            return metadata;
        };
        if let Ok(path) = ObjectPath::try_from(track_id(info)) {
            metadata.insert("mpris:trackid", Value::from(path));
        }
        metadata.insert("mpris:length", Value::from(micros(info.duration_ms)));
        metadata.insert("xesam:title", Value::from(info.title.clone()));
        metadata.insert("xesam:artist", Value::from(info.artists.clone()));
        metadata.insert("xesam:album", Value::from(info.album.clone()));
        if let Some(url) = &info.image_url {
            metadata.insert("mpris:artUrl", Value::from(url.clone()));
        }
        metadata
    }

    /// Read when asked, never signalled (MPRIS): `Seeked` reports jumps.
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        self.info
            .as_ref()
            .map_or(0, |info| micros(info.position_at(now_ms())))
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        self.can(|info| info.can_next)
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.can(|info| info.can_previous)
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        self.can(|info| info.can_play)
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        self.can(|info| info.can_pause)
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        self.can(|info| info.can_seek)
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn can_control(&self) -> bool {
        true
    }
}

pub(super) struct Mpris {
    connection: Connection,
}

impl Mpris {
    pub(super) async fn open(press: Press, raise: Raise) -> zbus::Result<Self> {
        let connection = connection::Builder::session()?
            .serve_at(PATH, Root { raise })?
            .serve_at(PATH, Player { press, info: None })?
            .build()
            .await?;
        // A second Melora takes an instance name, as MPRIS allows.
        if connection.request_name(NAME).await.is_err() {
            connection
                .request_name(format!("{NAME}.instance{}", std::process::id()))
                .await?;
        }
        Ok(Self { connection })
    }

    async fn update(
        &self,
        info: Option<&MediaInfo>,
        before: Option<&MediaInfo>,
    ) -> zbus::Result<()> {
        let player = self
            .connection
            .object_server()
            .interface::<_, Player>(PATH)
            .await?;
        player.get_mut().await.info = info.cloned();
        let emitter = player.signal_emitter();
        let current = player.get().await;
        current.playback_status_changed(emitter).await?;
        current.metadata_changed(emitter).await?;
        current.can_go_next_changed(emitter).await?;
        current.can_go_previous_changed(emitter).await?;
        current.can_play_changed(emitter).await?;
        current.can_pause_changed(emitter).await?;
        current.can_seek_changed(emitter).await?;
        if let (Some(info), Some(before)) = (info, before) {
            if info.jumped_from(before) {
                Player::seeked(emitter, micros(info.position_at(now_ms()))).await?;
            }
        }
        Ok(())
    }
}

impl Controls for Mpris {
    async fn show(&mut self, info: Option<&MediaInfo>, before: Option<&MediaInfo>) {
        // A failed signal leaves the desktop a step behind; the next change catches it up.
        let _ = self.update(info, before).await;
    }
}
