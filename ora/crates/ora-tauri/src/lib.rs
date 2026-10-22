//! The `ora` Tauri plugin. It exposes the shared session in `ora-core` to the client's main
//! window as a few narrow commands and a status stream. Tokens never cross into JavaScript.

use ora_core::{
    browser::{self, Browser, SystemBrowser},
    http::ReqwestClient,
    store::KeyringStore,
    types::{OraError, Profile, Status},
    Options, Ora,
};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tauri::{
    ipc::Channel,
    plugin::{Builder, TauriPlugin},
    AppHandle, Manager, Runtime, State, Window,
};

pub use ora_core as core;

const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);

type Core = Ora<ReqwestClient, KeyringStore, SystemBrowser>;

struct Shared(Arc<Core>);

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
            open_in_spotify
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

#[tauri::command]
async fn sign_in<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
) -> Result<Status, OraError> {
    main_only(&window)?;
    ora.0.sign_in().await
}

#[tauri::command]
async fn cancel_sign_in<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
) -> Result<Status, OraError> {
    main_only(&window)?;
    Ok(ora.0.cancel_sign_in())
}

#[tauri::command]
async fn sign_out<R: Runtime>(
    window: Window<R>,
    ora: State<'_, Shared>,
) -> Result<Status, OraError> {
    main_only(&window)?;
    ora.0.sign_out().await
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

/// Opens an `https://open.spotify.com/...` page in the system browser. Nothing else.
#[tauri::command]
async fn open_in_spotify<R: Runtime>(window: Window<R>, url: String) -> Result<(), OraError> {
    main_only(&window)?;
    let page = browser::spotify_page(&url).ok_or(OraError::Unsupported)?;
    SystemBrowser.open(&page).map_err(|_| OraError::Unsupported)
}
