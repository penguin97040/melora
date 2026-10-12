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

fn settings_directory(_app: &tauri::App) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    #[cfg(target_os = "windows")]
    {
        let exe = std::env::current_exe()?;
        Ok(exe
            .parent()
            .ok_or("Cannot locate the portable application folder.")?
            .join("data"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(_app.path().app_data_dir()?)
    }
}

pub fn run() {
    let result = tauri::Builder::default()
        .setup(|app| {
            app.manage(Mutex::new(Store::open(settings_directory(app)?)));
            if let Some(window) = app.get_webview_window("main") {
                window_layout::initialise(&window)?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![app_info, save_preferences])
        .run(tauri::generate_context!());
    if result.is_err() {
        eprintln!("Melora could not start. Check that the system WebView and application data folder are available.");
        std::process::exit(1);
    }
}
