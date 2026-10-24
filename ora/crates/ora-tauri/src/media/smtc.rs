//! Windows' media controls (`SystemMediaTransportControls`) for the main window: the media
//! keys and the media overlay. The app switches off WebView2's own entry
//! (`HardwareMediaKeyHandling`), so Melora appears once, under its own name, whichever
//! device plays. Windows fetches the artwork itself, from the image host the app allows.

use super::{Controls, Press};
use ora_core::{
    media::{MediaInfo, MediaKey},
    playback::now_ms,
};
use std::time::Duration;
use windows::{
    core::{factory, Ref, Result, HSTRING},
    Foundation::{TimeSpan, TypedEventHandler, Uri},
    Media::{
        MediaPlaybackStatus, MediaPlaybackType, SystemMediaTransportControls,
        SystemMediaTransportControlsButton, SystemMediaTransportControlsButtonPressedEventArgs,
        SystemMediaTransportControlsTimelineProperties,
    },
    Storage::Streams::RandomAccessStreamReference,
    Win32::{Foundation::HWND, System::WinRT::ISystemMediaTransportControlsInterop},
};

pub(super) struct Windows {
    controls: SystemMediaTransportControls,
    /// The artwork shown, so Windows fetches it again only when it changes.
    image: Option<String>,
}

impl Windows {
    pub(super) fn open(window: HWND, press: Press) -> Result<Self> {
        let interop =
            factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()?;
        let controls: SystemMediaTransportControls = unsafe { interop.GetForWindow(window) }?;
        controls.SetIsEnabled(false)?;
        controls.ButtonPressed(&TypedEventHandler::new(
            move |_, args: Ref<SystemMediaTransportControlsButtonPressedEventArgs>| {
                // The play/pause key arrives as Play or Pause, from the status shown.
                let key = match args.ok()?.Button()? {
                    SystemMediaTransportControlsButton::Play => MediaKey::Play,
                    SystemMediaTransportControlsButton::Pause => MediaKey::Pause,
                    SystemMediaTransportControlsButton::Stop => MediaKey::Stop,
                    SystemMediaTransportControlsButton::Next => MediaKey::Next,
                    SystemMediaTransportControlsButton::Previous => MediaKey::Previous,
                    _ => return Ok(()),
                };
                press(key);
                Ok(())
            },
        ))?;
        Ok(Self {
            controls,
            image: None,
        })
    }

    fn update(&mut self, info: Option<&MediaInfo>) -> Result<()> {
        let controls = &self.controls;
        let display = controls.DisplayUpdater()?;
        let Some(info) = info else {
            self.image = None;
            display.ClearAll()?;
            display.Update()?;
            controls.SetPlaybackStatus(MediaPlaybackStatus::Closed)?;
            return controls.SetIsEnabled(false);
        };
        controls.SetIsEnabled(true)?;
        controls.SetIsPlayEnabled(info.can_play)?;
        controls.SetIsPauseEnabled(info.can_pause)?;
        controls.SetIsStopEnabled(info.can_pause)?;
        controls.SetIsNextEnabled(info.can_next)?;
        controls.SetIsPreviousEnabled(info.can_previous)?;
        controls.SetPlaybackStatus(if info.playing {
            MediaPlaybackStatus::Playing
        } else {
            MediaPlaybackStatus::Paused
        })?;
        display.SetType(MediaPlaybackType::Music)?;
        let music = display.MusicProperties()?;
        music.SetTitle(&HSTRING::from(&info.title))?;
        music.SetArtist(&HSTRING::from(info.artists.join(", ")))?;
        music.SetAlbumTitle(&HSTRING::from(&info.album))?;
        if self.image != info.image_url {
            match &info.image_url {
                Some(url) => display.SetThumbnail(&RandomAccessStreamReference::CreateFromUri(
                    &Uri::CreateUri(&HSTRING::from(url))?,
                )?)?,
                None => display.SetThumbnail(None::<&RandomAccessStreamReference>)?,
            }
            self.image = info.image_url.clone();
        }
        display.Update()?;
        let at = |ms: u64| TimeSpan::from(Duration::from_millis(ms));
        let timeline = SystemMediaTransportControlsTimelineProperties::new()?;
        timeline.SetStartTime(at(0))?;
        timeline.SetMinSeekTime(at(0))?;
        timeline.SetEndTime(at(info.duration_ms))?;
        timeline.SetMaxSeekTime(at(info.duration_ms))?;
        timeline.SetPosition(at(info.position_at(now_ms())))?;
        controls.UpdateTimelineProperties(&timeline)
    }
}

impl Controls for Windows {
    async fn show(&mut self, info: Option<&MediaInfo>, _before: Option<&MediaInfo>) {
        // A refused update leaves the last one showing; the next change tries again.
        let _ = self.update(info);
    }
}
