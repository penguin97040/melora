mod navigation;
mod preferences;
mod window_layout;

use preferences::{Preferences, Store};
use serde::Serialize;
use std::sync::Mutex;
use tauri::Manager;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    version: &'static str,
    platform: &'static str,
    preferences: Preferences,
    storage_error: Option<String>,
}

#[tauri::command]
fn window_chrome(window: tauri::WebviewWindow) -> Result<&'static str, String> {
    if window.label() != "main" {
        return Err("Only the main application window can request window controls.".into());
    }
    Ok(if cfg!(target_os = "macos") {
        "macos"
    } else {
        "custom"
    })
}

#[tauri::command]
fn app_info(store: tauri::State<'_, Mutex<Store>>) -> Result<AppInfo, String> {
    let store = store
        .lock()
        .map_err(|_| "Preferences are temporarily unavailable.")?;
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
        preferences: store.preferences.clone(),
        storage_error: store.error.clone(),
    })
}

#[tauri::command]
fn save_preferences(
    preferences: Preferences,
    store: tauri::State<'_, Mutex<Store>>,
) -> Result<Preferences, String> {
    store
        .lock()
        .map_err(|_| "Preferences are temporarily unavailable.")?
        .save(preferences)
}

#[tauri::command]
fn show_main_window(
    window: tauri::WebviewWindow,
    startup: tauri::State<'_, Mutex<window_layout::StartupWindow>>,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Only the main application window can be shown.".into());
    }
    startup
        .lock()
        .map_err(|_| "The application window is temporarily unavailable.")?
        .reveal(|| {
            // Complete all opening geometry while hidden. No resize handler may
            // re-centre the window after show, including queued startup events.
            window_layout::fit_opening_window(&window)
                .and_then(|()| window.show())
                .map_err(|_| "The application window could not be shown.".to_string())?;
            // A window created hidden must also receive native keyboard focus.
            // Failure to activate an already visible window must not repeat geometry.
            if window.set_focus().is_err() {
                eprintln!("Melora could not focus the application window.");
            }
            Ok(())
        })
}

// Preferences and the Spotify Client ID (`spotify.json`) share this folder.
fn settings_directory(_app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    {
        Some(std::env::current_exe().ok()?.parent()?.join("data"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        _app.path().app_data_dir().ok()
    }
}

pub fn run() {
    let result = tauri::Builder::default()
        .plugin(ora_tauri::init(ora_tauri::OraConfig {
            service: "app.melora.desktop",
            app_name: "Melora",
            client_id_variable: "MELORA_SPOTIFY_CLIENT_ID",
            settings_dir: settings_directory,
            // Register http://127.0.0.1:43821/callback in the Spotify Developer Dashboard.
            callback_port: 43821,
        }))
        .setup(|app| {
            let directory = settings_directory(app.handle())
                .ok_or("Cannot locate the portable application folder.")?;
            // Windows portable: WebView2's profile, including the playback SDK's storage, stays
            // in `data\webview` beside the executable instead of %LOCALAPPDATA%.
            #[cfg(target_os = "windows")]
            let webview_data = directory.join("webview");
            app.manage(Mutex::new(Store::open(directory)));
            app.manage(Mutex::new(window_layout::StartupWindow::default()));
            let config = app
                .config()
                .app
                .windows
                .first()
                .ok_or("Missing main window configuration")?;
            let dev_url = if cfg!(dev) {
                app.config().build.dev_url.clone()
            } else {
                None
            };
            let builder = tauri::WebviewWindowBuilder::from_config(app.handle(), config)?
                .on_navigation(move |url| {
                    navigation::allowed(url, dev_url.as_ref(), ora_tauri::sdk_frame_allowed())
                })
                .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny);
            #[cfg(target_os = "windows")]
            let builder = builder.data_directory(webview_data);
            let window = builder.build()?;
            window_layout::fit_opening_window(&window)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            window_chrome,
            app_info,
            save_preferences,
            show_main_window
        ])
        .run(tauri::generate_context!());
    if result.is_err() {
        eprintln!("Melora could not start. Check that the system WebView and application data folder are available.");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    /// The remote (`https://`) sources of one CSP directive.
    fn remote_sources(directive: &str) -> Vec<String> {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let csp = config["app"]["security"]["csp"].as_str().unwrap();
        let sources = csp
            .split(';')
            .map(str::trim)
            .find(|d| d.split_whitespace().next() == Some(directive))
            .unwrap_or_else(|| panic!("{directive} missing"));
        sources
            .split_whitespace()
            .skip(1)
            .filter(|source| source.contains("://") && !source.starts_with("http://ipc."))
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn csp_allows_exactly_the_image_host_the_profile_keeps() {
        assert_eq!(
            remote_sources("img-src"),
            [format!("https://{}", ora_tauri::core::api::IMAGE_HOST)]
        );
    }

    #[test]
    fn csp_allows_only_the_playback_sdk_as_remote_script_and_frame() {
        assert_eq!(remote_sources("script-src"), [ora_tauri::SDK_ORIGIN]);
        assert_eq!(remote_sources("frame-src"), [ora_tauri::SDK_ORIGIN]);
        assert!(ora_tauri::SDK_FRAME_URL.starts_with(&format!("{}/", ora_tauri::SDK_ORIGIN)));
        // All Spotify HTTP is done in Rust, so the page itself connects nowhere else.
        for directive in ["default-src", "connect-src", "style-src", "font-src"] {
            assert!(remote_sources(directive).is_empty(), "{directive}");
        }
    }
}
