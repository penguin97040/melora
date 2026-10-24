//! The system's media controls, kept in step with the playback status. `ora_core::media`
//! decides what they show and what each key does; a key runs exactly like the player's own
//! button, so it works for this app's SDK and for a remote device alike.

#[cfg(all(unix, not(target_os = "macos")))]
mod mpris;
#[cfg(windows)]
mod smtc;

use crate::{run_command, Bridge, Shared};
use ora_core::{
    media::{self, MediaInfo, MediaKey},
    playback::now_ms,
};
use std::{future::Future, sync::Arc};
use tauri::{AppHandle, Manager, Runtime, WebviewWindow};

/// Called for every press, on whichever thread the OS delivers it.
type Press = Arc<dyn Fn(MediaKey) + Send + Sync>;

/// One OS's media controls.
trait Controls: Send + 'static {
    /// Shows `info`, or nothing; `before` is what was shown until now.
    fn show(
        &mut self,
        info: Option<&MediaInfo>,
        before: Option<&MediaInfo>,
    ) -> impl Future<Output = ()> + Send;
}

pub(crate) fn attach<R: Runtime>(window: &WebviewWindow<R>) {
    let app = window.app_handle().clone();
    let press: Press = Arc::new({
        let app = app.clone();
        move |key| {
            let app = app.clone();
            tauri::async_runtime::spawn(async move { pressed(&app, key).await });
        }
    });
    #[cfg(windows)]
    {
        // Here, on the thread that owns the window.
        if let Some(controls) = window
            .hwnd()
            .ok()
            .and_then(|hwnd| smtc::Windows::open(hwnd, press).ok())
        {
            tauri::async_runtime::spawn(follow(app, controls));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let window = window.clone();
        let raise = Arc::new(move || {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        });
        tauri::async_runtime::spawn(async move {
            if let Ok(controls) = mpris::Mpris::open(press, raise).await {
                follow(app, controls).await;
            }
        });
    }
    #[cfg(target_os = "macos")]
    let _ = (app, press);
}

/// Shows every change of what plays, until the app quits.
#[cfg_attr(target_os = "macos", allow(dead_code))]
async fn follow<R: Runtime>(app: AppHandle<R>, mut controls: impl Controls) {
    let mut updates = app.state::<Shared>().0.playback().subscribe();
    let mut shown: Option<MediaInfo> = None;
    loop {
        let info = media::media_info(&updates.borrow_and_update());
        if info != shown {
            controls.show(info.as_ref(), shown.as_ref()).await;
            shown = info;
        }
        if updates.changed().await.is_err() {
            break;
        }
    }
}

/// A key does nothing when the command fails (no permission, a restricted device): there is
/// no window to explain it in, and the player shows the state either way.
async fn pressed<R: Runtime>(app: &AppHandle<R>, key: MediaKey) {
    let ora = app.state::<Shared>();
    let status = ora.0.playback().status();
    if let Some(command) = media::command(&status, key, now_ms()) {
        let _ = run_command(&ora.0, &app.state::<Bridge>(), command).await;
    }
}
