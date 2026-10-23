//! The `ora` Tauri plugin. It exposes the session in `ora-core` to the app's main
//! window as a few narrow commands and status streams. The only token that crosses into
//! JavaScript is the access token `access_token` hands to the Web Playback SDK's
//! `getOAuthToken`, through the sdk-bridge.

use ora_core::{
    browser::{self, Browser, SystemBrowser},
    http::ReqwestClient,
    store::KeyringStore,
    types::{
        CatalogPage, Device, Feature, OraError, PlayRequest, PlaybackStatus, PlayerCommand,
        PlaylistPage, PlaylistSummary, Profile, QueueView, SdkEvent, SearchKind, SearchResults,
        Status, TrackPage,
    },
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

/// The origin of Spotify's Web Playback SDK script and its playback frame. The app allows it in
/// its CSP (`script-src`, `frame-src`) and nowhere else.
pub const SDK_ORIGIN: &str = "https://sdk.scdn.co";
/// The SDK's playback frame. Navigation handlers that also see sub-frames (macOS, Linux) may
/// allow it while `sdk_frame_allowed` is true.
pub const SDK_FRAME_URL: &str = "https://sdk.scdn.co/embedded/index.html";

static SDK_FRAME_ALLOWED: AtomicBool = AtomicBool::new(false);

/// Whether the capability probe found DRM, so the SDK's frame may load on this system.
pub fn sdk_frame_allowed() -> bool {
    SDK_FRAME_ALLOWED.load(Ordering::SeqCst)
}

/// What the app supplies when registering the plugin.
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
    /// `http://127.0.0.1/callback`, so the app registers `http://127.0.0.1:<port>/callback`.
    pub callback_port: u16,
}

/// The plugin, registered by the app with `.plugin(ora_tauri::init(config))`.
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
            playback_command,
            refresh_playback,
            list_devices,
            transfer_playback,
            refresh_liked,
            toggle_liked,
            playlists,
            playlist_tracks,
            liked_tracks,
            saved_albums,
            saved_shows,
            saved_episodes,
            album_tracks,
            show_episodes,
            saved_states,
            set_saved,
            queue,
            add_to_queue,
            recently_played,
            create_playlist,
            add_to_playlist,
            remove_from_playlist,
            move_in_playlist,
            playlist_order,
            set_playlist_order,
            search,
            search_page,
            play
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
    let moved_away = matches!(event, SdkEvent::State { state: None });
    let playback = ora.0.playback();
    playback.apply(event);
    SDK_FRAME_ALLOWED.store(playback.drm_available(), Ordering::SeqCst);
    if moved_away {
        // Playback left this app: show the device it went to.
        reconcile(&ora.0);
    }
    Ok(())
}

fn reconcile(core: &Arc<Core>) {
    let core = Arc::clone(core);
    tauri::async_runtime::spawn(async move { core.reconcile_playback().await });
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

/// While this app plays, play/pause, seek, next, previous, volume and disconnect run in the
/// SDK, and shuffle and repeat go through the Web API. Another active device is controlled
/// entirely through the Web API, and a refresh follows to confirm the result.
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
        None => {
            reconcile(&ora.0);
            Ok(())
        }
    }
}

/// Reads which device is active and what it plays (on focus, at a low rate, and on events).
#[tauri::command]
async fn refresh_playback<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
) -> Result<(), OraError> {
    main_only(&window)?;
    ora.0.refresh_playback().await
}

/// The account's Spotify Connect devices, for the device picker.
#[tauri::command]
async fn list_devices<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
) -> Result<Vec<Device>, OraError> {
    main_only(&window)?;
    ora.0.list_devices().await
}

/// Moves playback to the device behind a key from `list_devices`.
#[tauri::command]
async fn transfer_playback<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    key: String,
) -> Result<(), OraError> {
    main_only(&window)?;
    ora.0.transfer_playback(&key).await?;
    reconcile(&ora.0);
    Ok(())
}

/// Reads whether the track playing now is in Liked Songs; the playback status carries it.
#[tauri::command]
async fn refresh_liked<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
) -> Result<(), OraError> {
    main_only(&window)?;
    ora.0.refresh_liked().await
}

/// The `+` control. Returns whether the track is liked afterwards.
#[tauri::command]
async fn toggle_liked<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
) -> Result<bool, OraError> {
    main_only(&window)?;
    ora.0.toggle_liked().await
}

/// One page of the account's playlists (`offset` from the previous page).
#[tauri::command]
async fn playlists<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    offset: u32,
) -> Result<PlaylistPage, OraError> {
    main_only(&window)?;
    ora.0.playlists(offset).await
}

/// One page of a playlist's tracks and episodes.
#[tauri::command]
async fn playlist_tracks<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    id: String,
    offset: u32,
) -> Result<TrackPage, OraError> {
    main_only(&window)?;
    ora.0.playlist_tracks(&id, offset).await
}

/// One page of Liked Songs.
#[tauri::command]
async fn liked_tracks<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    offset: u32,
) -> Result<TrackPage, OraError> {
    main_only(&window)?;
    ora.0.liked_tracks(offset).await
}

/// The first page of each kind of result. A newer search makes this one return `Cancelled`.
#[tauri::command]
async fn search<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    query: String,
) -> Result<SearchResults, OraError> {
    main_only(&window)?;
    ora.0.search(&query).await
}

/// One page of one kind of result, for "See all".
#[tauri::command]
async fn search_page<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    query: String,
    kind: SearchKind,
    offset: u32,
) -> Result<CatalogPage, OraError> {
    main_only(&window)?;
    ora.0.search_page(&query, kind, offset).await
}

/// One page of saved albums.
#[tauri::command]
async fn saved_albums<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    offset: u32,
) -> Result<CatalogPage, OraError> {
    main_only(&window)?;
    ora.0.saved_albums(offset).await
}

/// One page of saved shows (podcasts).
#[tauri::command]
async fn saved_shows<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    offset: u32,
) -> Result<CatalogPage, OraError> {
    main_only(&window)?;
    ora.0.saved_shows(offset).await
}

/// One page of saved episodes.
#[tauri::command]
async fn saved_episodes<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    offset: u32,
) -> Result<TrackPage, OraError> {
    main_only(&window)?;
    ora.0.saved_episodes(offset).await
}

/// One page of an album's tracks.
#[tauri::command]
async fn album_tracks<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    id: String,
    offset: u32,
) -> Result<TrackPage, OraError> {
    main_only(&window)?;
    ora.0.album_tracks(&id, offset).await
}

/// One page of a show's episodes.
#[tauri::command]
async fn show_episodes<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    id: String,
    offset: u32,
) -> Result<TrackPage, OraError> {
    main_only(&window)?;
    ora.0.show_episodes(&id, offset).await
}

/// Whether each track, album, show or episode is saved, in the order asked.
#[tauri::command]
async fn saved_states<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    uris: Vec<String>,
) -> Result<Vec<bool>, OraError> {
    main_only(&window)?;
    ora.0.saved_states(&uris).await
}

/// Saves the items to the library (a track: Liked Songs), or removes them.
#[tauri::command]
async fn set_saved<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    uris: Vec<String>,
    saved: bool,
) -> Result<(), OraError> {
    main_only(&window)?;
    ora.0.set_saved(&uris, saved).await
}

/// What plays now and next on the active device.
#[tauri::command]
async fn queue<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
) -> Result<QueueView, OraError> {
    main_only(&window)?;
    ora.0.queue().await
}

/// Adds a track or episode to the active device's queue.
#[tauri::command]
async fn add_to_queue<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    uri: String,
) -> Result<(), OraError> {
    main_only(&window)?;
    ora.0.add_to_queue(&uri).await
}

/// The tracks played most recently, newest first.
#[tauri::command]
async fn recently_played<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
) -> Result<TrackPage, OraError> {
    main_only(&window)?;
    ora.0.recently_played().await
}

/// Creates a private playlist with this name.
#[tauri::command]
async fn create_playlist<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    name: String,
) -> Result<PlaylistSummary, OraError> {
    main_only(&window)?;
    ora.0.create_playlist(&name).await
}

/// Adds tracks and episodes to the end of a playlist the account owns or collaborates on.
#[tauri::command]
async fn add_to_playlist<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    id: String,
    uris: Vec<String>,
) -> Result<(), OraError> {
    main_only(&window)?;
    ora.0.add_to_playlist(&id, &uris).await
}

/// Removes every copy of these tracks and episodes from a playlist.
#[tauri::command]
async fn remove_from_playlist<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    id: String,
    uris: Vec<String>,
) -> Result<(), OraError> {
    main_only(&window)?;
    ora.0.remove_from_playlist(&id, &uris).await
}

/// Moves one item of a playlist from position `from` to position `to`.
#[tauri::command]
async fn move_in_playlist<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    id: String,
    from: u32,
    to: u32,
) -> Result<(), OraError> {
    main_only(&window)?;
    ora.0.move_in_playlist(&id, from, to).await
}

/// Melora's own order for the account's playlists (IDs); empty means Spotify's.
#[tauri::command]
async fn playlist_order<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
) -> Result<Vec<String>, OraError> {
    main_only(&window)?;
    ora.0.playlist_order()
}

/// Saves Melora's own order for the account's playlists; an empty list goes back to Spotify's.
#[tauri::command]
async fn set_playlist_order<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    ids: Vec<String>,
) -> Result<(), OraError> {
    main_only(&window)?;
    ora.0.set_playlist_order(&ids)
}

/// Plays from a list on the active device, or on this app's own. A refresh follows, so a
/// remote device's new track shows.
#[tauri::command]
async fn play<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
    request: PlayRequest,
) -> Result<(), OraError> {
    main_only(&window)?;
    ora.0.play(request).await?;
    reconcile(&ora.0);
    Ok(())
}

/// Opens an `https://open.spotify.com/...` page in the system browser. Nothing else.
#[tauri::command]
async fn open_in_spotify<R: Runtime>(window: Window<R>, url: String) -> Result<(), OraError> {
    main_only(&window)?;
    let page = browser::spotify_page(&url).ok_or(OraError::Unsupported)?;
    SystemBrowser.open(&page).map_err(|_| OraError::Unsupported)
}
