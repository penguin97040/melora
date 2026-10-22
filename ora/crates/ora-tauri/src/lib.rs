//! The `ora` Tauri plugin. It exposes the shared session in `ora-core` to the client's main
//! window as a few narrow commands and status streams. The only token that crosses into
//! JavaScript is the access token `access_token` hands to the Web Playback SDK's
//! `getOAuthToken`, through the shared sdk-bridge.

use ora_core::{
    browser::{self, Browser, SystemBrowser},
    http::ReqwestClient,
    store::KeyringStore,
    types::{Feature, OraError, PlaybackStatus, PlayerCommand, Profile, SdkEvent, Status},
    Options, Ora,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tauri::{
    ipc::Channel,
    plugin::{Builder, TauriPlugin},
    AppHandle, Manager, Runtime, State, Window,
};

pub use ora_core as core;

const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);

type Core = Ora<ReqwestClient, KeyringStore, SystemBrowser>;

struct Shared(Arc<Core>);

/// The bridge's command channel, once it has attached.
struct Bridge(Mutex<Option<Channel<PlayerCommand>>>);

impl Bridge {
    fn send(&self, command: PlayerCommand) -> Result<(), OraError> {
        let mut channel = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let sent = channel
            .as_ref()
            .is_some_and(|channel| channel.send(command).is_ok());
        if sent {
            Ok(())
        } else {
            // The page went away; the next `sdk_attach` replaces it.
            *channel = None;
            Err(OraError::NoActiveDevice)
        }
    }
}

/// The origin of Spotify's Web Playback SDK script and its playback frame. Clients allow it in
/// their CSP (`script-src`, `frame-src`) and nowhere else.
pub const SDK_ORIGIN: &str = "https://sdk.scdn.co";
/// The SDK's playback frame. Navigation handlers that also see sub-frames (macOS, Linux) may
/// allow it while `sdk_frame_allowed` is true.
pub const SDK_FRAME_URL: &str = "https://sdk.scdn.co/embedded/index.html";

static SDK_FRAME_ALLOWED: AtomicBool = AtomicBool::new(false);

/// Whether the capability probe found DRM, so the SDK's frame may load on this system.
pub fn sdk_frame_allowed() -> bool {
    SDK_FRAME_ALLOWED.load(Ordering::SeqCst)
}

/// What each client supplies when registering the plugin.
pub struct OraConfig<R: Runtime> {
    /// Credential store service name, for example `app.melora.desktop`.
    pub service: &'static str,
    /// Shown on the sign-in callback page.
    pub app_name: &'static str,
    /// Development override for the Client ID, for example `MELORA_SPOTIFY_CLIENT_ID`.
    pub client_id_variable: &'static str,
    /// Where `spotify.json` lives (beside the executable for Windows portable builds).
    pub settings_dir: fn(&AppHandle<R>) -> Option<PathBuf>,
    /// Fixed loopback port for the sign-in callback. The Spotify Dashboard refuses port-less
    /// `http://127.0.0.1/callback`, so each client registers `http://127.0.0.1:<port>/callback`.
    pub callback_port: u16,
}

/// The plugin, registered by each client with `.plugin(ora_tauri::init(config))`.
pub fn init<R: Runtime>(config: OraConfig<R>) -> TauriPlugin<R> {
    Builder::new("ora")
        .invoke_handler(tauri::generate_handler![
            status,
            sign_in,
            cancel_sign_in,
            sign_out,
            set_client_id,
            profile,
            subscribe,
            open_in_spotify,
            access_token,
            sdk_event,
            sdk_attach,
            subscribe_playback,
            play_here,
            playback_command
        ])
        .setup(move |app, _api| {
            let settings_dir =
                (config.settings_dir)(app).ok_or("Cannot locate the settings folder.")?;
            let http = ReqwestClient::new().map_err(|_| "The HTTPS client could not start.")?;
            let core = Arc::new(Ora::new(
                http,
                KeyringStore::new(config.service),
                SystemBrowser,
                Options {
                    app_name: config.app_name,
                    settings_dir,
                    client_id_variable: config.client_id_variable,
                    sign_in_timeout: SIGN_IN_TIMEOUT,
                    callback_port: config.callback_port,
                },
            ));
            app.manage(Shared(Arc::clone(&core)));
            app.manage(Bridge(Mutex::new(None)));
            tauri::async_runtime::spawn(async move {
                #[cfg(debug_assertions)]
                {
                    // Development only: which store was selected (never any values).
                    let backend = tauri::async_runtime::spawn_blocking(KeyringStore::backend).await;
                    eprintln!("ora credential store: {backend:?}");
                }
                core.restore().await;
            });
            Ok(())
        })
        .build()
}

fn main_only<R: Runtime>(window: &Window<R>) -> Result<(), OraError> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(OraError::Unsupported)
    }
}

#[tauri::command]
async fn status<R: Runtime>(window: Window<R>, ora: State<'_, Shared>) -> Result<Status, OraError> {
    main_only(&window)?;
    Ok(ora.0.status())
}

/// `feature` asks Spotify for that feature's extra permission as part of the sign-in.
#[tauri::command]
async fn sign_in<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    feature: Option<Feature>,
) -> Result<Status, OraError> {
    main_only(&window)?;
    ora.0.sign_in(feature).await
}

#[tauri::command]
async fn cancel_sign_in<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
) -> Result<Status, OraError> {
    main_only(&window)?;
    Ok(ora.0.cancel_sign_in())
}

/// Also stops embedded playback: the SDK must not keep playing for a signed-out account.
#[tauri::command]
async fn sign_out<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    bridge: State<'_, Bridge>,
) -> Result<Status, OraError> {
    main_only(&window)?;
    let status = ora.0.sign_out().await;
    if ora.0.playback().command(PlayerCommand::Disconnect).is_ok() {
        let _ = bridge.send(PlayerCommand::Disconnect);
    }
    status
}

#[tauri::command]
async fn set_client_id<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    client_id: String,
) -> Result<Status, OraError> {
    main_only(&window)?;
    ora.0.set_client_id(&client_id).await
}

#[tauri::command]
async fn profile<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
) -> Result<Option<Profile>, OraError> {
    main_only(&window)?;
    ora.0.profile().await
}

/// Sends the current status, then every change, until the page goes away.
#[tauri::command]
async fn subscribe<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    on_status: Channel<Status>,
) -> Result<(), OraError> {
    main_only(&window)?;
    let mut updates = ora.0.subscribe();
    let first = updates.borrow_and_update().clone();
    on_status.send(first).map_err(|_| OraError::Unsupported)?;
    tauri::async_runtime::spawn(async move {
        while updates.changed().await.is_ok() {
            let next = updates.borrow_and_update().clone();
            if on_status.send(next).is_err() {
                break;
            }
        }
    });
    Ok(())
}

/// For the sdk-bridge's `getOAuthToken` only: a fresh access token, once the sign-in covers
/// embedded playback. Never logged, stored or returned anywhere else.
#[tauri::command]
async fn access_token<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
) -> Result<String, OraError> {
    main_only(&window)?;
    Ok(ora.0.sdk_token().await?.expose().to_owned())
}

/// The bridge's reports: the capability probe and the SDK's events.
#[tauri::command]
async fn sdk_event<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    event: SdkEvent,
) -> Result<(), OraError> {
    main_only(&window)?;
    let playback = ora.0.playback();
    playback.apply(event);
    SDK_FRAME_ALLOWED.store(playback.drm_available(), Ordering::SeqCst);
    Ok(())
}

/// The bridge registers the channel its player commands arrive on.
#[tauri::command]
async fn sdk_attach<R: Runtime>(
    window: Window<R>,
    bridge: State<'_, Bridge>,
    on_command: Channel<PlayerCommand>,
) -> Result<(), OraError> {
    main_only(&window)?;
    *bridge
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(on_command);
    Ok(())
}

/// Sends the current playback status, then every change, until the page goes away.
#[tauri::command]
async fn subscribe_playback<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    on_status: Channel<PlaybackStatus>,
) -> Result<(), OraError> {
    main_only(&window)?;
    let mut updates = ora.0.playback().subscribe();
    let first = updates.borrow_and_update().clone();
    on_status.send(first).map_err(|_| OraError::Unsupported)?;
    tauri::async_runtime::spawn(async move {
        while updates.changed().await.is_ok() {
            let next = updates.borrow_and_update().clone();
            if on_status.send(next).is_err() {
                break;
            }
        }
    });
    Ok(())
}

/// Starts playback on this app's SDK device (a pasted Spotify link, or the last thing played).
#[tauri::command]
async fn play_here<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    link: Option<String>,
) -> Result<(), OraError> {
    main_only(&window)?;
    ora.0.play_here(link.as_deref()).await
}

/// Play/pause, seek, next, previous, volume and disconnect run in the SDK on this device;
/// shuffle and repeat go through the Web API.
#[tauri::command]
async fn playback_command<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    bridge: State<'_, Bridge>,
    command: PlayerCommand,
) -> Result<(), OraError> {
    main_only(&window)?;
    match ora.0.player_command(command).await? {
        Some(command) => bridge.send(command),
        None => Ok(()),
    }
}

/// Opens an `https://open.spotify.com/...` page in the system browser. Nothing else.
#[tauri::command]
async fn open_in_spotify<R: Runtime>(window: Window<R>, url: String) -> Result<(), OraError> {
    main_only(&window)?;
    let page = browser::spotify_page(&url).ok_or(OraError::Unsupported)?;
    SystemBrowser.open(&page).map_err(|_| OraError::Unsupported)
}
